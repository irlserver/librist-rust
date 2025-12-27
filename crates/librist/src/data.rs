//! Data block handling for RIST streams.

use crate::types::ReceiverDataFlags;
use std::ptr::NonNull;

/// A block of data received from or to be sent over RIST.
///
/// For received data, the `DataBlock` owns the underlying buffer
/// and will free it when dropped.
///
/// # Example
///
/// ```no_run
/// use librist::DataBlock;
///
/// fn process_block(block: DataBlock) {
///     let payload = block.payload();
///     println!("Received {} bytes on port {}", payload.len(), block.virtual_dst_port());
///     
///     if block.is_discontinuity() {
///         println!("Warning: packet loss detected!");
///     }
/// }
/// ```
pub struct DataBlock {
    /// The underlying librist data block.
    block: NonNull<librist_sys::rist_data_block>,
    /// Whether this block was received (needs to be freed).
    received: bool,
}

impl DataBlock {
    /// Creates a DataBlock from a received raw pointer.
    ///
    /// # Safety
    ///
    /// The pointer must be non-null and point to a valid `rist_data_block`
    /// that was returned by `rist_receiver_data_read2`.
    pub(crate) fn from_received(block: *mut librist_sys::rist_data_block) -> Self {
        Self {
            block: NonNull::new(block).expect("received block should not be null"),
            received: true,
        }
    }

    /// Gets the payload data.
    ///
    /// Returns the raw bytes of the received data.
    pub fn payload(&self) -> &[u8] {
        unsafe {
            let block = self.block.as_ref();
            if block.payload.is_null() || block.payload_len == 0 {
                &[]
            } else {
                std::slice::from_raw_parts(block.payload as *const u8, block.payload_len)
            }
        }
    }

    /// Gets the NTP timestamp of this block.
    ///
    /// The timestamp is in NTP format (seconds since 1900-01-01).
    pub fn timestamp_ntp(&self) -> u64 {
        unsafe { self.block.as_ref().ts_ntp }
    }

    /// Gets the virtual source port.
    ///
    /// This is used for multiplexing multiple streams.
    pub fn virtual_src_port(&self) -> u16 {
        unsafe { self.block.as_ref().virt_src_port }
    }

    /// Gets the virtual destination port.
    ///
    /// This is used for multiplexing multiple streams.
    pub fn virtual_dst_port(&self) -> u16 {
        unsafe { self.block.as_ref().virt_dst_port }
    }

    /// Gets the flow ID.
    ///
    /// The flow ID identifies a specific data flow within a RIST session.
    pub fn flow_id(&self) -> u32 {
        unsafe { self.block.as_ref().flow_id }
    }

    /// Gets the sequence number.
    ///
    /// The sequence number is derived from the RTP sequence number
    /// and can be used to detect packet loss or reordering.
    pub fn sequence(&self) -> u64 {
        unsafe { self.block.as_ref().seq }
    }

    /// Gets the raw flags.
    pub fn flags(&self) -> u32 {
        unsafe { self.block.as_ref().flags }
    }

    /// Gets the receiver flags (for received blocks).
    pub fn receiver_flags(&self) -> ReceiverDataFlags {
        ReceiverDataFlags::from_bits_truncate(self.flags())
    }

    /// Checks if there was a discontinuity before this block.
    ///
    /// A discontinuity indicates that one or more packets were lost
    /// before this block.
    pub fn is_discontinuity(&self) -> bool {
        self.receiver_flags()
            .contains(ReceiverDataFlags::DISCONTINUITY)
    }

    /// Checks if this is the start of a flow buffer.
    pub fn is_flow_buffer_start(&self) -> bool {
        self.receiver_flags()
            .contains(ReceiverDataFlags::FLOW_BUFFER_START)
    }

    /// Checks if there was a buffer overflow.
    ///
    /// An overflow indicates that the receiver's buffer was full
    /// and some data may have been lost.
    pub fn is_overflow(&self) -> bool {
        self.receiver_flags().contains(ReceiverDataFlags::OVERFLOW)
    }

    /// Returns the raw pointer to the data block.
    ///
    /// # Safety
    ///
    /// The returned pointer is valid as long as this `DataBlock` exists.
    #[allow(dead_code)]
    pub(crate) fn as_raw(&self) -> *const librist_sys::rist_data_block {
        self.block.as_ptr()
    }
}

impl Drop for DataBlock {
    fn drop(&mut self) {
        if self.received {
            unsafe {
                let mut block = self.block.as_ptr();
                librist_sys::rist_receiver_data_block_free2(&mut block);
            }
        }
    }
}

// Safety: DataBlock owns its data and doesn't share mutable state
unsafe impl Send for DataBlock {}
unsafe impl Sync for DataBlock {}

impl std::fmt::Debug for DataBlock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DataBlock")
            .field("payload_len", &self.payload().len())
            .field("timestamp_ntp", &self.timestamp_ntp())
            .field("virt_src_port", &self.virtual_src_port())
            .field("virt_dst_port", &self.virtual_dst_port())
            .field("flow_id", &self.flow_id())
            .field("sequence", &self.sequence())
            .field("flags", &self.receiver_flags())
            .finish()
    }
}

/// A builder for creating data blocks to send.
///
/// This is used when sending data with additional metadata.
#[derive(Debug, Default)]
pub struct DataBlockBuilder {
    virt_dst_port: Option<u16>,
    ts_ntp: Option<u64>,
    seq: Option<u64>,
}

impl DataBlockBuilder {
    /// Creates a new data block builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the virtual destination port.
    pub fn virtual_dst_port(mut self, port: u16) -> Self {
        self.virt_dst_port = Some(port);
        self
    }

    /// Sets the NTP timestamp.
    pub fn timestamp_ntp(mut self, ts: u64) -> Self {
        self.ts_ntp = Some(ts);
        self
    }

    /// Sets the sequence number.
    ///
    /// Note: This requires the `USE_SEQ` flag to be set.
    pub fn sequence(mut self, seq: u64) -> Self {
        self.seq = Some(seq);
        self
    }

    /// Builds a raw data block for sending.
    ///
    /// The returned data block references the provided payload slice,
    /// which must remain valid until the send operation completes.
    pub(crate) fn build_raw<'a>(
        &self,
        payload: &'a [u8],
    ) -> (
        librist_sys::rist_data_block,
        std::marker::PhantomData<&'a [u8]>,
    ) {
        let mut flags = 0u32;
        if self.seq.is_some() {
            flags |= librist_sys::rist_data_block_sender_flags::RIST_DATA_FLAGS_USE_SEQ.0;
        }

        let block = librist_sys::rist_data_block {
            payload: payload.as_ptr() as *const std::os::raw::c_void,
            payload_len: payload.len(),
            ts_ntp: self.ts_ntp.unwrap_or(0),
            virt_src_port: 0,
            virt_dst_port: self.virt_dst_port.unwrap_or(crate::DEFAULT_VIRT_DST_PORT),
            peer: std::ptr::null_mut(),
            flow_id: 0,
            seq: self.seq.unwrap_or(0),
            flags,
            ref_: std::ptr::null_mut(),
        };

        (block, std::marker::PhantomData)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_data_block_builder() {
        let data = vec![0u8; 100];
        let builder = DataBlockBuilder::new()
            .virtual_dst_port(1234)
            .timestamp_ntp(12345678);

        let (block, _lifetime) = builder.build_raw(&data);
        assert_eq!(block.payload_len, 100);
        assert_eq!(block.virt_dst_port, 1234);
        assert_eq!(block.ts_ntp, 12345678);
    }
}
