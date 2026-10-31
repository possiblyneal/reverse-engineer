## Purpose

Durable, non-sensitive technical findings from authorized analysis of local reverse-engineering targets.

## Ownership

| Path | What |
|---|---|
| `braintellect2-audio-feedback.md` | Evidence-backed BRAINtellect2 EEG-to-audio and database-recovery findings, plus the `SoundManager` session-setup crash (no host audio device → null `"Wave"` driver deref at RVA `0x485fc`) and its QEMU HDA-device remedy. |
| `braintellect2-device-connection.md` | How the app picks the B-2 Bluetooth COM port (`B-2-BTCOMPort` preference, `COM3` default), why it is not DB/registry/file persisted, the stale/default-port failure mode, the `findPortName` MAC resolver, the post-connect host-registration gate (validator `0x41b9d0`, host-ID check at fo `0x1aee8`) and its minimal NOP bypass, and that holding the COM port open keeps the controller awake. |
| `braintellect2-b2-protocol.md` | Static reconstruction of the B-2 serial wire protocol for a Linux client: 9600 8N1 RFCOMM transport; the five fixed `!*#` ASCII commands (101/110/112/131/48); the 101 serial-number handshake; the `processData_v5` sample-stream framing (0x64 sync, 16-bit BE counter, 24-bit BE signed samples, lead-off path); and the full ADC→volts formula `rawADC·D1/(channelGain·D2·sensorGain)` with per-family constants. Lists the runtime-built commands and reply layouts left unresolved pending live capture. |

## Local Contracts

- Do not put patient, session, account, or other personal data in this directory.
- Separate observed configuration from inferred behavior; cite the source artifact and method for every material claim.
- Treat raw exports, SQL dumps, decoded session records, keys, and decrypted design payloads as sensitive analysis materials. Keep them out of Git and under the repository `tmp/` directory.

## Work Guidance

- Record only reproducible technical findings that remain useful beyond the active session.
- Preserve important uncertainty explicitly; do not convert an unverified implementation detail into a fact.

## Verification

- Review reports for accidental sensitive-data disclosure before committing or sharing.

## Child Index

None.
