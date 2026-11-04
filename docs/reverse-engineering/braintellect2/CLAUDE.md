## Purpose

Durable, non-sensitive findings from authorized analysis of **BRAINtellect2**
(`braintellect.exe` v5.1.57) and its B-2 device — the reverse-engineering target
the `apps/braintellect-linux` clone reproduces.

## Ownership

| Path | What |
|---|---|
| `b2-protocol.md` | Static reconstruction of the B-2 serial wire protocol for a Linux client: 9600 8N1 RFCOMM transport; the five fixed `!*#` ASCII commands (101/110/112/131/48); the 101 serial-number handshake; the `processData_v5` sample-stream framing (0x64 sync, 16-bit BE counter, 24-bit BE signed samples, lead-off path); and the full ADC→volts formula `rawADC·D1/(channelGain·D2·sensorGain)` with per-family constants. Includes the 2026-11-03 live-session confirmation (500 Hz, 4 active channels, 16-byte frames, DataCapture = raw wire frames). Lists the runtime-built commands and reply layouts left unresolved pending live capture. |
| `device-connection.md` | Verified connect runbook (power-cycle → launch → authenticated bond → re-pair if needed → connect) with a symptom→cause→fix table, plus: how the app picks the B-2 Bluetooth COM port (`B-2-BTCOMPort` preference, `COM3` default), why it is not DB/registry/file persisted, the stale/default-port failure mode, the `findPortName` MAC resolver, the post-connect host-registration gate (validator `0x41b9d0`, host-ID check at fo `0x1aee8`) and its minimal NOP bypass, and that holding the COM port open keeps the controller awake. |
| `audio-feedback.md` | Evidence-backed BRAINtellect2 EEG-to-audio and database-recovery findings, plus the `SoundManager` session-setup crash (no host audio device → null `"Wave"` driver deref at RVA `0x485fc`) and its QEMU HDA-device remedy. |
| `session-storage.md` | How a completed session persists: embedded MySQL 5.5/InnoDB behind a Hibernate-shaped 34-table schema; the session→exercise→exercisedata record chain; the Java-serialized `byte[]` blob container; type 9 = length-prefixed zlib XML (cleartext feedback series + FFT, the SMART-2 parity baseline); types 0/1/5 = AES-128-CFB (fixed hardcoded key+IV via libgcrypt `Crypt` class, inlined with extraction sites) decrypting to `exerciseHeader`/`flowDesign`/`flowControlMessage` XML. |

## Local Contracts

Inherits every contract in the parent `../CLAUDE.md` (sensitive-data handling,
observed-vs-inferred, Ghidra via CLI).

- Do not ask the operator to power-cycle the B-2 to fix a connect or pairing failure. A
  power-cycle has not been shown to fix one: the real fixes were the outgoing COM port and a
  stale bond (`device-connection.md`). Diagnose remotely first.

## Child Index

- `literature/CLAUDE.md` — peer-reviewed HIRREM/Cereset corpus and independent
  EEG→audio feedback references; background for the feedback the clone reproduces.
