//! The real transport: a Linux `AF_BLUETOOTH`/`BTPROTO_RFCOMM` socket bound to
//! the B-2's MAC (`b2-protocol.md`, Transport). A thin adapter; all protocol
//! logic lives behind `Read + Write` in [`super::session`].
//!
//! The kernel RFCOMM socket carries no baud rate: 9600 8N1 is the controller's
//! serial side. Needs a paired, bonded adapter; not exercised by CI.

use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::time::Duration;

/// The SPP serial channel the app's COM port maps to is not recovered; SPP
/// devices commonly use channel 1.
pub const DEFAULT_CHANNEL: u8 = 1;

/// `BTPROTO_RFCOMM` from `<bluetooth/bluetooth.h>` (the libc crate omits it).
const BTPROTO_RFCOMM: libc::c_int = 3;

/// `struct sockaddr_rc` from `<bluetooth/rfcomm.h>`.
#[repr(C)]
struct SockaddrRc {
    family: libc::sa_family_t,
    bdaddr: [u8; 6],
    channel: u8,
}

/// Parse `AA:BB:CC:DD:EE:FF` into the kernel's little-endian `bdaddr_t`.
///
/// # Errors
/// `InvalidInput` unless it is six colon-separated hex bytes.
pub fn parse_mac(mac: &str) -> io::Result<[u8; 6]> {
    let bad = || io::Error::new(io::ErrorKind::InvalidInput, format!("invalid MAC: {mac}"));
    let mut out = [0u8; 6];
    let mut parts = mac.split(':');
    for slot in out.iter_mut().rev() {
        *slot = u8::from_str_radix(parts.next().ok_or_else(bad)?, 16).map_err(|_| bad())?;
    }
    if parts.next().is_some() {
        return Err(bad());
    }
    Ok(out)
}

/// A connected RFCOMM stream to the B-2.
pub struct RfcommStream(File);

impl RfcommStream {
    /// Connect to the controller at `mac` on RFCOMM `channel`.
    ///
    /// # Errors
    /// Fails on a bad MAC or when the socket cannot be created or connected.
    pub fn connect(mac: &str, channel: u8) -> io::Result<Self> {
        let bdaddr = parse_mac(mac)?;
        // SAFETY: plain socket(2) call; the result is checked before use.
        let fd = unsafe { libc::socket(libc::AF_BLUETOOTH, libc::SOCK_STREAM, BTPROTO_RFCOMM) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `fd` is a fresh, valid descriptor owned by nothing else.
        let owned = unsafe { OwnedFd::from_raw_fd(fd) };
        let addr = SockaddrRc {
            family: libc::sa_family_t::try_from(libc::AF_BLUETOOTH).expect("family fits"),
            bdaddr,
            channel,
        };
        // SAFETY: `addr` is a valid sockaddr_rc and the length matches its size.
        let rc = unsafe {
            libc::connect(
                fd,
                std::ptr::from_ref(&addr).cast::<libc::sockaddr>(),
                u32::try_from(std::mem::size_of::<SockaddrRc>()).expect("small struct"),
            )
        };
        if rc < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(File::from(owned)))
    }
}

impl RfcommStream {
    /// Make a read that finds no data wait at most `timeout`, then fail with
    /// `WouldBlock`, so a caller can notice a quiet link and a stop request.
    ///
    /// # Errors
    /// Fails if the socket option cannot be set.
    pub fn set_read_timeout(&self, timeout: Duration) -> io::Result<()> {
        let tv = libc::timeval {
            tv_sec: libc::time_t::try_from(timeout.as_secs()).unwrap_or(libc::time_t::MAX),
            tv_usec: libc::suseconds_t::from(timeout.subsec_micros()),
        };
        // SAFETY: `tv` is a valid timeval and the length matches its size.
        let rc = unsafe {
            libc::setsockopt(
                self.0.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_RCVTIMEO,
                std::ptr::from_ref(&tv).cast(),
                u32::try_from(std::mem::size_of::<libc::timeval>()).expect("small struct"),
            )
        };
        if rc < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

impl Read for RfcommStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }
}

impl Write for RfcommStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mac_parses_little_endian_and_rejects_malformed() {
        assert_eq!(
            parse_mac("00:11:22:AA:bb:FF").unwrap(),
            [0xFF, 0xBB, 0xAA, 0x22, 0x11, 0x00]
        );
        assert!(parse_mac("00:11:22:33:44").is_err());
        assert!(parse_mac("00:11:22:33:44:55:66").is_err());
        assert!(parse_mac("00:11:22:33:44:ZZ").is_err());
    }
}
