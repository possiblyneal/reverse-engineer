//! B-2 controller acquisition layer: the 101/110/112 handshake, start/stop, and
//! decoding the inbound sample stream to microvolts, over any `Read + Write`
//! byte stream. The Bluetooth RFCOMM socket is one such transport
//! ([`rfcomm::RfcommStream`]); parity tests drive the same code from fixtures.
//!
//! Not implemented (the protocol doc marks them unresolved): the meaning of the
//! 110 pairs 1-2 and the 112 payload, deriving the device family from the
//! handshake, and power-off.

pub mod command;
pub mod decode;
pub mod rfcomm;
pub mod session;
pub mod stream;
