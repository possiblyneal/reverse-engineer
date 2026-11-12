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

## Stored series (#56)

The type-9 series are not the SMART-2 graph's band amplitudes. They come from a second
flow design, BAGSD (`BAGSD-4-chnl-wDiscreteFFT-v5.1.0`, `flowDesign` blob; run `0x44b700`), that
the original runs over the session's stored filtered data after the exercise. The clone stores
all 17 series per channel (`feedback::StoredSeries`) and reproduces every one of exercise 146
within `RECORD_EPSILON`, asserted by the ignored `tests/replay_s56.rs` against the decoded
`ed_9.xml`; `tests/replay_s59.rs` does the same for exercise 147. Decoded records and the capture
stay under `tmp/s56/`, never committed.

Largest difference per series over the four channels, in µV (49 points per filter series):

| Series | #146 | #147 |
|---|---|---|
| Band-00 | 7.75e-7 | 2.55e-7 |
| Band-10..Band-90 | 4.47e-7 (Band-70) | 4.30e-7 (Band-20) |
| Band-VHF | 1.57e-7 | 3.24e-7 |
| High / Mid / Low | 4.67e-7 (Low) | 4.65e-7 (High) |
| 50Hz / 60Hz | 4.93e-8 | 8.96e-8 |
| FFT-Normal-Order | 9.19e-8 | 9.55e-8 |

Field map (clone -> original type 9):

| Clone field | Original |
|---|---|
| `Channel<n>` | `exerciseDataChannel channelName="Channel1..4"` |
| series `Band-00`, `Band-10`..`Band-90`, `Band-VHF` | `dataType="00-90"`, same names |
| series `High 1&2`, `Mid 1&2`, `Low 1&2` (`3&4` on Channel3..4) | `dataType="LMH"`, same names |
| series `50Hz`, `60Hz` | `dataType="LineNoise"`; Channel4's `60hz` is stored as `60Hz` |
| series `FFT-Normal-Order` | `dataType="FFT"` (see "Stored FFT (#59)") |
| point `x` (ms) | `xValue`: every 5000 ms, then a closing point at the stop |
| point `y` (µV) | `yValue`, 1e-6 volt units |
| `duration_s` | `properties` / header duration, 240 s |
| `capture` | none: the clone's raw RX capture, relative to the data directory |
| `started_at` | none: the clone's session start, Unix seconds |
| `test` | none: the clone's Mark as test flag; test sessions are hidden from Past sessions |

The `dataType` and side attributes are not stored: the series name gives the type, the channel
the side.

### Observed (exercise 146, 2026-11-07)

Run 8's stream replayed through `braintellect-linux emulate` (`replay-rig.md`), with hooks on the
consumed volts of all four channels (tag 12/13, 262,516 samples per channel from the stream
start), the aggregator emit `0x649580`, and the first 400 inputs of every band-pass object at the
BandPassFilter process `0x66ea7c` (tag 9).

- The 14 SMART-2 band-pass objects get their inputs during the exercise. The 64 BAGSD band-pass
  objects (16 per channel) get theirs 240 s later, all fed the same stream from the same first
  sample, the graph start (consumed sample 107504).
- Every BAGSD input is the SMART-2 Source output `x` as stored: `f32(trunc(x * d) / d)`, with
  `d` the calibration divisor (counts per volt, `b2-protocol.md`) and truncation toward zero.
  1600/1600 inputs bit exact (4 channels x 400). Rounding to nearest matches 810/1600,
  `f32(f32(n) / f32(d))` 961/1600.
- The SMART-2 inputs fix the 45 Hz low-pass switch per channel, 400/400 each: consumed sample
  107508 for Channel1, 107507 for Channel2..4. The Source chain runs warm from the first
  streamed sample with the interference filter off before that.
- The aggregator emits 49 times per series, all at the end of the exercise; x is 5000 ..
  240000 ms, then 240016 ms.

### Recovered model (matches the stored values within `RECORD_EPSILON`)

Per channel and band, from the graph start:

1. The stored Source output above (whole ADC counts, read back as `f32` volts).
2. An order-1 Butterworth filter (`infinite`), one section in transposed direct form II, `f64`.
   Band-pass (Band-10..90 1-3, 3-5.5, 5.5-7.5, 7.5-10, 10-12, 12-15, 15-23, 23-36, 36-48 Hz;
   LMH High 15.8-34, Mid 6.8-15.8, Low 1.5-6.8 Hz; 50Hz 49-51, Channel2 49-51.1; 60Hz 59-61):
   the bilinear transform with both edges prewarped by `tan` (design kind 2 at `0x653220`,
   prototype `0x652ac0`). Band-00 is a low-pass at 1 Hz and Band-VHF a high-pass at 62 Hz, the
   same prewarped bilinear order-1 design (inferred from the band-pass kind; the replays hold
   it).
3. `OutAmplitude`: sliding maximum of `|y|` over `W = max(rate / (2 * edge), 4)` samples, then
   the 16-tap mean times 2/16, in `f64`. The edge is the upper one of a band-pass, the low-pass's
   1 Hz (`W` 250; 125 fails) and the high-pass's 62 Hz (`W` 4, which the data cannot tell from the
   block minimum; 10 fails).
4. The `discrete` Aggregator (averageEpoch 5 s): every 2500 samples, the `f64` mean of the last
   2500 amplitudes times 1e6, at x = samples x 2 ms. At the stop it emits once more: the mean of
   the last 2500 amplitudes (relative samples 117508..120008 for the 240016 ms point), not of
   the 8 samples since the last point.

Variants that fit worse (offline, over the same data): an `f32` running sum in the aggregator,
or the band-pass output rounded to `f32`, each exceed 1e-6 µV on many points; the plain
unquantised Source output leaves the per-channel gain deficit of 3.5e-4..7.9e-4 relative that
blocked the previous attempt, which the truncation explains (it removes on average half a count
from every sample's magnitude).

### Inferred, not observed

- The `discrete` series keeps the last 2500 values (a ring) rather than a running sum, from the
  closing point's fit. The static read of `0x649cc0` with the AbsoluteDataSeries vtable
  `0x18bf978` (a `float` sum in a `double` slot, mean `(float)sum / (float)count` at `0x649850`)
  is another series class: its push and reset hooks (`0x6497b0`, `0x649820`, `0x649840`) never
  fired, and its `float` sum fits worse.
- A stream that stops exactly on a 5 s boundary is taken to add no closing point; no such stop
  was captured.
- The per-channel switch sample is the app's thread timing (Channel1 one sample later here; run
  8 switched at the graph start + 1). A live clone run switches at the graph start
  (`Mode::Fixed`), and its stored series start at the graph's first sample, after the Source warm-up (#66).

## Stored FFT (#59)

Each channel's `FFT` data type holds one `FFT-Normal-Order` series: a single `fftDataList`
(`xValue` the stop in ms, 240040 for exercise 147) of 256 `yValue`s in microvolts. It is the mean
of the discrete `DSPFFT` outputs over 500-sample blocks of the stored Source output, the final
partial block zero padded and included (`bagsd-design.md`). The clone stores it as the last
series of each channel in `StoredSeries::series()`: `FFT-Normal-Order`, 256 points, `x` the bin
index (the original keeps no per-bin x; bin `k` is `k * 500 / 512` Hz) and `y` microvolts. A live
clone session stores it with the other series; `tests/replay_s59.rs` holds the whole report (17
series, four channels) to exercise 147's record within `RECORD_EPSILON`.

Kept out of the clone: the `flowDesign` (type 1) and `flowControlMessage` (type 5) blobs.

Code located: BAGSD run 0x44b700; GeneralDataCollector 0x5c1300, 0x5c1c80, 0x5c2070; IIR design
0x66dc00, 0x6536a0, 0x653220, 0x652e90, 0x652ac0, 0x652020, 0x650a20; BandPassFilter process
0x66e9f0 (input at 0x66ea7c); aggregator message handler 0x649cc0, emit 0x649580.
