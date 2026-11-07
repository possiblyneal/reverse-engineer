//! The parity-replay harness: the one seam every clone layer is validated
//! through. It loads the scrubbed baseline fixtures (`tests/fixtures/`), and
//! provides one observable-behavior assertion per layer. Tolerances live only
//! in [`tolerance`]. Each test binary pulls it in with `mod harness;`, so items a
//! given binary does not use are expected.
#![allow(dead_code)]

pub mod assertions;
pub mod audio;
pub mod channels;
pub mod fixtures;
pub mod peer;
pub mod run8;
pub mod sensor;
pub mod session;
pub mod synthetic_bank;
pub mod synthetic_soundfont;
pub mod tolerance;
pub mod transport;
