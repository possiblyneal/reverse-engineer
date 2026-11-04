## Purpose

The BRAINtellect2 Linux clone: a from-scratch Rust reimplementation of the
BRAINtellect2 neurofeedback application for Debian Linux, driving the B-2 EEG
controller over Bluetooth RFCOMM. It is built, run, and tested in-repo as the
reverse-engineering effort's parity oracle; it ships no release artifact
(`.unit.json`: `run: longlived`, `ships: none`). See
`docs/adrs/0003-host-linux-clone-as-run-and-test-unit.md`.

## Ownership

Owns the Rust clone and its parity tests. The protocol and behavior it
reproduces are owned by `docs/reverse-engineering/`; this unit does not restate
those findings, it cites them.

## Local Contracts

- **Parity is the correctness contract.** Every layer is validated by diffing
  its observable output (bytes on the wire, decoded microvolts, note/trigger
  decisions, stored record fields) against the captured real-app baseline, never
  by inspection of internal structure.
- **No guessing past the evidence.** The protocol source of truth is
  `docs/reverse-engineering/braintellect2/b2-protocol.md`. Items it marks
  unresolved — the `setStart` command framing, the 110/112/131 reply layouts,
  and the inbound packet's trailer/length and `sampleValidation` check — are not
  implemented until the T2 baseline capture resolves them. Implemented so far:
  the verified `decode` arithmetic only.
- **Do not copy the original's bugs.** Reproduce intended behavior, not defects.
  Known defects to avoid: the `SoundManager` null-`"Wave"` dereference when no
  audio endpoint exists, and the validation-induced null cascades where the
  original assumes a valid license/device/calibration state and faults when it
  is absent. Handle missing devices, endpoints, and invalid input as errors.
- **Layers** (each a parity-gated vertical slice): RFCOMM acquisition core →
  SMART-2 feedback graph → audio output → session storage → session UI.

## Work Guidance

- Rust edition 2021; the gate runs `cargo clippy -- -D warnings`, so keep it
  warning-clean.
- Cite the originating VA (or the protocol-doc section) in a comment for any
  constant or formula recovered from the binary.

## Verification

- `scripts/check` runs the Rust capabilities: `cargo fmt --check`,
  `cargo clippy -- -D warnings`, `cargo test`, `cargo build --locked`.

## Child Index

None.
