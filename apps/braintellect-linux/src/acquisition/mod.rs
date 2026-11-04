//! B-2 controller acquisition layer: Bluetooth RFCOMM transport, the 101/110/112
//! handshake, start/stop, and decoding the inbound sample stream to volts.
//!
//! Only the statically recovered, cross-verified arithmetic lives here so far:
//! the 24-bit sample decode, the lead-off reading, and the ADC->volts
//! conversion with its per-family constants. The transport, handshake, and
//! packet framing are gated on the T2 baseline capture.

pub mod decode;
