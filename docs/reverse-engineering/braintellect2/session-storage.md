# BRAINtellect2 Session Storage

How the BRAINtellect2 app (`braintellect.exe` v5.1.57) persists a completed
training session, and the on-disk formats of the stored exercise results.

Source: a complete real-app SMART-2 session run with the B-2 connected in the
offline Windows VM on 2026-11-03 (session record `2026-11-03 15:56-B2-SMART-2`),
then the guest disk mounted read-only on the host and the MySQL datadir copied
and read with a throwaway `mysql:5.7` container. Raw exports are kept out of Git
under the repository `tmp/baseline-2026-11-03/` per this directory's contract.

## Observed: an embedded MySQL 5.5 / InnoDB store behind a Hibernate-shaped schema

- The app embeds MySQL (config `data/my.embedded.as.embedded.ini`:
  `basedir`/`datadir` under `C:/ProgramData/BST/BRAINtellect2/`,
  `default-storage-engine=INNODB`; `basedir` comment references
  `MySQLServer5.5`). All table data lives in the shared tablespace
  `data/ibdata1` (no per-table `.ibd`); only `.frm` files sit in
  `data/braintellectds/`.
- The database `braintellectds` has 34 tables with Hibernate-generated foreign
  key names (e.g. `FKA11C0E76D9E35B5`), a clinical ORM schema (`person`,
  `client`, `staff`, `session`, `exercise`, `exercisedata`, `montagesystem`,
  `placementsystem`, …). The serialized blobs (below) carry the Java
  object-stream magic, but no JVM or `.jar`/`.class` artifact exists on the guest
  (`find` over the disk): the native Qt `braintellect.exe` reads and writes the
  Java serialization format itself. The schema is a legacy-lineage artifact, not
  a running Java backend.
- `braintellectds.property` holds only 5 version-metadata rows and there is no
  `preference`/`setting`/`config` table (consistent with
  `device-connection.md`).

## Observed: the session → exercise → exercisedata record chain

A completed session writes three linked rows (verbatim column facts for the
2026-11-03 baseline; no personal fields reproduced):

- `session` (`sessionKey` 124): `ts`/`sessionDate` = session start,
  `name` = `Session-<date>`, `id` = a UUID, `clientKey` FK, `filesDirectory`
  **empty** (so raw data is in-DB, not external files),
  `placementSystemName` = "BST 10-10 placement system with selector".
- `exercise` (`exerciseKey` 124, FK `sessionKey`): `ts` = session end,
  `name` = `2026-11-03 15:56-B2-SMART-2-v5.1.2` (matches the Report-tab row),
  `montage` = `FP1FP2T3T4`, `design` = `BT-SMART-2-v5.1.2`, `duration` = 240
  (seconds = the 4:00 session), `properties` = a serialized blob.
- `exercisedata` (FK `exerciseKey`): one row per data `type`. The schema has
  `type`, `numberSamples`, `sequence`, and `data longblob`. For the baseline,
  four rows (`numberSamples` = 0 on all; the count field is unused here):

  | type | blob bytes | format | decrypted content |
  |---|---|---|---|
  | 0 | 1 334 | Java `byte[]` → AES-128-CFB (see below) | `<base:exerciseHeader>` XML |
  | 1 | 75 983 | Java `byte[]` → AES-128-CFB | `<flowDesign>` XML (SMART-2 feedback graph) |
  | 5 | 1 245 | Java `byte[]` → AES-128-CFB | `<flowControlMessage>` XML |
  | 9 | 53 444 | Java `byte[]` → **length-prefixed zlib XML (cleartext)** | `exerciseBAGSData` series + FFT |

- No raw 250/500 Hz sample stream is persisted. The ~132 KB of result blobs is
  all that survives a 4-minute exercise; the live 240 000-byte/30 s DataCapture
  buffers (`b2-protocol.md`) are processed down to these.

## Observed: blob container is a Java-serialized `byte[]`

Every `data`/`properties` blob is a Java serialized `byte[]`: a fixed 23-byte
wrapper `AC ED 00 05 75 72 00 02 5B 42 AC F3 17 F8 06 08 54 E0 02 00 00 78 70`
(`STREAM_MAGIC`, `TC_ARRAY`, `TC_CLASSDESC` `"[B"`, the standard `[B`
serialVersionUID `AC F3 17 F8 06 08 54 E0`, `TC_ENDBLOCKDATA`, `TC_NULL`) then a
4-byte big-endian array length then the raw bytes. **The payload begins at
offset 27** (23-byte wrapper + 4-byte length). Confirmed against all four
baseline blobs: the length field equals `total − 27` exactly (type 0 = 1307,
type 1 = 75 956, type 5 = 1218, type 9 = 53 417).

## Observed: type 9 is cleartext — length-prefixed zlib XML

The type-9 payload is `<4-byte big-endian uncompressed length><zlib stream>`
(`78 9C`). It inflates to the declared length exactly (baseline: 648 618 bytes)
and is the exercise result series: root `exerciseBAGSData`, with
`base:exerciseDataChannel` / `base:exerciseDataSeries` /
`base:dataItem`(`base:xValue`,`base:yValue`) lists and `base:fftDataList`. This
is the computed per-channel feedback graph and FFT output — the parity baseline
for the SMART-2 feedback layer — and it needs no key.

## Observed: types 0, 1, 5 are AES-128-CFB with a fixed hardcoded key and IV

The type-0/1/5 payloads (from offset 27) are **AES-128 in CFB mode (full-block
CFB-128)** under a single key and IV compiled into the binary. Decrypting all
three baseline blobs yields well-formed UTF-8 XML with correct closing tags and
no trailing corruption (type 0 `<base:exerciseHeader>`, type 1 `<flowDesign>`,
type 5 `<flowControlMessage>`).

- Routine: `com::bst::optimizer::model::util::Crypt`, built on **libgcrypt
  1.2.4** (statically linked). The handle-setup function at VA `0x4edf20` (file
  offset `0xecf20`) calls `gcry_cipher_open(&h, algo=7, mode=2, flags=0)` at VA
  `0x4ee07d` — libgcrypt constants `GCRY_CIPHER_AES`=7 (128-bit) and
  `GCRY_CIPHER_MODE_CFB`=2 — then sets key and IV via the libgcrypt ctl entry
  (`gcry_cipher_ctl(h, GCRYCTL_SET_KEY=1, …, 16)` at VA `0x4ee0a0` and
  `GCRYCTL_SET_IV=2` at VA `0x4ee0bf`). `Crypt::encrypt` fetches the handle from
  `0x4edf20` and calls `gcry_cipher_encrypt` at VA `0x4ee2b2`. The OpenSSL
  `EVP_rc4`/`RC4_set_key` path and the libgcrypt ARCFOUR self-test are unrelated
  dead code; RC4 is not used.
- Key and IV are both 16-byte constants assembled byte-by-byte on the stack
  inside `0x4edf20` (the `mov BYTE PTR [esp+…], imm8` runs at VA `0x4edf63`
  through `0x4ee05f`; one byte comes from `al`=`0x7d` set at `0x4edf5a`):
  - **Key** (hex): `1b 6f 9d 8f 3b 3d 59 ad 7d 6c 08 c0 de 74 64 f9`
  - **IV**  (hex): `7d 92 d9 7d 6a ae 25 df b2 71 c4 b3 a3 fb 14 57`

  Decrypt a stored blob by stripping the 27-byte Java `byte[]` wrapper and
  running AES-128-CFB:

  ```bash
  K=1b6f9d8f3b3d59ad7d6c08c0de7464f9; IV=7d92d97d6aae25dfb271c4b3a3fb1457
  tail -c +28 ed_451.bin | openssl enc -d -aes-128-cfb -K $K -iv $IV -nopad
  ```
- There is **no per-record IV/nonce**: the fixed key+IV is why the type-0/1/5
  ciphertexts share an identical 57-byte prefix (`2DD34747B6535BE9…`) — the three
  record XMLs begin with the same `<?xml version="1.0" encoding="UTF-8"
  standalone="no" ?>\n` declaration, and CFB with a fixed IV produces identical
  ciphertext wherever the plaintext agrees. (The payload lengths are not
  block-aligned, as expected for CFB, a streaming mode.)

Because type 9 (the feedback series) is cleartext and types 0/1/5 now decrypt,
every stored blob is readable. Type 1 (`<flowDesign>`) is the SMART-2 feedback
graph definition and is a parity input for the clone's feedback layer alongside
the type-9 output series.

## Observed: a session with no sensors fitted runs and saves normally

On 2026-11-04 a SMART-2 session was run with the B-2 connected and nothing in the
sensor bar (exercise 126), then compared with a worn session the same day (exercise
125). Source: the guest datadir copied out after the app exited, then
read with a throwaway `mysql:5.7` container, blobs decoded as above.

- Before Play, the Optimize tab's sensor status read green "Ready". The app's
  lead-off check did not flag the missing sensors.
- The session ran the full 240 s and saved the same four `exercisedata` rows.
  `properties` reports `deviceErrorCount=0` and `deviceRestartCount=0`.
- No stored row records lead-off or sensor-contact state. The only stored trace of
  missing sensors is in the signal itself: every band, LMH, line-noise, and FFT series in
  type 9 is an order of magnitude lower than in the worn session and nearly flat.
- The type-5 Evaluator log differs. In the worn session it logged `ControlLog` 111 at 01:00, then a
  `ControlChange` at 02:00 (control value 111 → 82, then held). With no sensors it logged only
  `ControlLog` entries with the value held at 111. The evaluator codes are mapped in
  `audio-feedback.md`.
- Launched with `C:\bsttest\bin` as its working directory, the app ran this session
  without writing to `braintellect.log`. Inferred, not verified: it resolves
  `C:\bsttest\configuration\log4j.properties` against the working directory, so launch
  it from `C:\bsttest`.

## Observed: the clone's stored series do not reproduce the original's (#56)

On 2026-11-07 the original's record for the #48 replay (exercise 138: the run-8 EEG stream plus 61
frame-drop insertions, no wearer) was decoded as above and compared with the clone's
`live.rs` series. Decoded XML is kept under `tmp/s56/`, never committed.

Field map (clone -> original type 9):

| Clone field | Original |
|---|---|
| `Channel<n>` | `exerciseDataChannel channelName="Channel1..4"` |
| series `Band 10`, `Band 80` | `dataType="00-90"` series `Band-10`, `Band-80` |
| point `x` (ms) | `xValue`, every 5000 ms from 5000 |
| point `y` (µV) | `yValue`, 1e-6 volt units |
| `duration_s` | `properties` / header duration, 240 s |

- Shape differs: the original has 49 points per series (5000, 10000, ..., 240000, then a closing point at 240008, one block after the stop); the clone stores 48 and no closing point.
- Values differ: the clone stores an instantaneous per-sample band value that moves by tens of percent between blocks; the original's points are steady (Channel4 Band-80 holds 2.64..2.74 µV) and its first point is a startup transient (2.94, 2.25, 2.90, 2.85 for Band-80 Channel1..4 against 1.36, 1.25, 2.43, 2.73 at 10 s).

### Recovered mechanism (offline fit, not exact)

The type-9 `00-90` series come from the BAGSD flow design "Session Assess 3.4-v3.3" (`flowDesign` blob): `Source` Out1..4 feed `Band-NN` BandPassFilter elements (Band-10 1..3 Hz, Band-80 23..36 Hz; Butterworth, `infinite`, order 1) to OutAmplitude, then an AggregatorFilter ("discrete", averageEpoch 5 s, 2500 samples), then x1e6.

Reproduced offline against b138 from the #48 replay inputs (`tmp/s48/replay/eeg.txt`, 4 x 24-bit counts per sample, volts = counts / `ChannelCalibration::new(12.0, 10.0).divisor(F12)`; fit program kept at `tmp/s56/fit56.rs`):

- Source chain in volts: 25 Hz notch, 150 Hz low-pass, 1.5 Hz high-pass, warm from sample 0 with the 45 Hz low-pass switched in at sample 25644. Without the notch the Band-10 error is ten times larger and Band-80 far worse.
- Band-pass: standard bilinear order-1 Butterworth band-pass, tan prewarp, unit gain at the digital centre `2*atan(sqrt(tan(w1/2)*tan(w2/2)))`, applied in transposed direct form II (decompiled design: kind 2 at 0x653220, prototype 0x652ac0).
- OutAmplitude: sliding max of |y| over W samples (Band-10 W = 83, Band-80 W = 6, the clone's `max(rate/(2*high), 4)` rule), then the 16-tap mean with gain 2/16 (the clone's FIR `BandPass` tail).
- Aggregate: plain mean of 2500 OutAmplitude samples times 1e6; window k covers samples s0 + 2500k .. s0 + 2500(k+1), s0 about 25639..25642 in the b138 run (consumed sample 107504 in the exercise 146 run, see below).

Fit quality on points 1..48: mean absolute error 1.2e-3..3e-3 µV (relative about 6e-4..1e-3), maximum 3e-3..0.3 µV on the transient first point; the best start sample differs per channel by up to three samples. A first-order highpass-times-lowpass cascade instead of the bilinear band-pass is ruled out (error 0.7..1.2 µV).

### Capture result (#56, exercise 146)

A second replay (run 8, no wearer, stored as exercise 146 on 2026-11-07) was captured with hooks on the consumed volts of all four channels (tag 12, 262,516 samples per channel since the stream started, none missing) and on the aggregator emit 0x649580. Decoded records are kept under `tmp/s56/` (`vol146.txt`, `b146/`, `fit146.rs`), never committed.

- The exercise window starts at consumed sample 107504 (0-based) for every one of the eight stored series, found by scanning; the 25644 of the b138 run was that run's own start. The Source chain runs warm from the first streamed sample and the 45 Hz low-pass is switched in near the exercise start (the exact switch sample is not constrained by the fit).
- The aggregator emits 49 times per series (counts 2500/2500, dt 2 ms, 64 series), all in a burst at the end of the exercise; the point x values are 5000 .. 240000 ms plus the closing 240016 ms point.
- With the real consumed volts and that start, all 46 steady points of every Band-10 and Band-80 series follow the model with a constant relative deficit per channel, uniform across windows and nearly equal for both bands: model above stored by 6.2e-4, 7.5e-4, 3.6e-4, 3.5e-4 (Band-10) and 6.8e-4, 7.9e-4, 4.2e-4, 4.0e-4 (Band-80) for Channel1..4; after a linear map the residual rms is 3e-5..9e-5 µV. The first point (startup transient) differs by 7e-3 (Band-10) to 0.13 (Band-80, Channel2) relative.
- So the volts input and the band-pass, OutAmplitude and aggregation structure are right, and the remaining gap (up to 8e-4 relative) is a per-channel gain not explained by the consumed volts (bit exact per #55), the band-pass, or an f32 running sum (a truncating f32 sum would give a channel-independent 1e-4 loss). The per-channel values do not track the channels' mean level or rms.

### Blocker

The fit does not reach the harness record tolerance (`RECORD_EPSILON` = 1e-6), so no stored-series value assertion exists and none is added. Ruled out: HP x LP cascade instead of the bilinear band-pass; f32-quantised band-pass coefficients; a dropped first sample per window; a per-window start offset; the volts conversion; a window start other than 107504; a channel-independent f32 sum bias. Not recovered:

- The per-channel gain above (3.5e-4..7.9e-4) and the startup transient of the first point (cold or warm state of the 45 Hz low-pass and the filters at the exercise start).
- The aggregator's reset between windows and the f32 values entering the mean: the hooks on the series push (0x6497b0, 0x649820) and reset (0x649840) never fired, so the real accumulation path is a different function, and the series pointer read at `this+0x1e4` did not hold the accumulator (the sum slot read garbage). The emit hook did fire with `this+0x1d0` = `this+0x1d8` = 2500.
- The Band-10 ranks for Channels 1..4 (A..D) are unlogged stand-ins.

Next capture: find the function that adds samples into the aggregator (callers of 0x649580's accumulator vtable 0x18bf978 slots, or hook the OutAmplitude output at 0x650a20 / 0x652020) and log its float input and the Source output per channel, so the first stage where the per-channel gain appears can be named.

Aggregator static read (0x649cc0 with the AbsoluteDataSeries vtable 0x18bf978): the running sum is a float32 held in a double slot, the mean is `(float)sum / (float)count` (0x649850), the point x is `(float)count * (float)dt` (0x649580), and a type-0xc (stop) message emits the partial window once, which is the closing point.

Code located: BAGSD run 0x44b700; GeneralDataCollector 0x5c1300, 0x5c1c80, 0x5c2070; IIR design 0x66dc00, 0x6536a0, 0x653220, 0x652e90, 0x652ac0, 0x652020, 0x650a20; aggregator message handler 0x649cc0, emit 0x649580, accumulator 0x6497b0/0x649820/0x649850/0x649840.

Kept out of the clone: the other nine `00-90` bands per channel (Band-00, 20..70, 90, VHF), the `LMH` (Low/Mid/High) and `LineNoise` (50Hz/60Hz) series, the 256-point `FFT-Normal-Order` series, and the `flowDesign` (type 1) and `flowControlMessage` (type 5) blobs. Follow-up: name the stage that introduces the per-channel gain, build the IIR band-pass and 5 s aggregator in `src/feedback/`, then add an exact harness assertion with a `parity_selfcheck` red case.
