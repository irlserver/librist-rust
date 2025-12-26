//! Async wrappers for RIST sender and receiver.
//!
//! This module provides async-friendly wrappers around the synchronous
//! `RistSender` and `RistReceiver` types, enabling use with Tokio.
//!
//! # Example
//!
//! ```no_run
//! use librist::{AsyncRistReceiver, AsyncRistSender, Profile, RistReceiver, RistSender};
//! use futures_util::StreamExt;
//!
//! #[tokio::main]
//! async fn main() -> librist::Result<()> {
//!     // Create and wrap sender
//!     let sender = RistSender::builder()
//!         .profile(Profile::Main)
//!         .build()?;
//!     sender.add_peer("rist://192.168.1.100:5000")?;
//!     sender.start()?;
//!     let async_sender = AsyncRistSender::new(sender);
//!
//!     // Create and wrap receiver
//!     let receiver = RistReceiver::builder()
//!         .profile(Profile::Main)
//!         .build()?;
//!     receiver.add_peer("rist://@:5000")?;
//!     receiver.start()?;
//!     let mut async_receiver = AsyncRistReceiver::new(receiver, 1024);
//!
//!     // Use as async stream
//!     while let Some(block) = async_receiver.next().await {
//!         println!("Received {} bytes", block.payload().len());
//!         async_sender.send(block.payload()).await?;
//!     }
//!
//!     Ok(())
//! }
//! ```

use crate::data::DataBlock;
use crate::error::{Error, Result};
use crate::{RistReceiver, RistSender};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

/// Async wrapper for `RistSender`.
///
/// Since librist's send operation is non-blocking (it just queues data),
/// this wrapper is lightweight and mostly provides async compatibility.
///
/// # Thread Safety
///
/// `AsyncRistSender` is `Send + Sync` and can be cloned and shared
/// between tasks.
#[derive(Clone)]
pub struct AsyncRistSender {
    inner: Arc<RistSender>,
}

impl AsyncRistSender {
    /// Creates a new async sender wrapping the given sender.
    ///
    /// The sender should already be configured and started.
    pub fn new(sender: RistSender) -> Self {
        Self {
            inner: Arc::new(sender),
        }
    }

    /// Wraps an existing Arc'd sender.
    pub fn from_arc(sender: Arc<RistSender>) -> Self {
        Self { inner: sender }
    }

    /// Sends data asynchronously.
    ///
    /// This is effectively non-blocking since librist's send queues data
    /// internally. For bulk sends with many packets, consider using
    /// `send_bulk` to avoid holding up the executor.
    pub async fn send(&self, data: &[u8]) -> Result<usize> {
        // librist's send is quick (just queues), so direct call is fine
        self.inner.send(data)
    }

    /// Sends data to a specific virtual destination port.
    pub async fn send_to_port(&self, data: &[u8], virt_dst_port: u16) -> Result<usize> {
        self.inner.send_to_port(data, virt_dst_port)
    }

    /// Sends multiple packets, using spawn_blocking to avoid holding the executor.
    ///
    /// This is more efficient for bulk sends as it moves the work to a
    /// blocking thread pool.
    pub async fn send_bulk(&self, packets: Vec<Vec<u8>>) -> Result<usize> {
        let sender = self.inner.clone();
        tokio::task::spawn_blocking(move || {
            let mut total = 0;
            for data in packets {
                total += sender.send(&data)?;
            }
            Ok(total)
        })
        .await
        .map_err(|e| Error::Other(format!("Task join failed: {}", e)))?
    }

    /// Returns a reference to the underlying sender.
    pub fn inner(&self) -> &RistSender {
        &self.inner
    }

    /// Returns the underlying Arc'd sender.
    pub fn into_inner(self) -> Arc<RistSender> {
        self.inner
    }
}

/// Async wrapper for `RistReceiver` with channel-based delivery.
///
/// This wrapper spawns a background task that polls the receiver and
/// forwards data blocks through an async channel, enabling true async
/// operation without blocking the executor.
///
/// # Dropping
///
/// When dropped, the background polling task is automatically cancelled.
pub struct AsyncRistReceiver {
    inner: Arc<RistReceiver>,
    data_rx: mpsc::Receiver<DataBlock>,
    /// Handle to the background polling task (dropped = cancelled)
    _poll_handle: JoinHandle<()>,
}

impl AsyncRistReceiver {
    /// Creates a new async receiver wrapping the given receiver.
    ///
    /// # Arguments
    ///
    /// * `receiver` - The receiver to wrap (should be started)
    /// * `buffer_size` - Size of the async channel buffer
    ///
    /// # Example
    ///
    /// ```no_run
    /// use librist::{AsyncRistReceiver, RistReceiver, Profile};
    ///
    /// let receiver = RistReceiver::builder()
    ///     .profile(Profile::Main)
    ///     .build()?;
    /// receiver.add_peer("rist://@:5000")?;
    /// receiver.start()?;
    ///
    /// // Buffer up to 1024 blocks
    /// let async_receiver = AsyncRistReceiver::new(receiver, 1024);
    /// # Ok::<(), librist::Error>(())
    /// ```
    pub fn new(receiver: RistReceiver, buffer_size: usize) -> Self {
        let inner = Arc::new(receiver);
        let (tx, rx) = mpsc::channel(buffer_size);

        let receiver_clone = inner.clone();
        let poll_handle = tokio::task::spawn_blocking(move || {
            loop {
                // Use short timeout to allow checking if channel is closed
                match receiver_clone.recv(100) {
                    Ok(block) => {
                        if tx.blocking_send(block).is_err() {
                            // Channel closed, stop polling
                            break;
                        }
                    }
                    Err(Error::Timeout) => {
                        // Check if we should stop (channel closed)
                        if tx.is_closed() {
                            break;
                        }
                    }
                    Err(_) => {
                        // Other error, stop polling
                        break;
                    }
                }
            }
        });

        Self {
            inner,
            data_rx: rx,
            _poll_handle: poll_handle,
        }
    }

    /// Wraps an existing Arc'd receiver.
    pub fn from_arc(receiver: Arc<RistReceiver>, buffer_size: usize) -> Self {
        let (tx, rx) = mpsc::channel(buffer_size);

        let receiver_clone = receiver.clone();
        let poll_handle = tokio::task::spawn_blocking(move || loop {
            match receiver_clone.recv(100) {
                Ok(block) => {
                    if tx.blocking_send(block).is_err() {
                        break;
                    }
                }
                Err(Error::Timeout) => {
                    if tx.is_closed() {
                        break;
                    }
                }
                Err(_) => break,
            }
        });

        Self {
            inner: receiver,
            data_rx: rx,
            _poll_handle: poll_handle,
        }
    }

    /// Receives data asynchronously.
    ///
    /// Returns `None` when the receiver is closed or an error occurs.
    pub async fn recv(&mut self) -> Option<DataBlock> {
        self.data_rx.recv().await
    }

    /// Receives data with a timeout.
    ///
    /// # Errors
    ///
    /// Returns `Error::Timeout` if no data is received within the timeout.
    pub async fn recv_timeout(&mut self, timeout: Duration) -> Result<DataBlock> {
        tokio::time::timeout(timeout, self.data_rx.recv())
            .await
            .map_err(|_| Error::Timeout)?
            .ok_or_else(|| Error::Other("Channel closed".into()))
    }

    /// Attempts to receive data without waiting.
    ///
    /// Returns `Ok(None)` if no data is immediately available.
    pub fn try_recv(&mut self) -> Result<Option<DataBlock>> {
        match self.data_rx.try_recv() {
            Ok(block) => Ok(Some(block)),
            Err(mpsc::error::TryRecvError::Empty) => Ok(None),
            Err(mpsc::error::TryRecvError::Disconnected) => {
                Err(Error::Other("Channel closed".into()))
            }
        }
    }

    /// Returns a reference to the underlying receiver.
    pub fn inner(&self) -> &RistReceiver {
        &self.inner
    }

    /// Returns the underlying Arc'd receiver.
    ///
    /// Note: The background polling task will continue running.
    pub fn inner_arc(&self) -> Arc<RistReceiver> {
        self.inner.clone()
    }
}

/// Implements `Stream` for use with `StreamExt` combinators.
impl futures_core::Stream for AsyncRistReceiver {
    type Item = DataBlock;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.data_rx).poll_recv(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Profile;

    #[test]
    fn test_async_sender_creation() {
        // Just test that we can create the types
        let sender = RistSender::builder().profile(Profile::Main).build().unwrap();

        let async_sender = AsyncRistSender::new(sender);
        assert!(!async_sender.inner().is_started());
    }

    #[test]
    fn test_async_sender_clone() {
        let sender = RistSender::builder().profile(Profile::Main).build().unwrap();

        let async_sender1 = AsyncRistSender::new(sender);
        let async_sender2 = async_sender1.clone();

        // Both should point to same inner
        assert!(std::ptr::eq(
            async_sender1.inner() as *const _,
            async_sender2.inner() as *const _
        ));
    }

    #[tokio::test]
    async fn test_async_sender_send_not_started() {
        let sender = RistSender::builder().profile(Profile::Main).build().unwrap();

        let async_sender = AsyncRistSender::new(sender);

        // Should fail because not started
        let result = async_sender.send(b"test").await;
        assert!(matches!(result, Err(Error::NotStarted)));
    }
}
