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
  (`scripts/run braintellect-linux -- ui --mac <MAC>`, or `-- --mac <MAC>` for a raw frame dump) needs the adapter free of the
  Windows VM (pairing on the host invalidates the VM's bond) and is manual.
- **Do not copy the original's bugs.** Reproduce intended behavior, not defects.
  Known defects to avoid: the `SoundManager` null-`"Wave"` dereference when no
  audio endpoint exists, and the validation-induced null cascades where the
  original assumes a valid license/device/calibration state and faults when it
  is absent. Handle missing devices, endpoints, and invalid input as errors.
- **One seam: the parity-replay harness** (`tests/harness/`). Every layer's tests
  (`tests/parity_<layer>.rs`) go through its loaders and per-layer assertions
  (command bytes, microvolts, note/trigger decisions, stored record) and assert
  observable behavior only. `tests/harness/transport.rs` is the fixture-fed
  in-memory transport; `tests/parity_selfcheck.rs` proves each assertion is green on
  the baseline and red on a wrong output.
- **Exact match only** (operator decision). Tolerances (`SAMPLE_EPSILON_UV`,
  `TIMING_WINDOW_MS`, `RECORD_EPSILON`) are defined once in
  `tests/harness/tolerance.rs`; never restate them, and add no relative or correlation
  tolerance. A stage is asserted only once it matches, and stays a reported figure
  until then. `note_decisions_match_the_baseline_exactly`
  (`tests/parity_smart2_exercise.rs`) holds every Note decision of the run-8 exercise to
  the baseline exactly.
- **Acquisition behavior** (`src/acquisition/`): the frame parser checks the header,
  then the counter, then the sample bits, in the original's order; `Session` skips
  stream frames still arriving before a handshake reply; a clean end of stream sends
  no Stop.
- **Feedback graph** (`src/feedback/`). The caller runs `SourceFilters` (`source.rs`),
  built from `Smart2Design::source`; `Smart2Graph` (`graph.rs`) runs from the Source
  output (`process_filtered_block`): `BandPass`, `PeakFrequency`, the `continuous`
  Aggregator, `ExprTriggerAll` (`trigger_all`), the Threshold/Counter `TriggerPath`
  and `NoteSelection`. `Smart2Design` is filled from `smart2_design.tsv`;
  `Smart2Graph::new` refuses a rate other than `RATE_HZ` (500) or an aggregator epoch
  of 0. The run-6 and run-8 replays feed the IPP 16-tap sum-order flags
  (`set_scalar_head(s)`) and the counter clock (`BlockClock`) as inputs, never read
  from the host. `tests/parity_smart2_exercise.rs` overrides the design's
  interference1/2, low-pass and high-pass Source fields to the run-8 device settings,
  warms the Source chain over the 4000-sample pre-roll, and switches the 45 Hz
  low-pass in cold one sample after the graph start. The 25 Hz band-stop (`Notch`,
  `iir.rs`) runs the app's own two-biquad taps (transposed direct form II), not a
  designed filter, so it serves only the 23.5/25.75 Hz setting at 500 Hz and
  `SourceFilters::new` refuses it at another rate; its run-8
  amplitudes are asserted bit for bit over blocks 0 to 6599. The ignored
  `end_to_end_note_decisions_match_the_baseline` (`tests/parity_smart2_stages.rs`)
  fails because run 6 did not log the counter lap read. Still unimplemented: the
  Evaluator (its code is a per-block input).
- **Session storage** (`src/storage.rs`): all sessions live in ONE local SQLite
  database (`rusqlite`, bundled SQLite), at `$XDG_DATA_HOME/braintellect-linux/sessions.db`,
  else `~/.local/share/braintellect-linux/sessions.db` (a relative
  `XDG_DATA_HOME` is ignored); `Store::open(path)` overrides
  it (tests use a scratch path). The schema is normalized (`session` header and
  duration, `channel`, `series`, `point`) and keeps the fields and meaning the original
  recorded (`docs/reverse-engineering/braintellect2/session-storage.md`), not its MySQL
  or Java-serialized, encrypted blobs. `PRAGMA user_version` is the schema version
  (`SCHEMA_VERSION`); another version is refused, never migrated. Offline: no network,
  and the original's records are never read or migrated.
- **Audio output** (`src/audio/`): `bank` parses an original `dll<id>.BT.dll` Qt resource into
  note to PCM (note = trailing decimal of the resource name; shape-based, so it needs no
  fixed offsets); `player` is deterministic (trigger decisions in, events and mixed PCM
  out, no gate of its own, whole samples unscaled, overlaps summed and saturated);
  `import` copies the owner's banks with `braintellect-linux import <original lib dir>` into
  `$XDG_DATA_HOME/braintellect-linux/instruments/` (else `~/.local/share/...`, a relative
  `XDG_DATA_HOME` ignored; the rule is shared with storage via `storage::resolve_data_dir`).
  `import` refuses a source that is the destination (`ImportError::SameDirectory`; copying
  onto itself would truncate the banks). `audio::load_instrument(dir, id)` reports a bank that
  was never imported as `InstrumentError::Missing`, naming the path and the import command; a
  sample rate of 0 is a `BankError`. The player sounds every trigger, repeated notes included
  (the original's `+0x2cf` check is `playOnChange`, false in SMART-2); the 40..76 note clamp is
  upstream in `feedback/note.rs`.
  Only `sink::AlsaSink` touches a device (libasound loaded with `dlopen`); tests never open
  it and nothing plays audio. The bank bytes are never committed: `tests/parity_audio.rs` and
  the audio tests use synthetic banks (`tests/harness/synthetic_bank.rs`); the ignored real-bank
  test reads `BT_BANK_1002_DLL`. Recovered behavior: `docs/reverse-engineering/braintellect2/audio-feedback.md`.
- **Live session and UI** (`src/live.rs`, `src/ui/`; ADR 0005). `live::run` is the session
  engine: `Session` over `Read + Write` -> `SourceFilters` -> `Smart2Graph` -> `audio::Player`
  -> an `AudioSink` (a trait; `AlsaSink` stays the only device code) -> `Store::save` when the
  session ends (stop, the length reached, or the device closing the stream). It stores only
  if a frame arrived, and an error (link, audio, store) ends it unstored. It blocks, so it
  runs off the UI thread and reports a `Snapshot` (phase, link health, frames, `StreamStats`,
  per-channel RMS microvolts over the last second, triggers) through `Environment::report`;
  the transport, sink, clock and stop flag are all injected. Live inputs the clone does not
  derive: every block runs under the Evaluator's `defaultControlValue`
  (`live::DEFAULT_CONTROL`, 111; the Evaluator is unimplemented), the IPP scalar-head flags
  stay all `false`, and the counter timer starts at the clock reading taken as the graph is
  built, both of its reads being the block's. The clinician sets the Source device settings
  (`SessionProperties`, default the run-8 baseline) and the length; the design is
  `Smart2Design::smart2`, checked against the design fixture in `tests/parity_feedback.rs`.
  The stored series are one point per band-pass amplitude (microvolts) every 5 s of stream
  time under `Channel<n>` / the band label; which signals the original's 16 stored series
  hold is unresolved. The UI shows signal level and stream health, never the original's
  "Ready"/"Adjust Fit" verdict (`sensor-status.md`, "Unresolved"). `ui::state::UiModel` is the
  pure model (actions and events in, `View` out) and is tested in `tests/ui_state.rs`;
  `ui::app` only draws it, and `ui::worker` is the device glue (RFCOMM with a 1 s read
  timeout, ALSA, the default store, bank 1002); neither is unit-tested. The window offers the
  ADR 0004 import when `load_instrument` reports `Missing`. Entry point:
  `scripts/run braintellect-linux -- ui --mac <MAC>`; the window needs a display and loads
  X11/Wayland/GL at run time (eframe `glow` with `x11` and `wayland`, no `-dev` packages).
  `tests/live_session.rs` runs the engine over the fixture transport (run-8 EEG framed by
  `tests/harness/transport.rs`) with `RecordingSink` and a synthetic bank.
- **B-2 emulator and capture** (`src/emulate.rs`, `src/capture.rs`). The live session tees its raw
  RX to `<data dir>/captures/b2-<secs>.rx` (`capture::Recording`). `braintellect-linux emulate
  <capture> <socket>` plays a B-2 over a Unix socket (the QEMU serial chardev): `Command::decode`
  recognises the app's requests and `PowerOff`, replies are replayed byte for byte, frames stream
  from Start at 250/s and stop on Stop, and nothing is sent on power-off. It is tested over
  `Read + Write` with `tests/harness/peer.rs` (`tests/emulator.rs`); the VM rig and the run-8
  replay result are in `docs/reverse-engineering/braintellect2/replay-rig.md`.
  `scripts/s48-prep.cmd` and `scripts/s48-run.cmd` are the guest-side steps of that rig.
  `scripts/replay-extract.py` reduces a replay hook log to `tmp/s48/replay/`, and the ignored
  `tests/replay_s48.rs` diffs the clone's decisions on that stream against the original's
  (via `harness::run8::replay_from`) and asserts the logged E..H amplitudes bit for bit; it locates
  the unlogged graph start and 45 Hz low-pass switch sample from those amplitudes, and
  `replay-extract.py` carries the replay's own band to band-pass object ranks (`audio-feedback.md`,
  "Replay start-up"); inputs are vendor-derived and stay outside Git.
- **Fixtures are scrubbed, no-wearer, and regenerated.** `tests/fixtures/` is
  committed so CI replays headlessly. It is derived only from no-wearer,
  sensors-off captures, minimal, with the B-2 serial replaced by `SCRUBBEDSN00` and
  no MAC, host id, machine/profile/user name, or timestamp. Regenerate it only with
  `python3 -I scripts/make-fixtures.py`; its docstring maps each raw capture under the
  repo's `tmp/` to its flag and fixtures. Raw captures stay in `tmp/`, never in Git;
  every fixture file stays under 500 KB. Grep the fixtures for the MAC and serial
  before committing.
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
