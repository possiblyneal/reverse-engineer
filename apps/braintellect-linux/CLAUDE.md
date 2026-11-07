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
  unresolved — the 110 and 112 bytes the app never reads, and the 131-138 mode
  and gain readings (inferred) — are not claimed as known. Implemented so far (`src/acquisition/`): the `decode` arithmetic
  (the sensor gain is not floored; the live app runs with 10), command encoding
  (`command`), the 32-byte frame parser with counter and `sampleValidation`
  error counters (`stream`), the handshake/start/stop/stream `Session` over any
  `Read + Write` (`session`), the decoded 110/112/131-138 replies (`info`), and
  the RFCOMM socket adapter (`rfcomm`). Frames
  surface 4 channels in resolved order (channel c reads payload slots c and 4+c,
  oldest first); the device family is read from the 110 reply (`StreamConfig.family`
  only overrides it; an unaccepted code is `Error::UnknownFamily`), the gains are
  supplied by the caller. `tests/parity_handshake.rs` replays the live handshakes
  (`handshake_live_{a,b}.txt`) and asserts decoded fields only. All four channels'
  microvolts are held bit exact to the original's logged volts (`assertions::channel_microvolts`):
  over the committed `consumed_*` fixtures in `tests/parity_acquisition.rs`, and over the
  whole #55 replay in the ignored `tests/replay_s55.rs` (`scripts/consumed-extract.py`
  output, `tmp/s55/replay`, `S55_REPLAY_DIR`).
- **Transport boundary.** Protocol logic runs over `Read + Write`; only
  `rfcomm::RfcommStream` touches Bluetooth, and CI never opens it. A live run
  (`scripts/run braintellect-linux -- ui --mac <MAC>`, or `-- --mac <MAC>` for a raw frame dump) uses the server's BlueZ, the adapter's only
  owner (`device-connection.md`, #38), and is manual.
- **Do not copy the original's bugs outside parity.** Reproduce intended behavior, not
  defects. Known defects to avoid: the `SoundManager` null-`"Wave"` dereference when no
  audio endpoint exists, and the validation-induced null cascades where the
  original assumes a valid license/device/calibration state and faults when it
  is absent. Handle missing devices, endpoints, and invalid input as errors.
- **Two feedback modes** (`feedback::Mode`, one explicit value passed to
  `Smart2Graph::new` / `TriggerPath::smart2` / `Threshold::smart2`; no global or flag).
  `Mode::Original` reproduces the original's feedback defects and is the mode of every
  parity test; parity is asserted in original mode only. `Mode::Fixed` is what a live
  session runs (`live::GraphStart::LIVE`) and corrects four defects: the threshold window erases one entry when a value
  leaves it, not every equal one (`multiset::erase(key)`, `audio-feedback.md`); the 45 Hz
  interference low-pass takes over at the first graph sample (`Mode::interference_switch`),
  not a thread-timed one; every band-pass uses the all-scalar-head 16-tap-mean sum order
  (`set_scalar_heads` is ignored in fixed mode); `NoteSelection` reads Evaluator code 112 as
  channel 3 and 113 as channel 4, not the channel 3 and 4 average (`audio-feedback.md`,
  "Channels 3n4 note codes"). The measured run-8 decision difference
  between the modes is in `audio-feedback.md`, "Fixed mode against original mode".
- **One seam: the parity-replay harness** (`tests/harness/`). Every layer's tests
  (`tests/parity_<layer>.rs`) go through its loaders and per-layer assertions
  (command bytes, microvolts, note/trigger decisions, stored record) and assert
  observable behavior only. `tests/harness/transport.rs` is the fixture-fed
  in-memory transport; `tests/harness/inputs.rs` finds the ignored replays' out-of-Git inputs
  (an environment variable, else the repository's `tmp/`); `tests/parity_selfcheck.rs` proves each assertion is green on
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
  output (`process_evaluated_block`, or `process_filtered_block` with a logged code):
  `BandPass`, `PeakFrequency`, the `continuous` Aggregator, the `Evaluator`
  (`evaluator.rs`: the four Balance and two Coherence series from the Aggregators'
  averages, the decision in the block that ends each Aggregator epoch, the design's
  `aggregator_epoch_s`, by the Coherence then the Balance rule, default 111; a Balance
  that is not finite never wins), the Ch3/Ch4 `Coherence` elements (`coherence.rs`, on the port of the
  original's IPP order-9 real FFT in `ipp_fft.rs`, pinned bit for bit by
  `tests/fixtures/ipp_real_fft_512.txt`, the original kernel's output under Unicorn on a
  synthetic vector), `ExprTriggerAll`
  (`trigger_all`), the Threshold/Counter `TriggerPath` and `NoteSelection`. `Smart2Design` is filled from `smart2_design.tsv`;
  `Smart2Graph::new` refuses a rate other than `RATE_HZ` (500) or an aggregator epoch
  of 0. The run-6 and run-8 replays feed the IPP 16-tap sum-order flags
  (`set_scalar_head(s)`) and the counter clock (`BlockClock`) as inputs, never read
  from the host. The counter takes its clock as a `CounterClock`: one read per block for
  the epoch test and, only on a fire, a lap read that restarts its timer. `tests/parity_smart2_exercise.rs` overrides the design's
  interference1/2, low-pass and high-pass Source fields to the run-8 device settings,
  warms the Source chain over the 4000-sample pre-roll, and switches the 45 Hz
  low-pass in cold one sample after the graph start. The 25 Hz band-stop (`Notch`,
  `iir.rs`) runs the app's own two-biquad taps (transposed direct form II), not a
  designed filter, so it serves only the 23.5/25.75 Hz setting at 500 Hz and
  `SourceFilters::new` refuses it at another rate; its run-8
  amplitudes are asserted bit for bit over blocks 0 to 6599. Parity tests pick the logged or
  the evaluated code with `harness::run8::Codes`.
- **Circle-Balance report** (`src/feedback/circle_balance.rs`). `B-2-Circle-Balance-fromBAGSD-v5.1.1`
  is a post-session report (`BAGSD.report`) over a stored exercise's `High`/`Low` LMH series, not a
  live design, so it is not in the session window. `CircleBalanceDesign::circle_balance` is its
  wiring, asserted against `tests/fixtures/circle_balance_design.tsv` (`make-fixtures.py --src64`,
  `tests/parity_circle_balance.rs`); `report` runs the `TimeFilter` window, the `absolute` mean
  and the Balance Meter expression. Its values are reported only: no capture of the original's
  chart inputs exists, and the clone stores no LMH series. The ignored `tests/replay_s62.rs`
  prints them over `S62_LMH` (`scripts/lmh-extract.py` output from a decoded type-9 XML, under
  `tmp/`). Evidence and the capture needed: `audio-feedback.md`, "Circle-Balance design".
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
  test reads `BT_BANK_1002_DLL`.
  Tones (`audio::tone`, `audio::soundfont`; ADR 0006): `TONES` lists the feedback tones in
  set-up order, the original banks 1002 (`DEFAULT_TONE`, index 0), 1001 and 1003, then recorded
  Harp, Acoustic guitar and Xylophone. `braintellect-linux import-soundfont <file.sf2>` renders
  the recorded ones from General MIDI programs 46, 24 and 13 into
  `tone-<key>.bank` beside the banks (notes 40..76, 96 kHz, 0.8 s; every file is written under
  a temporary name and renamed into place only once all render and write; a SoundFont sample
  with no rate is refused). `Tone::availability` reports a never-imported tone as unavailable, and
  the window disables it instead of failing the session; `UiModel::select_tone` refuses an
  unavailable tone or a change while a session runs. The SoundFont is not committed
  (`tests/harness/synthetic_soundfont.rs` builds a synthetic one; `tests/audio_tone.rs`). Recovered behavior: `docs/reverse-engineering/braintellect2/audio-feedback.md`.
- **Live session engine** (`src/live.rs`; ADR 0005). `live::run`: `Session` over
  `Read + Write` -> `SourceFilters` -> `Smart2Graph` -> `audio::Player` -> an `AudioSink` (a
  trait; `AlsaSink` stays the only device code) -> `Store::save` when the session ends (stop,
  the length reached, the device closing the stream, or the window closing, which stops the
  worker and waits for the store). It stores only if a frame arrived; an error (link, audio,
  store) ends it unstored. It blocks, so it runs off the UI thread and reports a `Snapshot`
  (phase, link health, frames, `StreamStats`, per-channel RMS microvolts over the last second,
  triggers, the control code) through `Environment::report`, every 25 blocks and in the frame a
  trigger or a code change happens. The transport, sink, clock, stop flag and graph start
  (`Environment::graph`, a `GraphStart`) are injected; the clock is read only as the graph is
  built, once per block, on a counter fire and per quiet read, and any frame counts as data
  for the no-data health.
- **Live graph start and inputs.** Before the graph's first block the Source filters run
  `GraphStart::warmup_samples` of the stream with the interference filter off, as the
  original's did from connect to Play; a live session (`GraphStart::LIVE`) is fixed mode after
  500 samples (1 s), and the session length counts from the first block. The control code
  comes from the Evaluator (111 until its first full epoch pair; `Snapshot.control`, shown in
  the session window). The IPP scalar-head flags are all `true` in either mode: the original's
  sum order cannot be derived, so a live run fixes the order that departs less from run 8's
  logged one (`audio-feedback.md`, "Live sum order"; pinned by
  `tests/parity_smart2_exercise.rs`). The counter timer starts one epoch
  (`feedback::TRIGGER_EPOCH_MS`) before the clock reading taken as the graph is built, so the
  first block is already eligible, as in the original (`audio-feedback.md`, "Counter timer
  start"). The clinician sets the Source device settings (`SessionProperties`, default the
  run-8 baseline) and the length; the design is `Smart2Design::smart2`, checked against the
  design fixture in `tests/parity_feedback.rs`.
- **Stored series and BAGSD** (`feedback::StoredSeries`, `assess.rs`): the original's BAGSD
  design fed the Source output from the graph's first sample, after the warm-up: per channel
  `Band 10` and `Band 80`, one microvolt point every 5 s of stream time and a closing point at
  the stop (`session-storage.md`, "Stored series (#56)"). The ignored `tests/replay_s56.rs`
  holds them to the original's exercise 146 record within `RECORD_EPSILON`
  (`assertions::stored_series`; inputs `vol146.txt` and `b146/series.tsv` in `tmp/s56`,
  `S56_REPLAY_DIR`). The design's discrete FFT is `feedback::FftSpectrum`; the ignored
  `tests/replay_s59.rs` only reports its deviation from the stored `FFT-Normal-Order` vectors
  (`bagsd-design.md`, "Clone and parity"). BAGSD is a post-session design, so the session
  window does not offer it.
- **Sensor status** (`src/sensor.rs`): the Head2D evaluator's three fault tests, the
  `stateChangeDelay` debounce and the engine lifecycle, off while a session connects or runs
  (`ui::state::Event::Sensor` carries it). `sensor::SensorMonitor` is the pre-session feed
  (512-sample FFT blocks per channel, five band levels, then the evaluator). `live::monitor`
  runs it on an opened B-2 before a session (`ui::worker::Worker::spawn_check`, the "Check
  sensors" button, `Command::CheckSensors`), needs no tone, and returns why it could not run or
  broke off, which ends the check with that reason in the notice. The FFT stage is
  `feedback::DspFft` (`DSPFFT::process`: f32 Bartlett, `RealFft512`, the `rsqrtps` magnitude of
  `ipp_magnitude.rs`, the x87 scale); the spectra, levels and evaluator fault flag are all
  replay-asserted bit exact against the original's sensors-off capture (`sensor-status.md`).
  The `rsqrtps` table is Intel's. Tests: `tests/parity_sensor_status.rs`
  (synthetic fault tests and the sensors-off replay of `tests/fixtures/sensor_*.tsv`),
  `tests/parity_selfcheck.rs`, `tests/parity_sensor_monitor.rs` (`live::monitor` over the
  in-memory stream).
- **Session UI** (`src/ui/`). `ui::state::UiModel` is the pure model (actions and events in,
  `View` out), tested in `tests/ui_state.rs`; `ui::app` only draws it, and `ui::worker` is the
  device glue (RFCOMM with a 1 s read timeout, ALSA, the default store, the chosen tone);
  neither is unit-tested. The window offers the ADR 0004 import when the default tone
  (`DEFAULT_TONE`, found by key) is not imported. Entry point:
  `scripts/run braintellect-linux -- ui --mac <MAC>`; the window needs a display and loads
  X11/Wayland/GL at run time (eframe `glow` with `x11` and `wayland`, no `-dev` packages). On
  Debian the host needs `libxkbcommon-x11-0` (X11 start fails without it) and `libasound2`; the
  B-2 must be paired, bonded and trusted in BlueZ first (`btmgmt bondable on` before pairing, or
  the bond is not kept).
- **Live tests.** `tests/live_session.rs` runs the engine over the fixture transport (run-8 EEG
  framed by `tests/harness/transport.rs`) with `RecordingSink` and a synthetic bank. The
  ignored `tests/replay_live.rs` reads the #33 capture back through `live::run`
  (`S33_CAPTURE`, `S33_REPLAY_DIR`) in original mode with the original's graph start, switch
  and clock and lap reads, and asserts the original's per-block codes and the block and note
  of every trigger exactly (`assertions::trigger_notes`; `audio-feedback.md`, "Live replay of
  the #33 capture").
- **B-2 emulator and capture** (`src/emulate.rs`, `src/capture.rs`). The live session tees its raw
  RX to `<data dir>/captures/b2-<secs>.rx` (`capture::Recording`). `braintellect-linux emulate
  <capture> <socket>` plays a B-2 over a Unix socket (the QEMU serial chardev): `Command::decode`
  recognises the app's requests and `PowerOff`, replies are replayed byte for byte, frames recorded
  before the last reply (a B-2 left streaming) are dropped, the rest stream from Start at 250/s and stop on Stop, and nothing is sent on power-off. It is tested over
  `Read + Write` with `tests/harness/peer.rs` (`tests/emulator.rs`); the VM rig and the run-8
  replay result are in `docs/reverse-engineering/braintellect2/replay-rig.md`.
  `scripts/s48-prep.cmd` and `scripts/s48-run.cmd` are the guest-side steps of that rig.
  `scripts/replay-extract.py` reduces a replay hook log to `tmp/s48/replay/` (or the #53 log `tmp/s53/replay/`, `S48_REPLAY_DIR`), and the ignored
  `tests/replay_s48.rs` diffs the clone's decisions on that stream against the original's
  (via `harness::run8::replay_from`) and asserts the logged A..H amplitudes, every logged Balance and
  Coherence value and the per-block Evaluator codes exactly; it locates
  the unlogged graph start, 45 Hz low-pass switch sample and A..H band to band-pass object ranks
  from those amplitudes (`audio-feedback.md`, "Replay start-up"; the extract keeps all 18 objects'
  flags and the complete logged block prefix, up to 30001); inputs are vendor-derived and stay
  outside Git.
- **Fixtures are scrubbed, no-wearer, and regenerated.** `tests/fixtures/` is
  committed so CI replays headlessly. It is derived only from no-wearer,
  sensors-off captures, minimal, with the B-2 serial replaced by `SCRUBBEDSN00` and
  no MAC, host id, machine/profile/user name, or timestamp. Regenerate it only with
  `python3 -I scripts/make-fixtures.py`; its docstring maps each raw capture under the
  repo's `tmp/` (the live handshakes under `tmp/s58`, `--src58`) to its flag and fixtures. Raw captures stay in `tmp/`, never in Git;
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
