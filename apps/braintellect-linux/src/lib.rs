//! BRAINtellect2 Linux clone: a from-scratch Rust reimplementation of the
//! BRAINtellect2 neurofeedback application for Debian Linux, driving the B-2
//! EEG controller over Bluetooth RFCOMM.
//!
//! Correctness is behavioral parity against a captured real-app baseline, not
//! inspection (see `docs/adrs/0003-host-linux-clone-as-run-and-test-unit.md`).
//! Each layer is validated by diffing its observable output against that
//! baseline through one shared harness.
//!
//! The protocol source of truth is
//! `docs/reverse-engineering/braintellect2/b2-protocol.md`. Items that spec
//! marks unresolved (the 110/112/131 reply layouts and the device family
//! the B-2 reports) are **not** implemented from
//! guesses.

pub mod acquisition;

pub mod feedback;

pub mod storage;

pub mod audio;

pub mod live;

pub mod ui;
