//! TUN device and file-descriptor data path.
//!
//! librist can read from / write to an arbitrary file descriptor instead of
//! going through [`RistSender::send`](crate::RistSender::send) or the receiver
//! data callback. This is the lowest-latency path and is the basis for IP
//! tunneling: pair a [`Tun`] device with [`RistSender::set_data_fd`] on one end
//! and [`RistReceiver::set_data_fd`] on the other to forward raw IP packets
//! over RIST.
//!
//! The fd is never owned by librist. When using a raw fd you are responsible
//! for keeping it open until the context is destroyed; [`Tun`] handles that
//! lifetime for you via RAII.

use std::ffi::CString;

use crate::error::{Error, Result, check_result};

bitflags::bitflags! {
    /// Flags controlling how librist treats a data file descriptor.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct DataFdFlags: u32 {
        /// The fd is a TUN device: apply platform-specific TUN framing
        /// (e.g. the macOS utun 4-byte address-family header) transparently.
        const TUN = librist_sys::RIST_DATA_FD_FLAG_TUN;
    }
}

/// Cumulative I/O counters for the data fd path.
///
/// `tx` is the sender side (read from the data fd, sent over RIST); `rx` is the
/// receiver side (received over RIST, written to the data fd).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DataFdStats {
    /// Packets read from the sender's data fd and sent over RIST.
    pub tx_packets: u64,
    /// Bytes read from the sender's data fd and sent over RIST.
    pub tx_bytes: u64,
    /// Packets received over RIST and written to the receiver's data fd.
    pub rx_packets: u64,
    /// Bytes received over RIST and written to the receiver's data fd.
    pub rx_bytes: u64,
}

impl DataFdStats {
    pub(crate) fn from_raw(raw: &librist_sys::rist_data_fd_stats) -> Self {
        Self {
            tx_packets: raw.tx_packets,
            tx_bytes: raw.tx_bytes,
            rx_packets: raw.rx_packets,
            rx_bytes: raw.rx_bytes,
        }
    }
}

/// A layer-3 TUN device (IP packets only).
///
/// Cross-platform: macOS (utun), Linux (`/dev/net/tun`); on Windows the
/// underlying librist implementation is a stub. Opening and configuring a TUN
/// device requires root/admin privileges on most platforms.
///
/// The device is closed automatically on drop. Pass [`as_raw_fd`](Tun::as_raw_fd)
/// to [`RistSender::set_data_fd`](crate::RistSender::set_data_fd) or
/// [`RistReceiver::set_data_fd`](crate::RistReceiver::set_data_fd) with
/// [`DataFdFlags::TUN`]; keep the `Tun` alive for the lifetime of the context.
#[derive(Debug)]
pub struct Tun {
    fd: i32,
    name: String,
}

impl Tun {
    /// Opens a TUN device.
    ///
    /// `requested_name` is a desired interface name (e.g. `"rist0"` or
    /// `"utun5"`); pass `None` to let the OS assign one. The actually assigned
    /// name is available via [`name`](Tun::name).
    pub fn open(requested_name: Option<&str>) -> Result<Self> {
        let requested = match requested_name {
            Some(name) => Some(CString::new(name).map_err(|_| Error::InvalidStringLength)?),
            None => None,
        };
        let requested_ptr = requested.as_ref().map_or(std::ptr::null(), |s| s.as_ptr());

        // librist copies the assigned name into this buffer; IFNAMSIZ is 16 on
        // Linux, utun names are short, so 64 is comfortably large.
        let mut name_buf = [0u8; 64];
        let fd = unsafe {
            librist_sys::rist_tun_open(
                requested_ptr,
                name_buf.as_mut_ptr() as *mut std::os::raw::c_char,
                name_buf.len(),
            )
        };
        if fd < 0 {
            return Err(Error::Rist(fd));
        }

        let name = std::ffi::CStr::from_bytes_until_nul(&name_buf)
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();

        Ok(Self { fd, name })
    }

    /// Returns the interface name assigned to this device.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the raw file descriptor.
    ///
    /// The fd remains owned by this [`Tun`]; do not close it directly.
    pub fn as_raw_fd(&self) -> i32 {
        self.fd
    }

    /// Reads one IP packet from the device. Platform-specific framing is
    /// stripped transparently, so the buffer receives a raw IP packet.
    pub fn read(&self, buf: &mut [u8]) -> Result<usize> {
        let ret = unsafe { librist_sys::rist_tun_read(self.fd, buf.as_mut_ptr(), buf.len()) };
        check_result(ret)?;
        Ok(ret as usize)
    }

    /// Writes one IP packet to the device. Platform-specific framing is added
    /// transparently.
    pub fn write(&self, buf: &[u8]) -> Result<usize> {
        let ret = unsafe { librist_sys::rist_tun_write(self.fd, buf.as_ptr(), buf.len()) };
        check_result(ret)?;
        Ok(ret as usize)
    }

    /// Assigns an IP address and prefix length to the interface.
    pub fn set_ip(&self, ip: &str, prefix_len: u32) -> Result<()> {
        let dev = CString::new(self.name.as_str()).map_err(|_| Error::InvalidStringLength)?;
        let ip = CString::new(ip).map_err(|_| Error::InvalidStringLength)?;
        let ret =
            unsafe { librist_sys::rist_tun_set_ip(dev.as_ptr(), ip.as_ptr(), prefix_len as i32) };
        check_result(ret)
    }

    /// Sets the interface MTU.
    pub fn set_mtu(&self, mtu: u32) -> Result<()> {
        let dev = CString::new(self.name.as_str()).map_err(|_| Error::InvalidStringLength)?;
        let ret = unsafe { librist_sys::rist_tun_set_mtu(dev.as_ptr(), mtu as i32) };
        check_result(ret)
    }

    /// Brings the interface up (`IFF_UP`).
    pub fn bring_up(&self) -> Result<()> {
        let dev = CString::new(self.name.as_str()).map_err(|_| Error::InvalidStringLength)?;
        let ret = unsafe { librist_sys::rist_tun_bring_up(dev.as_ptr()) };
        check_result(ret)
    }
}

impl Drop for Tun {
    fn drop(&mut self) {
        unsafe { librist_sys::rist_tun_close(self.fd) };
    }
}
