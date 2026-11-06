//! Audio output: the instrument bank, the deterministic player and the ALSA
//! sink (`docs/reverse-engineering/braintellect2/audio-feedback.md`, "Wave
//! device and instrument 1002").
//!
//! Only [`sink`] touches an audio device. The bank is the original's own
//! `dll<id>.BT.dll`, imported once by [`import`] into the user's data
//! directory; it never lives in Git.

pub mod bank;
pub mod import;
pub mod player;
pub mod sink;
