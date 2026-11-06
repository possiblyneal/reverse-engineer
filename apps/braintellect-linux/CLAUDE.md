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
  unresolved — the 110/112/131 reply layouts,
  the device family the B-2 reports, and the 110/112 payload meaning — are not
  implemented. Implemented so far (`src/acquisition/`): the `decode` arithmetic
  (the sensor gain is not floored; the live app runs with 10), command encoding
  (`command`), the 32-byte frame parser with counter and `sampleValidation`
  error counters (`stream`), the handshake/start/stop/stream `Session` over any
  `Read + Write` (`session`), and the RFCOMM socket adapter (`rfcomm`). Frames
  surface 4 channels in resolved order (channel c reads payload slots c and 4+c,
  oldest first); the device family and gains are supplied by the caller, not read from the device.
- **Transport boundary.** Protocol logic runs over `Read + Write`; only
  `rfcomm::RfcommStream` touches Bluetooth, and CI never opens it. A live run
  (`scripts/run braintellect-linux -- --mac <MAC>`) needs the adapter free of the
  Windows VM (pairing on the host invalidates the VM's bond) and is manual.
- **Do not copy the original's bugs.** Reproduce intended behavior, not defects.
  Known defects to avoid: the `SoundManager` null-`"Wave"` dereference when no
  audio endpoint exists, and the validation-induced null cascades where the
  original assumes a valid license/device/calibration state and faults when it
  is absent. Handle missing devices, endpoints, and invalid input as errors.
- **One seam: the parity-replay harness** (`tests/harness/`). Every layer's tests
  (`tests/parity_<layer>.rs`) go through its loaders and per-layer assertions
  (command bytes, microvolts, note/trigger decisions, stored record) and assert
  observable behavior only. Tolerances (`SAMPLE_EPSILON_UV`, `TIMING_WINDOW_MS`,
  `RECORD_EPSILON`) are defined once in `tests/harness/tolerance.rs`; never
  restate them. No relative or correlation tolerance: a stage is asserted only
  once it matches within these, and stays a reported figure until then
  (operator decision). `tests/parity_selfcheck.rs` proves each assertion is green on the
  baseline and red on a wrong output. `tests/harness/transport.rs` is the
  fixture-fed in-memory transport that replays handshake replies and the stream.
  `tests/parity_feedback.rs` replays the threshold-stage input through
  `src/feedback/` (`Threshold` then `Counter`, via `TriggerPath`) and matches the
  pass flags and counter triggers. The `smart2_*` fixtures (the run-6 baseline,
  `tmp/s43/baseline/`) replay `ExprTriggerAll`, the smoother and the 15000-sample
  min/max tracker with the note map (`NoteSelection`) from the logged amplitudes and
  PeakFrequencies. `Smart2Graph` (`src/feedback/graph.rs`) runs the stages upstream of
  those: the Source chain (`source.rs`: 25 Hz band-stop, 150 Hz FIR low-pass, optional
  45 Hz low-pass, 1.5 Hz high-pass; `iir.rs`, `fir.rs`), `BandPass`, `PeakFrequency`
  and the `continuous` Aggregator (the `Smoother`), all parameterised by
  `Smart2Design`, which `smart2_design.tsv` fills from the design XML.
  `tests/parity_smart2_stages.rs` asserts bit for bit (`f32`) the Source chain
  (`smart2_source_stages.tsv`; the 45 Hz low-pass `Interference1::LowPass45` is a
  device preference, not a design field), the Band 10/80 FIR output, the
  `OutAmplitude` 16-tap mean and PeakFrequency (`smart2_bandpass_stages.tsv`), the
  Aggregator, and, over the first 400 blocks, every band, aggregate and PeakFrequency
  bin of the graph (`smart2_stages.tsv`), every block's note after the first 15
  (pre-capture smoothing history), the notes at all 229 baseline fires and the
  trigger decisions. The 16-tap mean is the IPP `ippsFIR_32f` kernel's exact `f32`
  sum order (`BandPass::mean`): a SIMD order, or a scalar head for the first two
  outputs of a block when the destination is 8 mod 16. The heap alignment is not
  logged, so `make-fixtures.py` recovers it per block from the logged amplitude and
  `smart2_stage_inputs.tsv`/`smart2_bandpass_stages.tsv` carry it as flags the replay
  feeds (`set_scalar_head(s)`). `tests/parity_smart2_stages.rs` replays the graph from
  the live Source output (`process_filtered_block`). The ignored
  `end_to_end_note_decisions_match_the_baseline` fails because run 6 did not log the
  counter's lap read (`BlockClock::lap_ms`). `tests/parity_smart2_exercise.rs` replays
  the whole run-8 exercise from the decoded EEG the app consumed (`run8_*` fixtures),
  cold at the graph's first sample, with the logged Evaluator code, `BlockClock` (the
  counter's evaluation and lap reads) and sum-order flags as inputs (the counter
  clock is an input, never read from the host). It asserts the 415 Trigger decisions
  within the harness window, the Note decisions apart from the six known deviations
  (fires 0, 1, 3, 4, 7 and 89: the app's start-up transient and one borderline
  pass), and bit-exact E..H band amplitudes over blocks 1000 to 2999. The strict
  `note_decisions_match_the_baseline_exactly` stays ignored until the app's start-up
  state is reproduced. `ExprTriggerAll` is total over the design's Evaluator codes
  (`trigger_all`). Still unimplemented: the Evaluator (its code is a per-block input).
  `tests/parity_storage.rs` replays the record fixture through `src/storage.rs`
  (`tests/harness/session.rs` maps the flat fixture to and from `SessionRecord`).
- **Session storage** (`src/storage.rs`): all sessions live in ONE local SQLite
  database (`rusqlite`, bundled SQLite), at `$XDG_DATA_HOME/braintellect-linux/sessions.db`,
  else `~/.local/share/braintellect-linux/sessions.db`; `Store::open(path)` overrides
  it (tests use a scratch path). The schema is normalized (`session` header and
  duration, `channel`, `series`, `point`) and keeps the fields and meaning the original
  recorded (`docs/reverse-engineering/braintellect2/session-storage.md`), not its MySQL
  or Java-serialized, encrypted blobs. `PRAGMA user_version` is the schema version
  (`SCHEMA_VERSION`); another version is refused, never migrated. Offline: no network,
  and the original's records are never read or migrated.
- **Fixtures are scrubbed, no-wearer, and regenerated.** `tests/fixtures/` is
  committed so CI replays headlessly. It is derived only from no-wearer,
  sensors-off captures, minimal (a short stream window, a handful of record
  fields), with the B-2 serial replaced by the placeholder `SCRUBBEDSN00` and no
  MAC, host id, machine/profile/user name, or timestamp. Regenerate it only with
  `python3 -I scripts/make-fixtures.py` from the raw capture in the repo's
  `tmp/s26/run2/` (handshake, stream, record), `tmp/s26/run3/` (feedback) and
  `tmp/s26/run4/` (`smart2_design.tsv`, via `--src4`), `tmp/s43/live/` (`--src5`,
  `smart2_source_stages.tsv`) and `tmp/s43/baseline/` (`--baseline`: the other
  `smart2_*` fixtures, from the last run in `decisions9.log` and `hook9s.log`) and
  `tmp/s43/run8/run8.bin` (`--src8`: the `run8_*` fixtures, consumed EEG as 24-bit
  counts with no serial bytes, clocks as ms from block 0); raw captures stay in `tmp/`,
  never in Git, and every fixture file stays under 500 KB. Grep the fixtures
  for the MAC and serial before committing.
- **Layers** (each a parity-gated vertical slice): RFCOMM acquisition core →
  SMART-2 feedback graph → audio output → session storage → session UI.

## Work Guidance

- Rust edition 2021; the gate runs `cargo clippy -- -D warnings`, so keep it
  warning-clean.
- Cite the originating VA (or the protocol-doc section) in a comment for any
  constant or formula recovered from the binary.

## Verification

- `cargo test` replays the committed fixtures through the harness; the self-check is
  `tests/parity_selfcheck.rs`.
- `scripts/check` runs the Rust capabilities: `cargo fmt --check`,
  `cargo clippy -- -D warnings`, `cargo test`, `cargo build --locked`.

## Child Index

None.
