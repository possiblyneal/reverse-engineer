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
//! marks unresolved (the `setStart` command framing, the 110/112/131 reply
//! layouts, the device family the B-2 reports, and the inbound packet's
//! trailer/length and `sampleValidation` check) are **not** implemented from
//! guesses; they wait on the baseline capture (ticket T2).

pub mod acquisition;
