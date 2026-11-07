//! B-2 controller acquisition layer: the 101/110/112 handshake, start/stop, and
//! decoding the inbound sample stream to microvolts, over any `Read + Write`
//! byte stream. The Bluetooth RFCOMM socket is one such transport
//! ([`rfcomm::RfcommStream`]); parity tests drive the same code from fixtures.
//!
//! The 110, 112 and 131-138 replies are decoded ([`info`]) and the device family
//! is read from the 110 reply. Not implemented: the 112 payload and two 110 bytes
//! (unresolved in the protocol doc) and power-off.

pub mod command;
pub mod decode;
pub mod info;
pub mod rfcomm;
pub mod session;
pub mod stream;
