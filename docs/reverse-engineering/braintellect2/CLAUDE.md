## Purpose

Durable, non-sensitive findings from authorized analysis of **BRAINtellect2**
(`braintellect.exe` v5.1.57) and its B-2 device — the reverse-engineering target
the `apps/braintellect-linux` clone reproduces.

## Ownership

| Path | What |
|---|---|
| `b2-protocol.md` | Static reconstruction of the B-2 serial wire protocol for a Linux client: 9600 8N1 RFCOMM transport; the five fixed `!*#` ASCII commands (101/110/112/131/48); the 101 serial-number handshake; the observed 32-byte stream frame (`AC BC CC 03` marker, 16-bit BE counter, `03 03`, 8 x 24-bit BE signed samples, no trailer) with its `sampleValidation` rule, plus the unconfirmed static `processData_v5` reading (0x64 sync, placebo-mode branch); and the full ADC→volts formula `rawADC·D1/(channelGain·D2·sensorGain)` with per-family constants. Records the observed channel and sample order in the payload (slots `c` and `4 + c`, confirmed by run 8; order and volts conversion confirmed bit exact on all four channels by the #55 replay). Includes the 2026-11-03 live-session confirmation (500 Hz, 4 active channels, DataCapture 240000 B per 30 s) and the 2026-11-04 traced session (wire codes for start/stop/mode/injection/power-off, handshake reply layouts, family 12, calibration). Includes the decoded 110/112/131-138 handshake replies (live 2026-11-07; the device reports family 12 itself). Lists what remains unresolved: the unread 110/112 bytes, and how the `processData*` functions relate to the ring-buffer parser. |
| `device-connection.md` | How the app picks the B-2 Bluetooth COM port (`B-2-BTCOMPort` preference, `COM3` default), why it is not DB/registry/file persisted, the stale/default-port failure mode, the `findPortName` MAC resolver, the post-connect host-registration gate (validator `0x41b9d0`, host-ID check at fo `0x1aee8`) and its minimal NOP bypass, that holding the COM port open keeps the controller awake, that the one Bluetooth adapter has a single owner (the server's BlueZ, for the clone) because pairing on another stack is inferred to make the B-2 bond stale (#38), that the bond survives a host reboot and a B-2 power-cycle, the `0x17` pairing refusal with a passed-through adapter and its remote fix (re-enumerate the adapter through the host), and a connect symptom table. |
| `audio-feedback.md` | Evidence-backed BRAINtellect2 EEG-to-audio and database-recovery findings: the SMART-2 threshold (auto quantile, pass test), counter (`timeHigh` timing), and Evaluator (`coherenceChecker` control codes, `defaultControlValue` 111, the replay capture of its Balance inputs and bands A..D), the run-3 runtime capture of threshold pass flags and counter fires (quantile index `trunc(n*lower)`), the run-6 and run-8 baselines (the exact IPP 16-tap mean, the counter's two clock reads, the full `ExprTriggerAll` code table, and the whole-exercise replay from decoded EEG), the recovered PeakFrequency, BandPassFilter, Source software filter chain (notch, low-pass, high-pass) and Aggregator algorithms (static decompile, matched offline against run 4), the static `SoundElement` note hook points (unverified at runtime), the Wave instrument bank layout and note map, the playback gating, the replay start-up inputs (the 45 Hz low-pass switch offset and the per-process band to band-pass object ranks), plus the `SoundManager` session-setup crash (no host audio device → null `"Wave"` driver deref at RVA `0x485fc`) and its QEMU HDA-device remedy. |
| `sensor-status.md` | How the Optimize-tab sensor status ("Ready" / "Adjust Fit" / "Not Available") is computed: the decoded `BT-LOD-v5.1.11` design (4-channel FFT → Head2D, no lead-off input), the Head2D property map, the Image-overlay evaluator's three fault tests and `stateChangeDelay` debounce, the `setLODEngineState` / `LODEngineRunningSlot` lifecycle, why an empty sensor bar reads "Ready", and the levels checked against the original on a sensors-off replay (FFT scale, level offset, bin rule, line-noise start-up and 60 Hz choice, pair-fault blocks). |
| `session-storage.md` | How a completed session persists: embedded MySQL 5.5/InnoDB behind a Hibernate-shaped 34-table schema; the session→exercise→exercisedata record chain; the Java-serialized `byte[]` blob container; type 9 = length-prefixed zlib XML (cleartext feedback series + FFT, the SMART-2 parity baseline); types 0/1/5 = AES-128-CFB (fixed hardcoded key+IV via libgcrypt `Crypt` class, inlined with extraction sites) decrypting to `exerciseHeader`/`flowDesign`/`flowControlMessage` XML; a session with no sensors fitted still reads "Ready", runs, and saves, with no stored lead-off state. |
| `replay-rig.md` | Ticket #48: the `braintellect-linux emulate` B-2 emulator played into the original through a QEMU 16550 serial port (COM2): the emulator contract, the quadlet and guest-ini before/after, the run steps with the run-8 hook set, the results of replaying run 8 (the original's own decisions differ from run 8; the clone matches the replay 874/874 with E..H bit exact) and #33's live clone capture (734/734 over the 26022 blocks logged before the capture ran out), #55's all-channel consumed volts (the clone bit exact on channels 0 to 3), why a replay of a live capture stops short of the exercise end, and the host clock a replay needs (turbo on; the night `no_turbo` schedule caps the host at 800 MHz). |

## Local Contracts

Inherits every contract in the parent `../CLAUDE.md` (sensitive-data handling,
observed-vs-inferred, Ghidra via CLI).

- Do not ask the operator to power-cycle the B-2 to fix a connect or pairing failure. A
  power-cycle has not been shown to fix one: the real fixes were the outgoing COM port and a
  stale bond (`device-connection.md`). Diagnose remotely first: a `0x17` refusal is fixed by
  resetting the adapter from the host, and only a `PageTimeout` means the B-2 is asleep.

## Child Index

- `literature/CLAUDE.md` — peer-reviewed HIRREM/Cereset corpus and independent
  EEG→audio feedback references; background for the feedback the clone reproduces.
