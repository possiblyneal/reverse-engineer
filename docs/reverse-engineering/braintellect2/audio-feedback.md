# BRAINtellect2 Audio Feedback

## Scope

This record summarizes authorized analysis of the BRAINtellect2 executable and a returned tablet retry archive. It excludes personal, session, and raw EEG data. Raw archive contents, the SQL recovery export, and decrypted designs remain sensitive materials under `tmp/` and are not repository documentation.

## Evidence

- Executable: `tmp/bst-export/install/BRAINtellect2/bin/braintellect.exe`
- Returned retry archive SHA-256: `014b9570a8dadbd5373c318db15f2c5f3e633a63918b9c8165d62a69f16365df`
- Database recovery: MySQL 5.5.62 opened the copied InnoDB datadir in forced-recovery mode, then produced a schema export.
- Active design reference: all recovered exercises referenced `BT-SMART-2-v5.1.2`.
- Flow-design values were stored as Java-serialized byte arrays encrypted with the application's static AES-128 CFB implementation. Decrypting the byte-array payloads produced XML that parses as `http://optimizer.bst.com/model/flowDesign` flow designs.

## Recovered Design Inventory

| Design | XML elements | Notable role |
|---|---:|---|
| `BT-LOD-v5.1.11` | 207 | lead-off detection |
| `BT-SMART-2-v5.1.2` | 1,137 | active exercise design and audio feedback |
| `B-2-Circle-Balance-fromBAGSD-v5.1.1` | 1,162 | post-session Balance Meter report over stored LMH series (`BAGSD.report`; "Circle-Balance design") |
| `BAGSD-4-chnl-wDiscreteFFT-v5.1.0` | 3,807 | four-channel discrete-FFT design |

## SMART-2 EEG-to-Audio Path

The decoded SMART-2 XML wires four channel peak-frequency outputs to an expression transform, then sends the expression result to the sound element's note input:

```text
PeakFreq Ch1 ─┐
PeakFreq Ch2 ─┼─> ExprNoteSelection ─> SoundElement.InNote
PeakFreq Ch3 ─┤
PeakFreq Ch4 ─┘
Evaluator ────┘

Threshold 20 ─> ThresholdCounter ─> SoundElement.InTrigger
```

`ExprNoteSelection` selects a single channel or averages a pair according to the evaluator code:

```text
11 or 81    -> average channels 1 and 2
101 or 181 -> average channels 3 and 4
12 or 82    -> channel 1
13 or 83    -> channel 2
102 or 182 -> channel 3
103 or 183 -> channel 4
otherwise   -> average channels 3 and 4
```

The output is an input to the tone controller, rather than a direct acoustic rendering of raw EEG.

## SMART-2 Sound Configuration

| Setting | Decoded value |
|---|---|
| Output | Wave, both channels |
| Wave instrument | `1002` |
| Default note | MIDI `60` (C) |
| Note automatic range | enabled |
| Configured note input range | `0.0` to `60.0` |
| Configured note output range | MIDI `30` to `66` |
| Scale | `None` |
| Play interval | 400 ms |
| Maximum sound length | 500 ms |
| Forced note | enabled |

MIDI note number converts to acoustic frequency using `440 × 2^((note − 69) / 12)`. Thus the configured MIDI endpoints correspond to approximately 46 Hz (note 30) and 370 Hz (note 66). The executable's `noteAutoRange` implementation remains necessary to state the final dynamic mapping precisely.

## SMART-2 Trigger Configuration

The tone trigger comes from `Threshold 20`, followed by `ThresholdCounter`:

| Component | Decoded configuration |
|---|---|
| Threshold direction | `decrease` |
| Threshold mode | `auto` |
| Threshold type | `single` |
| Lower/upper auto settings | `0.65` / `0.75` |
| Threshold epoch | 5,000 ms |
| Average epoch | 125 ms |
| Counter type | `timeHigh` |
| Counter trigger epoch | 500 ms |

The threshold's pass/fail output feeds the counter, and the counter's output triggers the sound. The design also sets `threshold = 10.0`, `duration = 0`, `reverseRatioOnDecrease = false`, and `triggerModulus = 1`.

### Threshold (observed)

Static disassembly of `braintellect.exe` (SHA-256 `af7ff5430ee10c2d548450819f54a3f939af474953a42b959fd99739519c2560`) with Ghidra headless. The property map is at `0x13e7eda`–`0x13e80f8` and setup is at `0x6300e0`.

- **Smoothing:** `averageEpoch` (ms) sets a boxcar mean over the last `rate·averageEpoch/1000` samples (`0x6543f0` → `0x654fe0`). During warm-up it divides by the number of samples seen. The smoothed value `v` is `OutSignal`.
- **Window:** the last `rate·thresholdEpoch/1000` values of `v`, kept as a ring and a sorted multiset (`0x62e850`); the multiset drops every entry equal to the one leaving the ring, so `n` can fall below the ring size (see "Run 8 whole-exercise capture"). `thresholdEpoch` must be at least 50 ms.
- **Auto threshold** (`0x62e6e0`, after every input block in auto mode, gated at `0x62fd2f`). With `n` values in the window:
  - single, goal `increase`: `k = round(n·(1 − lower))`;
  - single, goal `decrease`: `k = round(n·lower)`;
  - `k = n` is clamped to `n − 1`, and the threshold is the `k`-th order statistic of the window (`0x62e3c0`);
  - double: low = order statistic at `round(n·(1 − lower))`, high = order statistic at `round(n·upper)`;
  - an empty window gives `0.0`.
- **Pass test** (`0x62ed70`): single `increase` passes when `v > thr`, single `decrease` when `v < thr` (strict), and double when `low < v < high`.
- **Duration** (`0x62edd4`–`0x62ee35`): `0` means each sample passes or fails on its own. Otherwise passing time accumulates, the output stays `0` until it reaches `duration`, and any fail resets it.
- **Outputs** (loop `0x62f920`): `OutPassFail`, `OutPassPercent` (passes ÷ filled count), `OutRatio` (`v/thr`; `(thr − v)/thr` when `reverseRatioOnDecrease` is set and the goal is `decrease`), `OutSignal`, and `OutThresholdLow`/`OutThresholdHigh`.
- **Manual mode** copies `threshold`, `lowerThreshold`, and `upperThreshold` divided by `1e6` (`0x6300e0`).
- **Resets:** a type-2 data message resets the series, and type-3 or type-4 control messages reset the window (`0x62f8f0` → `0x62f460`).

So in SMART-2 (single, `decrease`, auto), the input is smoothed over 125 ms, the threshold is the 0.65 quantile of that smoothed signal over the last 5,000 ms, and a sample passes when it falls below it. `upperThreshold` (0.75) and `threshold` (10.0) have no effect in this configuration.

### Counter (observed)

The property map is at `0x13f2afa` and the setter is at `0x6433d0`. `type` accepts `rising`, `falling`, `timeHigh`, and `timeLow`. `triggerEpoch` sets `+0x1a0` and `triggerModulus` sets `+0x1a4`.

- `timeHigh` runs handler `0x642ed0` with predicate `x > 0` (`0x642610`). `timeLow` uses `x == 0` (`0x642630`).
- The handler reads the timer (`0xf30a30`) once per input block, before its sample loop. For each sample where the predicate holds, it increments a count. If that block's elapsed time is at least `triggerEpoch`, it also zeroes the count and sets the trigger, which is not cleared again within the block. The timer restarts after any block that fired. The constructor (`0x643240`) starts the timer.
- `triggerModulus` is read only by the `rising`/`falling` handler (`0x642cc0`, trigger when `count % modulus == 0`).

### Inferred

- About 65% of the 5,000 ms window passes by construction, so the auto threshold tracks the signal: about two thirds of recent samples pass, whatever the absolute level.
- `timeHigh` fires on the first passing sample in the first block that begins at least `triggerEpoch` (500 ms) after the last fire or after start. The trigger output then stays 1 for the rest of that block. The input does not have to stay high for those 500 ms. Fires repeat at most once per epoch while passes continue, subject to block size and to how `SoundElement` consumes the trigger.
- `triggerEpoch` is read as milliseconds against the timer. The divisor at `0x1895360` and the constructor defaults were not reconciled.
- The static `round(n·lower)` reading of the quantile index is superseded by the run-3 capture below (`trunc(n·lower)`, zero-based).

### Logging (observed)

No runtime log string exists for threshold pass/fail, threshold updates, or counter fires. Only the validation errors exist (for example `"invalid trigger epoch"`, `0x18becfc`). A session's `braintellect.log` therefore cannot confirm this behavior. Only the element outputs in a recorded feedback series can.

### Run 3 decision capture (observed)

One SMART-2 session (guest profile, no wearer, sensors off) captured with one Frida script hooking the serial reads and the threshold and counter functions on a shared clock. Raw captures stay in the main checkout's `tmp/s26/run3/` (not committed); the committed fixtures are `tests/fixtures/feedback_{blocks,decisions,eeg}.tsv`, scrubbed and reproduced by `scripts/make-fixtures.py`.

- **Quantile index:** the threshold is the sorted window's element at `trunc(n·lower)`, zero-based, clamped to `n − 1`, with `lower` the single-precision `0.65`. This replaces the static `round(n·lower)` reading.
- **Window capacity:** a fixed 2,500 values (500 Hz × 5,000 ms). The threshold is recomputed after every input block and applies to the next block's samples.
- **Pass flags:** the clone reproduces every recorded pass flag of the fixture's blocks (0 mismatches).
- **Counter clock:** the timer is read once per block, as the static reading said. The clone reproduces every recorded fire given the observed per-block timer reading and a timer start at least 250 ms before the first block (the baseline's first fire is 250 ms after the first block). The start offset is recovered in "Counter timer start" (ticket 54).
- **Decision stream:** `Trigger` and `Note` decisions are in `feedback_decisions.tsv`. The clone reproduces the `Trigger` decisions only.

Gaps (not reproducible from this evidence, so not implemented):

- The capture covers about 163 of 240 s: the guest's D: volume filled and the hook logs stopped. The PeakFrequency hook recorded nothing, cause unknown, so no per-channel peak outputs exist.
- Upstream stages (band-pass, Aggregator, `ExprTriggerAll`), the `averageEpoch` smoothing, `ExprNoteSelection` channel order, and the `SoundElement` note mapping are unresolved. The channel-order mapping is not adopted: the evidence does not single one out. End-to-end EEG-to-decision parity is therefore not claimed; `feedback_eeg.tsv` holds the aligned EEG window for when those stages are recovered.

## SMART-2 Evaluator

The `Evaluator` element (`style = coherenceChecker`, `valuesToCheck = 2`, `coherenceThreshold = 0.80`, `balanceThreshold = 0.15`, `defaultControlValue = 111`, `averageEpoch = 60`, `sendControlMessageEachEpoch = true`) produces the control value that `ExprNoteSelection` and `ExprTriggerAll` decode.

Inputs from the design:

- **Balance** is `(B − A)/min(A, B)` over 60 s averages of band-passed amplitude, for the Ch1/Ch2 and Ch3/Ch4 pairs in Band 10 (1–3 Hz) and Band 80 (23–36 Hz).
- **Coherence** inputs exist only for Ch3/Ch4, in Band 10 and Band 80.

### Observed

The key table is at `0x13f1144`–`0x13f1253`, the setter at `0x63dc28`–`0x63dedb`, and the constructor defaults at `0x640907`–`0x640989`.

- **Epoch:** `averageEpoch` is in **seconds** here: epoch samples = `averageEpoch × rate`, with no `/1000` (`0x63f7ef`–`0x63f80f`). At each boundary, `process` (`0x640130`) pushes the latest input into each series, which holds the last `valuesToCheck` epochs (`0x63e540`), and runs `processLogic` (`0x63eb70`) once.
- **Decision**, recomputed from `defaultControlValue` every epoch:
  1. **Coherence:** the first Coherence series that is full and has every value with `|value| > coherenceThreshold` wins (`0x63e2c0`). The code comes from the Balance input on the same band with the largest `|latest|`. `balanceThreshold` is not applied on this path.
  2. **Balance:** otherwise, among Balance series that are full and have every value with `|value| > balanceThreshold`, the largest `|latest|` wins.
  3. Otherwise the default stands.
- **Code** = band base (`"Band 10"` → 11, `"Band 80"` → 81; `0x640a22`–`0x640a51`) + 100 for `"Channels 3n4"` (`0x63efa3`) + 1 if the latest balance is below 0, else 2 (`0x63f011`–`0x63f040`). That gives 12/13, 82/83, 112/113, or 182/183. Balance below 0 means channel A is larger, so the code ends in 2.
- **Control message** (`0x64033f`–`0x640420`): `"Old control value: …, new control value: …"` is `ControlChange` when the value changed and `ControlLog` when it did not. With `sendControlMessageEachEpoch`, every epoch logs one.

So **111 is `defaultControlValue`**, meaning no test passed. It is not an `ExprNoteSelection` code: it falls to "otherwise" and averages channels 3 and 4. **82** is Band 80, Ch1/Ch2 pair, channel 1 larger.

**Design mismatch:** for the Ch3/Ch4 pair the Evaluator produces 112/113 and 182/183. `ExprNoteSelection` lists 102/103 and 101 instead, so 112/113 also fall to "otherwise" (average of channels 3 and 4). `ExprTriggerAll` does decode 112/113. The clone keeps this mismatch in original mode; fixed mode corrects it ("Channels 3n4 note codes").

### Channels 3n4 note codes (ticket 64)

**Source.** The four designs in the 2026-11-07 guest datadir (`flowdesign` table, the `tmp/s59/datadir` copy read with a throwaway `mysql:5.7` container, blobs decoded as in `session-storage.md`; decrypted XML in the main checkout's `tmp/s64/`).

Observed:

- **The Evaluator never emits 101, 102 or 103.** Its codes are 111 (default) or a band base 11, 81, 111 or 181 plus 1 or 2 ("Observed" above).
- **`ExprTriggerAll` pairs the bands by +70 for both channel pairs.** It decodes 11/81, 12/82 and 13/83 for Ch1/Ch2, and 111/181, 112/182 and 113/183 for Ch3/Ch4, where 112 and 182 read channel 3 (E, G) and 113 and 183 read channel 4 (F, H). Both expressions take the same Evaluator output (`InSignalJ`, `InSignalE`).
- **`ExprNoteSelection` keeps that pairing for Ch1/Ch2 only.** It lists `(11 || 81)`, `(12 || 82)` and `(13 || 83)`, then `(101 || 181)`, `(102 || 182)` and `(103 || 183)`, with A..D = PeakFreq Ch1..Ch4. 181 − 101 = 80 breaks the +70 step its first three branches and every branch of `ExprTriggerAll` keep. 182 → C (Ch3) and 183 → D (Ch4) match `ExprTriggerAll`'s 182 → G and 183 → H.
- **No other design decodes these codes.** `B-2-Circle-Balance-fromBAGSD-v5.1.1` (eight balance `ExpressionTransform`s, `ln(1+abs((((1.08*B)-A)*10)/(A min (B*1.08))))*sign((1.08*B)-A)`, into `CircleGraphChart`s) and `BAGSD-4-chnl-wDiscreteFFT-v5.1.0` have no Evaluator and no code expression. `BT-LOD-v5.1.11` has none either.
- **The literature corpus gives no code table.** It names the aim (`literature/2013_Gerdes-Tegeler_HIRREM-allostatic-methodology_BrainBehav.md`: the tone follows the dominant frequency, toward hemispheric symmetry, with asymmetry measured in 23–36 Hz, the design's Band 80), but not which channel a code selects.

Inferred: 101, 102 and 103 are typos for 111, 112 and 113. By the design's own +70 pairing and `ExprTriggerAll`, the intended note selection is 112 → channel 3 and 113 → channel 4. 182/183 already decode as intended. 101 and 111 both reach the channel 3 and 4 average (111 through "otherwise"), so that typo changes nothing.

The clone's fixed mode selects channel 3 for 112 and channel 4 for 113. Original mode keeps the average. `tests/parity_feedback.rs` shows both. No recorded stream has reached 112/113 (run 8: 111, 12, 82; #53 and run 6: 111, 183; #33: 111, 82, 12), so the run-8 comparison in "Fixed mode against original mode" is unchanged.

### Observed sessions

The stored type-5 logs (2026-11-04) show:

- exercise 125 (worn): `ControlLog` 111 → 111 at 01:00, `ControlChange` 111 → 82 at 02:00, then 82 held;
- exercise 126 (no sensors): 111 held throughout.

### Inferred

- With `valuesToCheck = 2`, no series is full at the 01:00 epoch, so 01:00 always reports the default. A change can first appear at 02:00.
- With no sensors, floating inputs give near-equal band amplitudes and low coherence, so neither test passes and 111 holds.
- Which path produced 82 in exercise 125 is not determined; it needs the per-epoch input values.
- No dispatch on `style` or `evaluatorLogic` was found in `processLogic`. Whether another style changes the logic is unresolved.

### Replay capture of the Evaluator inputs (#53, observed unless marked)

**Method.** The run-8 rx stream was replayed into the original (`replay-rig.md`) with `hook12.js` (`frida-inject -f`) writing one binary log of f64 records `[tag, length, ...]`: tag 20 is the series push (`0x63d9c0`: `[ts, series, countBefore, vectorSize, last, scale]`, one per block per series), tag 21 the series key dump (the name is in words 39 to 86), tag 22 the Evaluator decision (`0x63eb70`: `[ts, evaluator, ret, currentCode]`), tag 24 the code setter (`0x63ea70`), and tag 11 the `ExpressionTransform` inputs. The log is `g-s53.bin` in the main checkout's `tmp/s53/r3/` (341 MB, 30004 blocks, wearer-free: the stream is the recorded no-wearer run). `scripts/replay-extract.py` reduces it to `amps.tsv` (A..H, 32 float32 words per block), `evaluator.tsv` (the six series inputs per block) and the existing replay files; `replay_s48` reads them with `S48_REPLAY_DIR`.

- **Bands A..D.** Tag 11's nine-record group is `recs[0:4]` = A..D, `recs[4:8]` = E..H, `recs[8]` = the control code, four samples each (a five-record group is the peaks, a two-record group the aggregates). The earlier "A..D are not logged" is withdrawn.
- **Series.** The Balance series are `Bal1n2B10`, `Bal1n2B80`, `Bal3n4B10`, `Bal3n4B80` and the Coherence series `Coh3n4B10`, `Coh3n4B80`.
- **Epoch timing.** An epoch is 30000 samples, 7500 blocks of 4. It ends at blocks 7499, 14999, 22499, 29999 (zero-based) and the code decided there applies to the block that completes the epoch (first block with the new code: 14999 in this capture and in run 8).
- **Balance.** Every logged Balance value of the capture (30001 blocks, four series) is reproduced bit for bit. The Aggregator's `float32` mean over the 30000-sample trailing window (an `f64` running sum cast to `float32`) feeds double arithmetic `(B·g − A)/min(A, B·g)`, cast to `float32` once. `g` is 1.0 for the Ch1/Ch2 series. For the Ch3/Ch4 series `g` = 1.08, applied to the Ch4 average (F, H): the design's own expression is `((1.08*B)-A)/(A min (B*1.08))` (`Expr Balance Ch3n4 Band 10/80`, observed).
- **Codes.** 111 for blocks 0 to 14998, then 183 to the end (111 at the first epoch end because `valuesToCheck` = 2). The Balance bases are 11, 81, 111, 181, plus 1 if the latest is below 0, else 2.
- **Coherence.** Every logged Coherence value (30001 blocks, both series) is reproduced bit for bit by the algorithm in "Coherence" below. The values at the epoch ends stay below 0.80, so the Coherence path decided no epoch of this capture; the value is 1.0 before the first estimate (693 blocks above 0.80, from block 0).
- **Clone.** `feedback::Evaluator` decides the code from the clone's own band-pass amplitudes and Coherence elements, both paths: it reproduces every per-block code of the capture (`replay_s48`, which asserts all six series bit for bit), and run 8's 111, then 12 from block 14999, then 82 from block 22499, in `parity_smart2_exercise`. Run 8 carries no A..D ranks, so its stand-in ranks are used; a different order moves an amplitude by one `float32` unit and cannot move a Balance across 0.15 there.

### Coherence (static, matched bit for bit against the #53 capture)

The design's two `Coherence` elements (Ch3/Ch4, Band 10 and Band 80, `processingStyle` `Relaxed`) each feed an `AggregatorFilter` whose 60 s mean is the Evaluator's Coherence input, as the band-pass amplitudes feed the Balance series. Observed by the static reading below and confirmed by the bit-exact match of every logged value; nothing here is fitted.

- **Element** (process `0x646990`): each input goes through the element's own FIR band-pass, built as the band-pass elements' (same band and order). The filtered samples are held in `float32` and gathered 16 blocks (64 samples) at a time (`+0x1ac`, 16 under `Relaxed`). A block that completes a packet outputs the new estimate for each of its samples; any other block repeats the last one, and 1.0 before the first.
- **`DSPCoherence`** (constructor `0x644ac0`, process `0x645aa0`): Welch's magnitude-squared coherence over a history of 4 s (constant `4.0` at `0x1895398` times the rate: 2000 samples, newest first). Segments of N = rate = 500 samples, hop N/2 = 250 (half overlap under `Relaxed`), (2000 − 250)/250 = 7 segments. Each segment is multiplied by a symmetric 500-point Hamming window (`0.54 − 0.46 cos`, constants `0x18cb358`/`0x18cb360`; IPP generic kernel `0x67535c`, the cosine by an angle-addition recurrence from both ends in `f64`, each value rounded to `float32`) and zero padded to the FFT length 512.
- **Bins:** bins per Hz = `float32(512/rate)`, first bin = `trunc(bins/Hz · low)`, count = `ceil(float32(bins/Hz · high)) − first + 1`: bins 1..4 for Band 10 (1–3 Hz) and 23..37 for Band 80 (23–36 Hz).
- **Estimate:** per bin, `float32` sums over the segments of `|X|²`, `|Y|²` and `X·conj(Y)`; each sum divided by the segment count in `f64` and rounded to `float32`; `power = float32(Syy·Sxx)`; when it is non-zero, the bin's coherence is `float32((re² + im²)/power)` in `f64`, clamped to 1.0. The mean over the bins (an `f64` division of the `float32` sum) enters an 8-value mean that starts full of 1.0 (`0x6447b0`, built as `(8, 1.0, 0)`); its output is the estimate.
- **FFT:** `ippsFFTInitAlloc_R_32f(order 9, flag 8)`, forward `0x66ff20`, dispatched through the table at `0x1eed1e8` (six CPU variants; variant 1, kernel wrapper `0x67831c`, was traced, and variants 1 to 5 gave identical outputs under Unicorn). The 512 reals are read as 256 complex `z[n] = (x[2n], x[2n+1])`. The complex FFT (`0x6c7de0`, power-of-4 path `0x6c82d0`) is radix-4 decimation in time: a twiddle-free 4-point DFT of `z[a], z[a+64], z[a+128], z[a+192]` with `a` the 6-bit bit reversal of the output group, then three stages with quarters 4, 16, 64 computing `dft4(w[p], w[p+2Q]·W^i, w[p+Q]·W^2i, w[p+3Q]·W^3i)` with twiddles `float32(cos)`, `float32(−sin)` of the stage length (exact at the quarter turns). The 4-point DFT sums `a = x2+x0, b = x0−x2, s = x3+x1, d = x1−x3` and outputs `s+a`, `b − i·d`, `a−s`, `b + i·d`; the complex product is `(xr·wr − xi·wi, xi·wr + xr·wi)`. The split pass (`0x6c89c0`) forms bin 0 = `re0+im0`, Nyquist = `re0−im0`, and for k = 1..127, with `T0 = float32(0.5 cos(2πk/512))` and `T1 = float32(0.5 − 0.5·float32(sin))`: `dr = R−Rm`, `si = Im_m + I`, `p = si·T0 + dr·T1`, `q = T1·si − T0·dr`, bin k = `(Rm+p, q−Im_m)`, bin 256−k = `(R−p, q−I)`; then the imaginary part of bin 128 is negated. The kernel was run under Unicorn on a synthetic vector; the clone's port (`feedback/ipp_fft.rs`) matches its 514 output words bit for bit (`tests/fixtures/ipp_real_fft_512.txt`). A textbook FFT differs in the last bits and moves Coherence values by one `float32` unit.

### Live replay of the #33 capture (tickets #60 and #66, observed unless marked)

The #33 capture was read back through `live::run` over an in-memory transport (`tests/replay_live.rs`, ignored): its handshake, then every stream frame the original consumed, without the 124 frames its host skipped, under the original's logged counter clock reads. The original's graph started at consumed sample 15616 (located by `replay_s48`).

- **Codes.** The live engine's Evaluator gives the original's per-block codes exactly: 111, 82 from block 14999, 12 from block 22499 (26022 blocks).
- **Trigger and Note, ticket #60 engine.** 351 of the clone's 366 notes were the original's 367 (same block and note). Ticket #66 attributed all 16 original notes it missed by replaying the extract through the harness (`run8::replay_from`), changing one input at a time from an original-mode replay that gives all 367 notes exactly:

| Notes | Original blocks (read ms) | Input that reproduces them alone | Cause |
| --- | --- | --- | --- |
| 10 | 44 to 664 (157 to 4841 ms) | Source chain cold at the graph start (all 10) | the original's Source filters had run the 31 s of stream before Play |
| 3 | 7995, 17191, 21025 (63401, 137028, 167720 ms) | one clock read per block, lap read = block read (each fires one block early) | the original's counter takes a second read to restart its timer on a fire, 4 to 6 ms after the block read |
| 3 | 19451, 24268, 24330 (155107, 193969, 194479 ms) | fixed mode's threshold window (only variants with it) | fixed mode's one-entry erase, a deliberate correction ("Fixed mode against original mode") |
| 0 | none | all-scalar-head sum order against the logged flags (367 of 367) | none; all-SIMD moves 4 triggers (blocks 10108, 20950, 21025, 22655) one to three blocks early |

- The cold-start, one-read and fixed-window variants together reproduce the #60 engine's 351 exactly. Of the 10 start notes, 7 also move with the 45 Hz low-pass switched in at the graph start and not four samples later; a fixed-mode replay with a warm chain still differs on those 7, because fixed mode switches at the graph start by design.
- **Warm-up length.** A fixed-mode replay warmed for 0 samples before the graph start differs from one warmed for the full 15616 on 9 notes, 128 samples on 1, and 256, 500, 1000, 1500, 2500 or 5000 samples on none. `live::run` warms the Source filters, with the interference filter off, for `GraphStart::LIVE.warmup_samples` (500 samples, 1 s) of the stream before the graph's first block.
- **Ticket #66 engine.** With the warm-up and the counter's second read on a fire, `live::run` in original mode, given the original's start (15616 samples of warm-up, switch 4 graph samples in) and its clock and lap reads, reproduces the block and note of all 367 triggers; `replay_live` asserts it (`assertions::trigger_notes`). The live engine sets the all-scalar-head order in original mode too, since the graph's original-mode default is all-SIMD. Inferred, from the harness replays above: a fixed-mode live run on the same stream differs from the original only on the 7 start notes and the 3 threshold-window notes, all deliberate fixed-mode corrections.

## PeakFrequency and Note Hook Points (static, braintellect.exe 5.1.57)

Image base `0x400000`; all addresses are VAs. Recovered with Ghidra headless. The window, FFT and bin range of `0x632d90` are superseded by the Run 4 sections below; the remaining items are inferred from disassembly.

### Observed in code

- `DSPPkFreq::filter` is `0x632d90` (`__thiscall`, args: input float pointer, window length `N`, two output pointers). It copies the window to doubles, applies a window function through the statically linked IPP-style thunks (`0x66fd20`..`0x66fe40`), runs a real FFT, and reduces the magnitude spectrum to one peak. It writes the peak frequency to `*(this+0x58)` and the peak amplitude to `*(this+0x38)` as float32. The frequency is `(lowIdx + idx) * (rate / N)` with `idx` the argmax over the bins from `lowIdx` to `highIdx`.
- The `PeakFrequency` element's process function is `0x6340d0`. For each new sample it shifts the window held at `this+0x1bc` (length `this+0x18c`, fill count `this+0x1c0`) and calls `0x632d90` once, so the element recomputes the peak per sample.
- Threshold: pass test `0x62ed70` (`__thiscall`, one float), thresholds as doubles at `this+0x100` and `this+0x110`, pass flag at `this+0x89`; direction at `this+0x7c`, mode at `this+0x84`; automatic quantile recompute `0x62e6e0` using the order-statistic reader `0x62e3c0`. The quantile index is `round(n * (1 - q))` (or `round(n * q)` for the other direction), reduced by one when it equals `n`.
- Counter block processor `0x642ed0` (`__thiscall`): the count at `this+0x18c` increments on each pass, and the trigger is raised and the count reset when the count reaches the `timeHigh` bound at `this+0x1a0`, compared against a per-block timer.
- `SoundElement` pipe handler `0x59ad70`, note step lookup `0x5a0030` (maps the incoming note value into a 12-step pattern relative to `this+0x2c8`), and note play `0x5a04e0` (clamps the note to the range at `this+0x2d4`/`this+0x2d8`, then calls the output device through vtable slots `+0x1c`, `+0x20`, `+0x24`).

### Not resolved

- The conversion of the note input to the MIDI note number, including `noteAutoRange`.
- Whether the order-statistic index is zero- or one-based.

### Why a live capture is required

Static reading does not fix the unresolved items, and the stored baseline carries no note or trigger decisions (the type-9 XML has no PeakFreq, note, threshold or counter output). A capture needs the hooks above, logged with the same millisecond clock as the serial trace, during one SMART-2 session.

## Run 4 Capture: Stages Upstream of the Threshold (observed)

One complete SMART-2 session (guest profile, no wearer, sensors off, 240 s, 30,005 blocks of 4 samples) under one combined Frida script, hard-restarted app. The raw logs stay in the main checkout's `tmp/s26/run4/` (never committed). Verified by replaying the logged inputs against the logged outputs:

- **PK hook silence in run 3:** `0x632d90` and `0x6340d0` are called. The run 3 `onLeave` read dereferenced a wrong object or field (access violation at 0x200), which Frida swallowed. Run 4 logged the same failure as an error; the PeakFreq values were captured instead as the A..D inputs of `ExprNoteSelection`.
- **Channel order:** see `b2-protocol.md`; PeakFreq Ch`n` reads payload slots `n-1` and `n+3`.
- **PeakFrequency:** exact algorithm recovered from `0x632d90`, see "Recovered upstream stages" below. 99.7 % of 1000 samples match once the Source software filters (25 Hz notch, 1.5 Hz high-pass) are applied before the window; the earlier 97.8 % was the raw channel windowed directly.
- **ExprNoteSelection:** input `x` is PeakFreq D when `J == 183`, else `(C + D) / 2`. Per sample `x` feeds a tracker (trailing 15000 samples, min and max) and a smoother (trailing 62-sample mean). The note is `trunc((hi - lo) * ratio) + lo` with `lo = 40`, `hi = 76`, `ratio = (mean - min) / (max - min)` (0 when the range is not positive), in float32 through the map at `0x578240`. The note is taken at each counter fire. Exact against the logged inputs.
- **ExprTriggerAll:** one branch per Evaluator code, read from the design's own expression (`tmp/s26/run4/130-t1.xml`): 11 gives `A + B + C + D`, 12 `A`, 13 `B`, 81 `C + D`, 82 `C`, 83 `D`, 112 `E`, 113 `F`, 181 `G + H`, 182 `G`, 183 `H`, and any other code (111 included) `E + F + G + H`. The sums are formed in double precision and rounded to `float32` once. The Threshold input is that value in volts, smoothed in microvolts by a 62-sample smoother. Exact against the logged inputs for codes 111, 183 (run 6) and 12, 82 (run 8). Inputs A..D are the Band 10 (1-3 Hz) then Band 80 (23-36 Hz) BPF amplitudes of Ch1, Ch2 and E..H the same for Ch3, Ch4 (A, B = Ch1, Ch2 Band 10; C, D = Ch1, Ch2 Band 80; E, F = Ch3, Ch4 Band 10; G, H = Ch3, Ch4 Band 80).
- **Evaluator J:** 111 until 120000 ms, 183 afterwards (first block with 183 is 14999) in run 6; 111 for blocks 0 to 14998, 12 for the next 7500 and 82 for the last 7502 in run 8 (a different SMART-2 setting). The clone's Evaluator reproduces both sequences from band amplitudes ("Replay capture of the Evaluator inputs"); the parity tests can still feed the logged codes.

### Not resolved

- The first about 600 logged samples of Band 10 and Band 80 (BandPassFilter start-up) do not match with a warm Source chain: that replay reproduces neither the size nor the timing of the early transient. Run 8 resolves the start-up for its exercise (warm Source chain, 45 Hz low-pass switched in cold; see "Run 8 whole-exercise capture"). Run 4's account, kept for its figures: cold, Band 10 matches within about 1 to 3 % up to block 44 (sample 176) and then diverges (the logged transient stays large until about block 80, the replay's decays by block 60). The remaining cause is not isolated (Source IIR transient details, or the start-up of the band-pass itself).
- Band 10 on Ch2 (correlation 0.73 at a median error of about 1 %): a few late-held maxima in the 83-sample max window differ, cause not isolated. The Rust replay (`apps/braintellect-linux/tests/parity_smart2_stages.rs`) leaves it out of its stage figures, as it does the start-up span above.
- Replaying run 4's decoded EEG through the clone never matched the run-4 band amplitudes (1 to 5 % off) or all notes. The run 6 baseline replays from the live Source output and matches exactly (see "Run 6 baseline"); run 8 replays from decoded EEG and matches the triggers and nearly all notes (see "Run 8 whole-exercise capture").
- The exact IIR design behind the Source notch and high-pass (IPP generator internals) and whether the 150 Hz low-pass FIR is applied: the replay cannot tell it from a 4-sample change of alignment.
- Why the log records `dataAlreadyFiltered: 1` while the software filters demonstrably act (`+0x10c` flag; see "Source-side software filters").
- The Aggregator numeric replay against logged output (the code reading is below).

## Recovered upstream stages (static decompile, checked offline against run 4)

Headless Ghidra on `braintellect.exe` 5.1.57, image base `0x400000`; the classes were found through MSVC RTTI. IPP calls go through thunk tables indexed by `_DAT_0200f428`; thunk identities below are inferred from argument shapes, not from names. Scratch scripts and captures stay in the main checkout's `tmp/s43/` (not committed). Checks replay the run 4 serial frames (channel `c` = payload slots `c` and `4+c`, volts per `b2-protocol.md`) against the logged stage values.

### Alignment (observed)

Logged sample 0 of Ch1 is absolute channel sample `2 * 34000 + 3659` of the run 4 stream (frame about 35829.5), found by sliding-DFT against the logged PeakFreq. Earlier guesses were wrong.

### BandPassFilter OutAmplitude (decompile, partly matched)

Vtable `0x18bf7fc`; rebuild `0x66dc00`; FIR constructor `0x650ed0`; IIR constructor `0x6536a0`; apply `0x651490`; amplitude `0x650cf0`; sizing `0x6506c0`; 16-tap setup `0x6507e0`; per-block process `0x66e9f0`.

- Stage 1: `y` = FIR of the input, taps from `ippsFIRGenBandpass` with edges `f / rate`.
- Stage 2: `tmp[i]` = max-abs (`ippsMaxAbs_32f`) of `y` over the last `W` samples (sliding, zero-initialised history). `W = max(trunc(rate / (2 * highCutoff)), blockLen)`: Band 10 (1-3 Hz) gives `W = 83`, Band 80 (23-36 Hz) gives `W = 6`.
- Stage 3: `OutAmplitude` = 16-tap FIR of `tmp`, each tap `2.0 / 16` (constant `2.0` at `0x18953a8`).
- This explains the earlier puzzle: Band 10 holds for runs because the 83-sample max window changes rarely; the 16-tap mean produces the linear ramps between holds; Band 80 (`W = 6`) never holds. A Hamming FIR followed by `|y|` and a boxcar could not reproduce either.
- The input to this element is not the raw frame: the Source applies the software filter chain described in "Source-side software filters" first. The earlier fit that inferred a 1.5 Hz high-pass was the high-pass slot of that chain; the Band 80 mismatch was its 25 Hz notch.
- Offline match with that chain (run 4, 2000 samples, FIR order 128 = 128 taps, Hamming, `W` 83 and 6, 16-tap `2/16` mean): see the match table in "Source-side software filters". Band 10 steady state and Band 80 after about 600 samples match; the start-up does not.

### Source-side software filters (decompile, matched)

The transform between the raw B-2 frames and the Source outputs that feed BandPassFilter and PeakFrequency is the device-layer software filter chain of `BSTAmplifierDataProcessor`.

Observed in code:
- Per sample, `processData` (`0x555c00`) converts the 3-byte big-endian value (`0x555930`; scale factors from `0x5558c0`) and, when the byte at `this+0x10c` is zero, calls `0x557f00`, which runs the channel's filter slots through the filter apply `0x651490` in this order, skipping null slots: `+0x30` interference filter, `+0x34` interference filter 2, `+0x38` low-pass, `+0x3c` high-pass. The result goes to the engine queue `0x5773a0`.
- `setInterferenceFilter` `0x554fe0`: `"60 Hz"` IIR band-stop (kind 3, type 5, order 1, 59/61 Hz); `"50 Hz"` the same at 49/51 Hz; `"50/60 Hz"` FIR band-stop (Hamming, 64 taps, 42/68 Hz); `"45Hz LP"` FIR low-pass (Hamming, 256 taps, 47.5 Hz).
- `setInterferenceFilter2` `0x555370`: `"25 Hz"` IIR band-stop (kind 3, type 5, order 2, 23.5/25.75 Hz, ripple argument 0.1); `"10 Hz"` FIR band-stop (Hamming, 384 taps, 9/11 Hz).
- `setLPFilter` `0x554ae0`: FIR (`0x650ed0`), kind 0, Hamming, 24 taps, requires 0 < cutoff <= rate. `setHPFilter` `0x554d60`: IIR (`0x6536a0`), kind 1, type 5, order 1, ripple argument 0.1.
- Run 4 `app.log` (observed): `interferenceFilter -> Off`, `interferenceFilter2 -> 25 Hz`, `lowPassCutoff -> 150.0`, `highPassCutoff -> 1.5`, `sampleRate: 500`, `dataStorageStyle: filtered`, `dataAlreadyFiltered: 1`.
- The 25 Hz band-stop design (observed in code, decompiled and run under Unicorn): `0x653460` builds an order-2 Butterworth analogue low-pass prototype (poles `0x652ac0`, pushed by `0x6529c0`/`0x6527f0`, `std::complex<double>` x87 code), transforms it to a band-stop (`0x651bd0`, `0x653010`), forms biquad coefficients (`0x652020`) and normalises the gain (`0x650a20`). The IIR object (`0x6536a0`) holds two cascaded biquads, taps `b0 b1 b2 a0 a1 a2` per section (`a0` is 1), with the gain folded into the first section only; it hands them to the IPP biquad init (`0x670050`) and applies them through `0x670040`. For 23.5 to 25.75 Hz at 500 Hz the taps are section 1 `0.9802055417010919, -1.86748240405372, 0.9802055417010919, 1, -1.8927648988910217, 0.9808192344572515` and section 2 `1, -1.9051947011162667, 1, 1, -1.879538922464532, 0.9795922720679167`. The last digit of section 1's `a2` is `...2514` or `...2515` with the x87 precision control (64 or 53 bit); the two are indistinguishable in the run-8 output.
- The IPP IIR kernel with the x87 generic path (`0x68f350`, selected through the dispatch tables `0x1eed3e0`/`0x1eed3f8`/`0x1eed410` by `_DAT_0200f428`) evaluates each section in transposed direct form II: `y = b0*x + d0; d0 = b1*x + d1 - a1*y; d1 = b2*x - a2*y`, input `float32`, state `f64`, output rounded to `float32`. The kernel is entered at `0x68f2b0`; the other variants (entries `0x68e904`, `0x68df64`, `0x68d5c0`, `0x68cc4c`) were not read; with the real taps direct form II and transposed direct form II give the same `float32` output over the whole run-8 exercise, so the data does not select one.
- The BandPassFilter element (`0x66dc00`) picks FIR (`0x650ed0`, mode 0) or IIR (`0x6536a0`, mode 1) from `this+0x198`; the run 4 design gives every BandPassFilter windowType Hamming and filterOrder 128.

Inferred:
- Type 5 is Butterworth and the high-pass is a first-order bilinear Butterworth (`b0 = 1 / (1 + w)`); the clone builds it from `tan`. The band-stop no longer is inferred: the clone uses the app's own taps (see Observed in code above). The earlier design, a single 4th-order polynomial from an order-2 prototype in direct form I, differed from the app's taps at the last bit and is removed.
- The flag at `+0x10c` is zero in the run 4 path: the 1.5 Hz high-pass and the 25 Hz notch both visibly act on the data, although the log reports `dataAlreadyFiltered: 1`. The flag's source (a string compare in the constructor `0x5576b0` at `0x5577ab` against a runtime-initialised global) was not resolved.
- Alignment: the best fit is 4 samples later than the earlier PeakFrequency-derived start (logged sample 0 = channel sample `2 * 34000 + 3659 + 4`). A 150 Hz Hamming FIR low-pass would add about 11.5 samples of delay; with it, the best offset moves to about 15 and the match is the same, so whether the low-pass runs cannot be decided from this data.

Offline match, run 4, filters started warm (6000 samples of history), band-pass started at zero state at logged sample 0, pure-Python replay (`python3 -I`, no signal values recorded here). Correlation and relative error are against the logged BandPassFilter amplitude:

| Input to the band-pass | Band 10 (n >= 450), Ch1..Ch4 | Band 80 (n >= 600), Ch1..Ch4 |
|---|---|---|
| raw frame | high-pass missing: scale off about 12x | not tried |
| 1.5 Hz high-pass only | corr 0.998 / 0.730 / 0.971 / 0.987 | corr 0.648 / 0.666 / 0.424 / 0.193 |
| 25 Hz notch + 1.5 Hz high-pass | corr 0.998 / 0.731 / 0.971 / 0.987 (median error 1.0-2.5 %) | corr 0.997 / 0.998 / 0.999 / 0.998 (median error 6.2 / 3.2 / 1.7 / 1.7 %, 90th percentile 31 / 13 / 7 / 7.5 %) |

- PeakFrequency bin, Ch1, samples 1000-1999: raw 986/1000 at offset 0, 974/1000 at offset 4; notch + high-pass 997/1000 at offset 4 (high-pass only 992/1000). The earlier 97.8 % figure was over a different 2000-sample span.
- Band 10 Ch2 stays at 0.73 although its median error is 1 %: a few late-held maxima differ (the 83-sample max window is sensitive to single peaks).
- Not matched: the first about 600 samples of both bands (see "Not resolved"). This table started the Source filters warm; that is superseded: the logged Band 10 start-up matches only with cold Source filters ("Note divergence from decoded EEG").

### Run 5 live Source-chain capture (observed)

A Frida hook injected into the running `braintellect.exe` (no restart) logged the inputs and outputs of the filter apply `0x651490` per sample for channel 0 at the four Source return sites `0x557fb5`, `0x557fe6`, `0x558011`, `0x55803c`, with every read range-checked. The session was a no-wearer, sensors-off stream with no exercise running; 6036 samples were captured. Raw logs stay in `tmp/s43/live/` (never committed); `smart2_source_stages.tsv` holds the scrubbed stage values.

Observed:
- All four slots run, one call each per sample (n = 1), and each stage's output is bit-identical to the next stage's input. Slot kinds: `+0x30` kind 0 (FIR, 256 taps, edge 47.5 Hz: the `"45Hz LP"` interference filter), `+0x34` kind 1 (IIR, edges 23.5/25.75 Hz), `+0x38` kind 0 (FIR, 24 taps, 150 Hz), `+0x3c` kind 1 (IIR, 1.5 Hz). Samples are `float32`. The live guest profile has interference filter 1 on; the run 4 design has it `Off`, so slot `+0x30` is not part of the SMART-2 design the clone replays.
- Free fits of the captured stage input to output: the low-pass is a symmetric 24-tap FIR with unit DC gain; the band-stop is a 4th-order IIR (the two cascaded biquads under "Source-side software filters"); the high-pass is first order with `b0 = 1 / (1 + w)`, `w = tan(pi * 1.5 / 500)`.
- Fed the captured band-stop input, an f64 chain (Hamming 24-tap low-pass unit DC gain, the band-stop, first-order bilinear high-pass) with each stage output rounded to `float32` reproduces every stage output bit for bit once the filter memory has converged (3000 samples; the capture started mid-stream). Rounding is required: without it the outputs differ in the last `float32` bit. The IPP IIR filters compute with `f64` state and coefficients and round only the output (an `f32` state or `f32` coefficients matches 15 to 62 % of samples).

- The 256-tap slot `+0x30` is a Hamming low-pass at 47.5 Hz with unit DC gain (the clone's `lowpass_taps`), computed as the `f64` sequential sum and rounded once to `float32`. The clone's full four-stage chain (`Interference1::LowPass45`) reproduces the live band-stop, low-pass and high-pass outputs bit for bit from the chain input (`smart2_source_stages.tsv` column 0 is the slot `+0x30` input), 5736 of 5736 samples after the 3000-sample convergence. The setting is the `interferenceFilter` device preference (`"45Hz LP"`, `setInterferenceFilter` `0x554fe0`), not a design XML field; the clone keeps it as `SourceDesign.interference1`.

Consequence: the Source chain is exact for the live design (`Interference1::LowPass45`) and for the SMART-2 design (`Off`).

### Run 5 live BandPass and PeakFrequency capture (observed)

After a hard restart (process kill, no graceful exit) and an exercise start, a second hook (`hook7`) logged, with pointers and lengths validated before every read: the `ippsFIRGenBandpass` tap output once per BandPassFilter object, each object's per-block input, FIR output and `OutAmplitude` (apply `0x651490`, amplitude `0x650cf0`, call site `0x66ea81`, four samples per block), and the PeakFrequency input window and outputs (`0x632d90`). Four channels each have Band 10 (1.0 to 3.0 Hz) and Band 80 (23.0 to 36.0 Hz) objects; about 5800 samples per object were kept. Raw logs stay in `tmp/s43/live/`.

Observed:
- The generated taps (128, `f64`) equal the clone's `bandpass_taps` (Hamming, unit gain at the band centre) to 1e-16. Edges are 0.002 to 0.006 and 0.046 to 0.072 of the sample rate.
- The FIR output is the sequential `f64` sum of `tap[k] * x[n-k]` (k from 0, zero history at object start) rounded once to `float32`. The clone reproduces every FIR output of Band 10 and Band 80 bit for bit from the object's first sample.
- `OutAmplitude` is a sliding maximum of the absolute FIR output over `W` samples (`W = 83` for Band 10, `W = 6` for Band 80; the maximum is exact) followed by a 16-tap mean (`0x66ffc0`, `float32` taps of 0.125).
- PeakFrequency's window is an `float32` array filled from index 0 with the Source output (the same samples the band-passes read) until 512 have arrived, then the oldest drops off the front. The window is the symmetric Hann (denominator `N - 1`), and the clone reproduces the logged frequency and amplitude bit for bit over all captured samples except sample 1 (two nonzero samples, a flat spectrum whose bin is set by FFT rounding). The amplitude is computed in `float32` (`(4 / N)`, the magnitude and the window mean each rounded to `float32`); the same in `f64` differs from the app in 26 % of samples at the last `float32` bit.

Resolved by the run 6 baseline (below): the 16-tap mean is the IPP `float32` FIR (`ippsFIR_32f`), bit exact in the sum order the kernel uses for the block's heap alignment.

### PeakFrequency (decompile, matched)

Vtable `0x18bc414` (secondary `0x18bc400`); `DSPPkFreq::filter` `0x632d90`; constructor about `0x633400`-`0x633f23`; process `0x6340d0`; free `0x632c10`.

- `N = 2^fftOrder = 512` (`fftOrder` 9), `df = 500 / 512`. Bins `ftol(low / df)`..`ftol(high / df)`: Ch1, Ch2 6..14; Ch3, Ch4 7..15.
- The input is a ring of `this+0x18c` samples, newest at the end once full, filled from the front with the tail zero until then (observed in the run 5 capture), windowed by a symmetric Hann window (`ippsWinHann_64f_I` over ones, denominator `N - 1`; window mean at `this+0x68` is sum / N).
- Unscaled real FFT (flag 8), conj-pack expansion to complex, magnitude, first maximum over the bin range. Frequency = `(lowbin + idx) * df`; amplitude = `(4.0 / N) * max / mean(window)` (constant `4.0` at `0x1895398`).
- Thunks (inferred): `0x66fd90` Zero_64f, `0x66fd80` Mul_64f_I, `0x66fd70` FFTFwd_RToPack_64f, `0x66fd60` ConjPack_64fc, `0x66fd50` Magnitude_64fc, `0x66fd40` MaxIndx_64f; window selectors `0x66fda0` Hann, `0x66fdb0` Hamming, `0x66fdc0` Kaiser.
- Superseded by the run 5 capture: the denominator is `N - 1` and the result is `float32`, bit exact on the live input. Earlier offline match: 1955 of 2000 samples (97.8 %) with a Hann denominator `N` (the `N - 1` and `N + 1` variants and an added high-pass were no better on run 4). The mismatches are near-ties between adjacent bins, so the true input differs slightly from the raw frames, consistent with the unlocated Source-side stage.

### Aggregator (static, partial)

`AggregatorFilter` vtable `0x18bf91c` (secondary `0x18bf90c`). Observed in code: reset on message type 3 (`0x649c70`); property setter `0x649b60` creates the aggregation object at `this+0x1e4` from the flow-style string; setup `0x64a3e0` passes `rate * [0x62]` (sample rate times `averageEpoch`) to that object's virtual `+0x18`.

- Style strings (static initialisers at `0x13f8620`, `0x13f8640`, `0x13f8660`): `0x1fbf194` = `"absolute"`, `0x1fbf3c4` = `"continuous"`, `0x1fbf178` = `"discrete"`. `"absolute"` creates the `0x18bf978` object (0x70 bytes; running sum in a double at `+0x60`, count at `+0x68`; value = sum / count, `0x649850`). `"continuous"` and `"discrete"` create the `0x6542a0` object (0x60 bytes, vtable `0x18c1a98`). The run 4 design uses `"continuous"` with `averageEpoch` 60 for all six Aggregators, so the `0x18bf978` object is not on the replayed path.
- `0x18c1a98` per-sample (virtual `+8`, `0x6543f0`): `v' = scale * v + offset` (floats at `+0x50`, `+0x54`, copied by the constructor from the owning property block at `+0x18c`, `+0x190`). When the window length at `+0x58` is above zero (set by virtual `+0x18`, `0x654510`, which also rebuilds the buffer through `+0x1c`, `0x654530`), `v'` goes through the moving average `0x654fe0`; otherwise `v'` is the output. The block entry (virtual `+4`, `0x6544d0`) just repeats the per-sample call. The output is held as a float at `+0x5c`.
- Moving average `0x654fe0` (object `0x654f20(window, 0, 1)`): circular buffer of doubles. Running sum += `x`, then -= the overwritten oldest entry (float arithmetic, stored in a double), the slot is overwritten with `x`, and the result is sum / `min(samples seen, window)`. This is a sliding boxcar mean with a partial-fill divisor, window `rate * averageEpoch` = 30000 samples at 500 Hz, matching the 60 s boxcar read earlier from the logged sum and window. Inferred, not replayed: `scale` = 1 and `offset` = 0 (not read from the design); the Evaluator-side `0x654440` (sign times log) variant is not used by this path.

### Note divergence from decoded EEG (observed, with inferences marked)

Ticket 43. Notes at the 20 counter fires from decoded EEG matched 9 of 20 with warm Source filters and match 17 of 20 with cold ones. Per-block comparison against the run 4 logs (clone `PeakFrequency` bins versus the logged `ExprNoteSelection` inputs, clone tracker range versus the logged one):

- **Observed, cause of the 11 early misses: the tracker minimum.** The logged range of the selected PeakFrequency `(C + D) / 2` is `[7, 15]` bins from the first block (block 0 has Ch3 = Ch4 = 7 at sample 0). With warm filters the replay's minimum fell from 11 to 7 only by block about 850, so every note before then was scaled by a wrong lower bound. The tracker, smoother and `noteAutoRange` logic are not at fault: replayed from the logged PeakFrequency they are exact, and no state carried before the logged span matters (the app's first `NP`/`TR` logs start at block 0 with `cnt=1`).
- **Observed: the Source filters start cold at logged sample 0.** The stream ran about 143 s before the graph's first sample (`SM` smoothers log from `1793911027988`, the first `PKIN` at `1793911171296`), but the logged Band 10 amplitudes in blocks 0 to 44 follow a zero-state Source chain (high-pass step transient of the sample offset) to within about 1 to 3 %, where the warm replay is 2 to 8 times off. Superseded by run 8 ("Run 8 whole-exercise capture"), where exact logs show the chain warm with only the 45 Hz low-pass switched in cold at the graph start. The `+15` sample alignment offset is kept; scanning it from -40 to +40 with cold filters, 0 minimises the PeakFrequency bin mismatches.
- **Observed: counter fires.** With cold filters the replay fires on the baseline's blocks except fire 1 (block 48 against 49, same millisecond) and fire 5 (block 359 against 358): the pass flags near the Threshold differ by one block because the Band amplitudes carry 1 to 5 % error. With warm filters the first three fires were 16 blocks early.
- **Remaining misses (fires 1, 2, 5).** Fire 1: the replay fires a block early (pass-flag timing above) and the note path differs by 2 steps. Fire 2 (block 128): the replay's Ch3/Ch4 bins in blocks 108 to 127 are 7 and 15 where the app logs 10 and 7; the first 512 samples (window not yet full) depend on the Source transient that is not matched past block 44. Fire 5: counter one block late.
- **Tried, rejected (observed):** filling the PeakFrequency window from the front instead of zero-padding at the front of the ring reproduces sample 0 of all four channels (the Hann weight at index 0 is zero, so the spectrum ties at the lowest bin) but raises the bin mismatches over blocks 0 to 129 from 1065 to 1324 of 2080 and leaves 17 of 20 notes; reversing the window orientation raises steady-state mismatches. Bins in the first blocks are decided by FFT rounding on near-flat spectra (a single nonzero sample), which the replay's FFT cannot reproduce.
- **Observed (ticket 43g): the 45 Hz low-pass does not explain the run-4 residuals.** Run 4's `app.log` has `interferenceFilter -> Off`, so the stage is absent there. Replaying run-4 decoded EEG through the clone with `LowPass45` and scanning the start offset -140 to -100 (about the stage's 127.5-sample group delay) raised the median Band 10/80 amplitude error from 0.9 to 5 % (stage-dependent) to 25 to 60 %, the PeakFrequency bin mismatches from 114 to over 3600 of 4000 (samples 600 on), and the notes from 17 to at most 12 of 20. The residuals stay open and the stage is not part of the run-4 design.
- Not resolved: the Source transient past block 44 (candidate: the IIR design or initial state; the 150 Hz low-pass FIR, with the alignment offset, did not discriminate in a scan of 33 offsets with and without it).

### Run 6 baseline (observed)

Ticket 43h. One SMART-2 session (no wearer, sensors off) after a hard restart, with serial RX, the Source output, every band-pass call (`0x66ea81`: input, FIR output and amplitude of four samples, plus each object's edges), PeakFrequency and every Expression/Threshold/Counter stage hooked. Raw logs stay in `tmp/s43/baseline/`; `make-fixtures.py` builds the `smart2_*` fixtures from the last run in them.

- **The 16-tap mean is exact.** `OutAmplitude` is the `float32` FIR of the last 16 sliding maxima with taps 0.125, each product rounded to `float32`. The kernel (`0x67a000`, thunk `0x66ffc0`) sums in a fixed order that depends on the output buffer's alignment (`dst % 16`). Aligned to 16: SIMD order for all four outputs: `(p0+p1)+(p2+p3)`, then for each further group of four `s+=p3; s+=p2; s+=(p0+p1)`. Aligned to 8: the first two outputs of the block sum sequentially from 0 (scalar head), the last two use the SIMD order. This explains the earlier "hidden state at block positions 0 and 1". The alignment follows the heap and is not logged, so each block's order is recovered from the logged amplitude (a block both orders explain is read as SIMD): every one of about 13.8k blocks times 8 bands is explained. The clone takes the order per block as input (`BandPass::set_scalar_head`).
- **Input is the live Source output.** The run 6 fixtures start at the live Source output (`smart2_stage_inputs.tsv`, `float32` volts), and the Source chain is verified against its own live capture (run 5). Run 8 supplies the serial stream and the consumed samples on one clock (below).
- **Everything downstream is exact** from that input: Band 10/80 amplitudes, Aggregators, PeakFrequency bins (except sample 1, the flat spectrum), every block's note after the first 15, the notes at all 229 counter fires and the trigger decisions (count and kind exact, times within the harness window).
- **Counter timer: not exact in run 6.** The counter fires on wall-clock block times. A fire is one block early or late in some stretches (blocks 110 to 210, 1119 to 1163 of the first 1500) and a shifted fire reads a different note, so the full note-decision list is not asserted. Run 6 did not log the timer reads; run 8 did, and explains the shift (next section).

### Run 8 whole-exercise capture (observed, with inferences marked)

Ticket 43. One SMART-2 exercise (no wearer, sensors off, about four minutes past the 120000 ms switch, 30001 graph blocks) with serial RX, the consumed per-sample volts, every band-pass call's destination, the counter timer's clock reads, the Evaluator code and the Expression, Threshold, Counter and note stages hooked into one binary log. The Source settings were the 45 Hz interference low-pass, the 25 Hz band-stop, the 150 Hz low-pass and the 1.5 Hz high-pass. Raw logs stay in `tmp/s43/run7/` and `tmp/s43/run8/`; `make-fixtures.py --src8` builds the `run8_*` fixtures (consumed EEG as 24-bit counts, the per-block table, the baseline decisions, the logged E..H amplitudes of blocks 0 to 6599) with the raw serial bytes left out.

Observed:

- **Received versus consumed.** The serial trace holds 149,122 frames and the app consumed 148,896 of them: 76 counter jumps drop whole frames. Matching each consumed ch0 volt forward into the received stream finds a counterpart for every consumed sample, so the consumed stream is a subsequence of the received frames. Earlier wording that decoded EEG cannot reproduce the app's input was a limit of the older logs, not of the app.
- **Conversion.** The app converts each sample as `raw / DIV` in `f64` with `DIV = 12 * (2^23 - 1) * 10 / 2.4`, then rounds to `float32` when the graph takes it. This is exact for all 272,793 logged ch0 samples. Channel `c` reads payload slots `c` and `4 + c` (`b2-protocol.md`).
- **Graph start.** The graph's first block takes the consumed sample at index 25600 and block `b` takes the four samples from `25600 + 4b`. Observed: the Source chain is warm there. Its logged output matches, 1600 of 1600 samples, a chain that ran the earlier stream with the interference filter off and has a zero-history 45 Hz low-pass switched in at consumed sample 25601, while the band-stop, 150 Hz low-pass and high-pass keep their state. Inferred: the exercise start calls `setInterferenceFilter` (`0x554fe0`), which builds a new interference filter. The clone does this with `SourceFilters::set_interference1`; the fixtures carry 4000 consumed samples before the graph start to warm the chain.
- **Band-pass to object mapping.** The eight band-pass objects map to bands A..H by matching each object's logged amplitudes to the clone's (exact, 3200 of 3200 blocks per band); the 16-tap-mean sum order is taken from the logged destination alignment of each object's call (scalar head when `dst % 16 == 8`).
- **Counter timer.** The counter tests `now - start >= 500` with `now` the GetLocalTime time of day in ms read at evaluation. On a fire the app takes a second read as the new start, which differs from the evaluation read by 1 to 6 ms in 17 of 415 fires. The clone's counter takes both reads (`CounterClock`): a replay supplies them as logged (`BlockClock`), and `live::run` reads its clock again on a fire. That removes the one-block fire shifts of run 6.
- **Evaluator code.** 111 for blocks 0 to 14998, 12 for the next 7500, 82 for the last 7502 (`ExprTriggerAll` branches above).

Replay from the consumed EEG, Source warm from the 4000-sample pre-roll, with the logged codes, clocks and sum orders (`apps/braintellect-linux/tests/parity_smart2_exercise.rs`):

- All 415 baseline fires are reproduced in count and kind; each lands within the harness window.
- Notes at the fires: all 415 are exact (`note_decisions_match_the_baseline_exactly`).
- The logged E..H band amplitudes are bit exact over all 30001 blocks (checked once with a throwaway counter; the test asserts blocks 0 to 6599, which spans the former burst at blocks 6409 to 6477; the burst at 12082 to 12175 is exact too but not asserted, to keep the fixtures under 500 KB). Before the band-stop used the app's taps, 93 blocks differed at the last `float32` bit, all in bands F and H (channel 4), in two bursts starting at band-stop outputs 1579 and 79 `f64` ulps from a `float32` rounding midpoint; the cause was the clone's single-polynomial coefficients, not the structure (see "Source-side software filters"). Bands A..D are not logged beyond the early blocks.
- **Threshold window erases by value (observed in code, confirmed by the log).** The window is a ring of `thresholdEpoch` entries plus a `multiset<float>` (`0x62e850`). After inserting the new value, when the multiset holds more entries than the ring (`this+0xe4` above the ring size), `0x62e520` removes the value leaving the ring with `multiset::erase(key)`: equal range (`0x62df60`), then erase of the whole range (`0x62e2e0`). Every element equal to that value goes, not one. A repeated smoothed value therefore shrinks the set below the ring size, and the quantile count `n` (`0x62e6e0`, `0x62e3c0` read `this+0xe4`) is the set's size, not the ring's. The pass test `0x62ed70` runs before the insert, with the threshold from the end of the previous block. The earlier report of 37 blocks whose counter count differed (first at block 6343 sample 3, the cause of fire 89's early note) came from the clone erasing one entry; with erase-by-value the counter count after every block equals the logged one (45 blocks differed when resynced per block, 0 now) and every one of the 415 Note decisions is exact. Earlier exclusions (smoother orders and windows, fixed Threshold windows, quantile index `k +- 1`, threshold lag, recompute order) stand: none of them was the cause.

#### Replay start-up (ticket 49, observed)

The #48 replay of the run-8 stream into the original (`replay-rig.md`) differed from the clone at the first Source/band amplitude (block 0 sample 1 band E, 4.4359e-10 against 4.4354e-10 logged; 4 of 128 logged E..H values in blocks 0..7 exact, first Note at block 67 40 against 42). Two inputs were wrong, neither a Source or band-pass defect:

- **Interference switch sample.** The replay's own `app.log` (`tmp/s48/rep8.fi`) shows one `setInterferenceFilter(45Hz LP)` on all 8 channels, from the UI thread (`0x108c`) 16 ms after `playSlot()`, with the same Source settings as run 8 (interference filter Off at init, band-stop 25 Hz, low-pass 150, high-pass 1.5; no second switch). The sample processing runs on another thread, so the consumed sample at which the zero-history 45 Hz low-pass takes over is not fixed against the graph start. With the graph start 25640 (unique: every other start in the range matches 0 to 4 values), switching at 25641 matches 4 of 128, at 25644 (start + 4) matches 128 of 128, and every other switch in start-300 to start+300 matches at most 16. Run 8's switch was at start + 1. The app log cannot place the switch to the sample: `playSlot()` to `setInterferenceFilter` is 15 ms in run 8 and 16 ms in the replay, one tick of the log clock (about 8 samples).
- **Band to object mapping.** The 16-tap-mean sum-order flag of each band comes from the band-pass object that owns it, ranked by address among the 18 graph-call objects. Heap addresses differ per process, so run 8's ranks (E, F, G, H = 9, 16, 12, 0) do not carry over. Over blocks 0 to 299, band E reproduces the logged amplitudes exactly under rank 7, F under 3, G under 1, H under 5 (1200 of 1200 each; no other rank gives more than 1109). With the wrong flags only the scalar-head samples (the first two of a block) differed, by one `float32` unit in the last place, which is why 118 of 128 matched at the right switch. Run 8 did not log A..D, so it keeps stand-in ranks for them (unverified). From #53 the replay captures log A..D too, and `replay_s48` locates all eight ranks per run, one rank per band reproducing the logged amplitudes over blocks 0..299 (s48: A..H = 13, 9, 0, 10, 7, 3, 1, 5; s53: 10, 0, 1, 2, 3, 6, 4, 5) and then asserts every logged A..H amplitude bit for bit over all 30001 blocks. The graph start and the 45 Hz switch are located first (s53: start 107408, 340 samples under the logged count, switch at the start; s48: 25640 and 25644). The start is found from block 0's first sample, where the bands are cold, so it does not depend on the switch.

With both, `replay_s48` matches the logged E..H amplitudes bit for bit over all 30001 blocks and all 874 Trigger/Note decisions. The graph start (25640) and the switch (25644) are still located from the logged amplitudes, not logged; `parity_smart2_exercise` keeps run 8's start + 1.

Not resolved:

- Which IPP IIR kernel variant the VM CPU dispatched (`_DAT_0200f428` was not read) and the x87 precision control at run time; neither changes the output observed here.

### Counter timer start (ticket 54)

Observed. The first logged counter evaluation of each exercise (hook tag 3, `now - start`, where `start` is the timer object's start at `+0x200`) gives the timer's age at the first graph block. No hook read the constructor's own clock, so the start is taken from that first record:

| Log | Timer age at block 0 | Consumed samples before the start | Samples from the start to graph start |
| --- | --- | --- | --- |
| Run 8 | 14882 ms | 18480 | 7120 |
| Run 8 replay (`rep8.bin`) | 21018 ms | 15452 | 10188 |
| #33 live-capture replay (`rep33.bin`) | 3707 ms | 14104 | 1512 |

- The offset is not fixed: it varies from 3.7 to 21 s, and neither the age, the sample count before the start nor the distance to the graph start repeats. The start is not derived from the graph start; it is whenever the counter was constructed before streaming reached the graph.
- Every offset is above the 500 ms epoch, so the first block was already eligible: the first fire is the first block with a passing sample, whatever the exact offset. The first fire of the run-8 original is at block 52 (first logged lap read).
- Rule used by the clone: the timer starts one epoch (500 ms) before the first block. Any start at or beyond one epoch fires identically; an earlier start (a timer started at the first block) holds the first fire back until 500 ms have passed. `live::run` takes the clock at the graph's construction and subtracts the epoch; the replay harness uses the first block's read minus 500 ms.
- Inferred: a start under 500 ms before the first block would delay the first fire, but no log shows one.

### Live sum order (ticket 54)

Observed. The 16-tap-mean sum order (the IPP scalar-head flag, from the destination buffer's heap alignment) depends on the process and cannot be derived, so a live session fixes one order for every band-pass. Run 8's logged flags set the scalar head on 120462 of 240008 band-pass calls. Replaying the same stream with every flag false (all-SIMD) and every flag true (all-scalar-head), against the logged flags (`parity_smart2_exercise`, `replay_s48`):

| Stream | Policy | Band amplitudes differing | Blocks with a difference | Trigger or Note decisions differing |
| --- | --- | --- | --- | --- |
| Run 8 | all-SIMD | 126070 of 960032 (13.1 %) | 28702 of 30001 | 0 of 830 |
| Run 8 | all-scalar-head | 125013 of 960032 (13.0 %) | 28655 of 30001 | 0 of 830 |
| Run 8 replay (`rep8.bin`; A..D flags are run 8's, so E..H are exact and A..D stand in) | all-SIMD | 127499 of 960032 | 28317 of 30001 | 0 of 874 |
| Run 8 replay | all-scalar-head | 125236 of 960032 | 28163 of 30001 | 0 of 874 |

- Chosen policy: all-scalar-head, the smaller deviation on both streams (rule: smaller measured deviation, a tie goes to all-SIMD). The gap is under 2 % of the differing amplitudes, so the choice is nearly a coin flip.
- Every difference is the last `float32` bit of an amplitude; no decision changes. Only the first two outputs of a block can differ (`BandPass::set_scalar_head`), so the last sample of each block, which the live session stores, is identical under either order.

### Fixed mode against original mode (ticket 63)

Observed. The clone's feedback graph has two modes (`apps/braintellect-linux/CLAUDE.md`, "Two feedback modes"): original reproduces the defects below and carries every parity test; fixed is what live sessions run. Fixed corrects (1) the threshold window erase-by-value (see "Threshold window erases by value": one entry leaves, so the window stays at the ring size), (2) the 45 Hz low-pass switch sample (the first graph sample, not the thread-timed one that was start + 1 in run 8 and start + 4 in the #48 replay), and (3) the 16-tap-mean sum order (all-scalar-head, "Live sum order").

Measured by replaying run 8 (30001 blocks, logged Evaluator codes and clock reads as inputs in both modes; `parity_smart2_exercise`):

| Run 8 | Original mode (baseline) | Fixed mode | Differing |
| --- | --- | --- | --- |
| Trigger decisions | 415 | 415 | 24 blocks fire differently |
| Note decisions at a fire | 415 | 415 | 24 blocks sound a different note |

The 24 and 24 are counts of blocks whose decision differs, so a fire that moves to the next block counts in both blocks. The sum order alone changes no decision (0 of 830, "Live sum order"), so the difference comes from the threshold window and the switch sample; the split between those two is not measured.

## Circle-Balance design (ticket 62)

`B-2-Circle-Balance-fromBAGSD-v5.1.1`, decrypted by #64 into `tmp/s64/design171.xml` (never committed). Static evidence from that XML and from `braintellect.exe` under `analyzeHeadless`.

### Observed: a report design, not a live one

- `flowEngine.configuration.designRole` is `BAGSD.report`. Its one `Source` feeds only `BAGSDataTypeBandFilter` elements (`InBAGSD`), which take stored BAGSData, not EEG. The design that writes that data is `designRole` `BAGSD.generate` (`design172.xml`, flow name "Session Assess 3.4-v3.3", the type-9 generator in `session-storage.md`). Strings on the report path: `run(): processing BAGSD data with design:`, `Unable to find BAGSD design`, `open(): parsing BAGSD data, type:`.
- Its input is the `LMH` data type. Every stored SMART-2 record's type-9 XML holds it: `exerciseDataChannel channelName="Channel1..4" dataType="LMH"`, series `High 1&2`, `Mid 1&2`, `Low 1&2` (Channel1 `side="left"`, Channel2 `right`; `3&4` for Channels 3 and 4), 49 points each, x every 5000 ms. The generator makes them with order-1 Butterworth band-passes (Low 1.5..6.8, Mid 6.8..15.8, High 15.8..34 Hz) into `discrete` Aggregators, epoch 5 s, the same chain #56 fitted for the `00-90` bands.

### Observed: element graph (59 elements, 72 connections)

Per chart input: `Source` `OutN` -> `BAGSDataTypeBandFilter` (`BAGSDataType` LMH, band `High` or `Low`) -> `TimeFilter` -> `AggregatorFilter` (`flowStyle` absolute, `averageEpoch` 3) -> `ExpressionTransform` (`InSignalA` channel 1 or 3, `InSignalB` channel 2 or 4) -> `CircleGraphChart`. The full wiring is `apps/braintellect-linux/tests/fixtures/circle_balance_design.tsv`.

| Chart (`chartTitle` B-2 Balance Meter) | Channels A, B | Labels | `Chart1` "Before" | `Chart2` "After" |
|---|---|---|---|---|
| `Emotional State` | 1, 2 | SELF / FEELINGS ABOUT / OTHERS | Start anchor, 60..120 s | End anchor, 120..60 s before the end |
| `Activation State` | 3, 4 | BRAKE / STUCK ON THE / GAS | Start anchor, 60..120 s | End anchor, 120..60 s before the end |

- Each graph takes two series: High on `rangeAxis` top ("OUTWARD ... STATE") and Low on bottom ("INWARD ... STATE"), both `valuesScale` 1.0, `valuesScaleAdd` 0.0. `graphMaxRangeValue` is 4 on every graph.
- All eight expressions are `ln(1+abs((((1.08*B)-A)*10)/(A min (B*1.08))))*sign((1.08*B)-A)`: the B gain 1.08 applies to channels 1n2 too, unlike the SMART-2 Evaluator.

### Observed in code

- `TimeFilter` (vtable `0x18bb0ac`, constructor `0x6244f0` with defaults begin 15, end 75, validation `0x6248a0`, receive `0x624a90`). On the start message (type 2; header `+0x90` the exercise length in seconds) a Start anchor keeps `(begin*1000, end*1000]` ms and an End anchor `((length-begin)*1000, (length-end)*1000]`, and the matching sample counts at the rate. A series message (type 8) is re-emitted with only the points `lo < x <= hi`; a sample block (type 7) is dropped outside the sample range. Validation: "End time must be greater than Begin time." (Start), "Begin time must be greater than End time." (End), "Time values must be greater than zero."
- `absolute` `AggregatorFilter` (receive `0x649cc0`; style strings as in "Aggregator (static, partial)"). A type-8 message adds every point to the `0x18bf978` accumulator (virtual `+8`) and marks a pending emit; nothing is emitted per point and `averageEpoch` is not read on this path. The pending value (sum / count, `0x649850`) is emitted once, on the stop message (type `0xc`) or the next start (type 2), through `0x649580`.
- `ExpressionTransform` (vtable `0x18bde84`, receive `0x63c1b0`): messages are paired by the first x of each (`this+0x3b8`, sentinel `0x18bda40`); `0x63a620` copies each variable's values as `float32` times a per-variable double scale cast to `float32`; `0x63b120` evaluates once per point and writes 0 for a non-finite result. The parser is muParser (RTTI `mu::ParserError`, its `Unexpected operator "$TOK$" found at position $POS$` message).
- `CircleGraphChart` (vtable `0x18a899c`, receive `0x5c7e20`, input `0x5c4b40` matching the pipe name): a series or block goes to `0x5c4550`, which adds each value times a `float32` scale (`+0x18c` of the input's property block) to a running `float32` sum held in a double, and adds the value count to a counter.

### Inferred, not replayed

- The report runs over one stored exercise: each band filter sends the whole series as one type-8 message, and the exercise length is the record's (240 s). So Before keeps x in (60000, 120000] ms (12 points) and After x in (120000, 180000] ms for 240 s.
- Each chart value is therefore one expression result per graph and band, `ln(1 + |10*(1.08*B - A) / min(A, 1.08*B)|) * sign(1.08*B - A)` evaluated in double over the two `float32` window means (muParser's default `double` value type; the scale `+0x30` is taken as 1), rounded to `float32`.
- The chart's scale at `+0x18c` is the connection's `valuesScale` (1.0), and it draws sum / count, clipped to `graphMaxRangeValue` 4. Not read: the drawing code (`0x5c7c60`, `0x5c5c80`).

### Clone status

- Has: the design wiring, `TimeFilter` window and `absolute` mean and the expression over stored `High`/`Low` LMH series (`src/feedback/circle_balance.rs`); the wiring is asserted against the fixture.
- Lacks: the LMH series themselves (the clone stores one band-pass amplitude series per channel, not the generator's 16 series, and the #56 fit of that generator is not exact), the BAGSD report runner, the `CircleGraphChart` drawing, and a session-window entry. A report design cannot run a live session; it would be selectable only as a post-session report once the clone stores LMH series.
- Blocker for an exact assertion: no capture of the original's chart inputs exists. Needed: a run of the original's report over a stored no-wearer exercise with a hook on `0x5c4550` (values and count per chart input) and on `0x63b120`'s result, plus that exercise's decoded type-9 XML. `tests/replay_s62.rs` reports the clone's values over such an XML (`scripts/lmh-extract.py`); until then they are reported, not asserted.
- Reported, not asserted: over the stored LMH series of the no-wearer exercise 146 (#56, 240 s), the clone gives Emotional Before High/Low -1.0614 / 0.0603, After -1.1350 / -0.5883, and Activation Before 0.9281 / 0.4452, After 0.9628 / 0.4069. Every window keeps 12 points.

## Export Completeness and Startup

The first tablet export survives as the extracted `tmp/bst-export/` tree; its original ZIP was not found in the worktree. Its `STATUS.txt` reports install capture succeeded, database capture failed, and registry capture did not complete. The retry archive's `STATUS.txt` reports that install capture was intentionally skipped, the raw database was captured without an SQL dump because `mysqld.exe` was not found, and no registry keys were found within the collector's bounded search. Neither export proves a complete tablet backup.

Captured configuration identifies these tablet locations:

- Program: `C:\Program Files (x86)\BST\BRAINtellect2`; its reset script adds `lib` to `PATH` before launching `bin\braintellect.exe`.
- Database: `C:\ProgramData\BST\BRAINtellect2\data`.
- Application log: `C:\ProgramData\BST\BRAINtellect2\log\braintellect.log`.
- Preferences and MySQL support resources: `C:\ProgramData\BST\BRAINtellect2\preferences` and `C:\ProgramData\BST\BRAINtellect2\share`.

Static PE import inspection identifies `LIBMYSQLD.dll` and `SessionReader.dll` in the captured program `lib` directory. `MSVCP90.dll` and `MSVCR90.dll` are imported but absent from the captured install tree; their presence on the tablet or VM and their actual loader resolution have not been established. The location or existence of a separate tablet `mysqld.exe` is unknown. Static imports do not identify every dynamically loaded dependency. No complete installer manifest has been recovered.

The retry archive's database and application-state copies contain 130 matching files by size and ZIP CRC; this comparison does not establish cryptographic identity or an atomic database snapshot. A local forced-recovery export was subsequently obtained from a copy, as described above. A program-only startup test in a fresh offline Windows 11 guest reached the error `Requested database not configured: main`. After staging copies of the captured configuration, MySQL support files, and database files, startup advanced to an error requesting a connected controller. This confirms that the configuration error was resolved, not that the database was successfully read or that startup would finish with a controller. No physical controller or tablet was connected at that point.

The controller advertises as classic Bluetooth device `B-2` and pairs without a PIN (yes/no confirmation only). Pairing it in the offline guest, through a passed-through Broadcom BCM20702 adapter (`0a5c:21e8`), created no Bluetooth COM port. The executable imports Windows Bluetooth and socket libraries, so it may connect to the controller directly over Bluetooth rather than through a COM port; that is untested.

## Wave device and instrument 1002 (static, Ghidra headless)

Observed in the binary (image base `0x400000`) and in the install's `lib` directory:

- `WaveDevice` (`media\WaveDevice.cpp`) vtable is `0x18a3570`. The `SoundElement` play path (`0x5a04e0`) calls slot `+0x1c` (`0x598060`, instrument open: reads the instrument plugin's rate, channel count, bit depth and volume, builds the `WAVEFORMATEX`), slot `+0x20` (`0x598f10`, note on: `sendNoteOnMsg`), and slot `+0x24` (`0x661f50`, returns 1).
- Note on takes a free `waveOut` handle (`0x597930`, `waveOutOpen` with `CALLBACK_FUNCTION`), `waveOutPrepareHeader` (`0x5969b0`) over the plugin's per-note buffer (plugin vtable `+0x2c` size, `+0x30` data pointer, indexed by the note), sets the volume with `waveOutSetVolume` (`0x596830`, left, right or both from the pan string) and `waveOutWrite`s the buffer. The device does not synthesise or resample: it plays the plugin's stored buffer as is, at the plugin's rate, and the host mixer converts to the output format.
- Wave instrument 1002 is a plugin DLL, `lib\dll1002.BT.dll` (exports `create`, `data`, `destroy`, plus zlib). It embeds 45 RIFF/WAVE blocks, each mono, 96000 Hz, 16-bit PCM, 153600 bytes of data (0.8 s). `dll1001.BT.dll` and `dll1003.BT.dll` are sibling banks. The recorded timbre is therefore sampled audio held in the original DLL; no waveform, envelope or table exists in `braintellect.exe` to recover. `braintellect-smart2-piano-feedback-20s.wav` (stereo, left differs from right) shows overlapping decaying notes consistent with sampled piano.
- `SoundElement` play (`0x5a04e0`): returns if the output device pointer (`+0x1f8`) is null; takes the note (`+0x194`, or the pipe value at `+0x334`), clamps it to `+0x2d4`/`+0x2d8` (and returns without playing when the matching drop flag `+0x2cd`/`+0x2ce` is set), maps it through the step lookup `0x5a0030` (12-step pattern relative to `+0x2c8`, a scale table at `+0x2f4`), and plays only when `+0x2cf` (`playOnChange`) is clear or the step differs from the last played step (`+0x32c`); see "Playback gating".

### Instrument bank layout and note map (static, recovered)

Observed in `dll1002.BT.dll` (the Qt resource inside the DLL, parsed by `apps/braintellect-linux/src/audio/bank.rs`):

- The samples are a Qt rcc resource. Each data blob is `[BE u32 length][RIFF/WAVE file]` (length is the RIFF size plus 8). The first blob's length prefix is at file offset `0x1398f0`. The name table is at `0x7d1960`, entries `[BE u16 chars][BE u32 hash][UTF-16BE]`. The tree is near `0x7d2076`, 14-byte big-endian nodes `name_off u32, flags u16, country u16, lang u16, data_off u32`; flags 0 is a file and `data_off` is relative to the first blob's prefix. Nodes are in name-hash order, so blob order is not note order.
- Note map: the file name is `AcousticStrings<N>` and the trailing decimal `N` is the MIDI note. Bank 1002 holds notes 40 to 84 (45 notes), each 96 kHz mono 16-bit PCM, 76800 frames (0.8 s). Bank 1003 has the same shape; bank 1001 parses too but its notes differ in length (note 40 has 76882 frames).
- The plugin vtable slots the device reads are `+0x2c` (buffer size) and `+0x30` (data pointer), both indexed by the note.
- The recorded playback gain is about 1.0, so the samples play unscaled. The plugin's float at `this+4` (a loudness figure) is not otherwise used by the clone.

### Playback gating (static, checked against the run-8 recording)

- Observed in code: the `SoundElement` play path (`0x5a04e0`) has no timer and no length cap, so the 400 ms play interval and 500 ms maximum length properties are not enforced there. The trigger reaches it at most once per counter interval (500 ms, the counter's epoch gate).
- Observed in the audio: each note rings its full 0.8 s with onsets about 0.5 s apart, so a note overlaps the next and they mix (sum).
- Forced note (settled). `+0x2cf` is `playOnChange`, loaded by the `SoundElement` property loader from the design; the SMART-2 design has it false, so the check passes on every trigger and a note equal to the last one plays again. The design's "Forced note: enabled" is `forceNote`, stored at `+0x2d1`, which `0x5a04e0` never reads. `+0x32c` is updated on every successful note on. The step lookup `0x5a0030` returns the note unchanged when the scale pattern marks it in scale; the recording's median pitch offset from the clone's note is 0.00 semitones over 373 fires, consistent with the identity for scale None (the scale table itself was not read). Recording check: run 8 sits at about 5649.2 s of `bst-feedback.wav` (onset-train correlation 1646 against 832 for the next best offset); the 18 of the 22 repeated notes that follow a gap of 700 ms or less each have an onset (18 of 18, as for non-repeats at 99 %), and the 4 repeats after longer gaps lose sync like the non-repeats after longer gaps. Repeats are not suppressed.
- The note is clamped to 40..76 upstream, in `src/feedback/note.rs` (`NOTE_LOW`, `NOTE_HIGH`), not in the player.
- The clone's `Player` therefore adds no gate, plays whole samples unscaled and sums overlaps, saturating to 16 bits. `tests/parity_audio.rs` replays the 415 run-8 notes and matches note, start sample and length exactly.

### Not resolved

- The exact consumer of the `playEvery` and `endAfter` properties is not found statically.
- Per-note matching of the clone's rendering against the recording is weak (correlation 0.6 to 0.77, overlapping notes), so the sample bytes themselves are not asserted.

Decision (`docs/adrs/0004-read-instrument-samples-from-the-original-install.md`): the clone does not carry the sample bytes. `braintellect-linux import <original lib dir>` copies the owner's own `dll<id>.BT.dll` banks into the clone's data directory; no vendor bytes are in the repository.

## SoundManager requires a host audio output device (session-setup crash)

Entering session setup ("Set up session" → "Select session properties") constructs
the `SoundManager` singleton and looks up its output driver by the key string
`"Wave"`. On a host with **no** audio output device the lookup returns null and the
app dereferences it, crashing with an access violation. This is independent of the
license/registration patches.

Static evidence (captured original `braintellect.exe`,
SHA-256 `af7ff5430ee10c2d548450819f54a3f939af474953a42b959fd99739519c2560`,
image base `0x400000`):

- `SoundManager::CTOR()` at VA `0x54b170` (`src\flowEngine\media\SoundManager.cpp`)
  enumerates audio hardware via winmm. It logs `number MIDI devices:` / `number Wave
  devices:` and **only registers a driver when the count is > 0**: the Wave branch at
  VA `0x54b5af` is `jbe 0x54b737`, skipping the entire `create Wave device:` block
  (`0x54b645`) when `waveOutGetNumDevs()` returns 0. MIDI is gated the same way at
  `0x54b373`.
- Session setup (VA `0x4485e0`) calls `SoundManager` getInstance (`0x54b7f0`) then the
  keyed lookup `0x549fc0` with key global `0x1f76400` = `"Wave"`
  (singleton class `com.bst.optimizer.flowEngine.media.SoundManager`). The returned
  pointer is stored at `[this+0xf4]` and immediately dereferenced for its vtable
  (`mov edx,[eax]`) at VA `0x4485fc` (file offset `0x479fc`, RVA `0x485fc`) with **no
  null check** — the faulting instruction when the registry has no `"Wave"` entry.

Observed: a crashed guest run logged Windows Application Error ID 1000, exception
`0xC0000005`, faulting module `braintellect.exe`, fault offset `0x000485FC` — matching
the static call site exactly.

Runtime remedy (offline QEMU test VM): the guest had zero audio endpoints
(`Win32_SoundDevice`, `Get-PnpDevice -Class AudioEndpoint`/`MEDIA` all empty) despite
`Audiosrv`/`AudioEndpointBuilder` running. Adding an emulated HDA output to QEMU
(`-audiodev none,id=snd0 -device ich9-intel-hda -device hda-output,audiodev=snd0`; a
null host backend still presents a render endpoint to Windows) made Windows enumerate
`Speakers (High Definition Audio Device)`, after which `SoundManager` registers the
`"Wave"` driver, session setup completes, and the crash is gone. A headless VM with no
sound hardware reproduces the crash.

Any valid `audiodev` backend satisfies the endpoint requirement. Through 2026-11-03 the
offline VM used `-audiodev wav,id=snd0,path=/storage/bst-feedback.wav`, which both
prevents the crash and records the session's feedback audio for baseline capture
(verified: a SMART-2 Piano session produced a 44.1 kHz stereo `pcm_s16le` WAV of audible
feedback tones). That backend finalizes the WAV header only on a clean guest shutdown.

Since 2026-11-04 the VM uses `-audiodev alsa,id=snd0,out.dev=bstout,out.frequency=44100`
so the operator hears the feedback live. `bstout` is an ALSA `plug` over a `file`-plugin
tee (`/etc/asound.conf` in the container, mounted from the host) that plays to the host's
analog output (`plughw:0,0`, front headphone jack) and writes the same stream to
`/storage/bst-feedback.wav` (verified on the host: the tee wrote a valid 44.1 kHz stereo
`pcm_s16le` WAV with a correct header even when the player was killed). The container needs
`/dev/snd`, and the host's onboard audio is blacklisted by default
(`/etc/modprobe.d/blacklist-audio.conf`), so `snd_hda_intel` must be loaded by hand before
the VM starts. Each boot still overwrites the WAV.

Observed 2026-11-04 with a live B-2 and headband: a full 4-minute SMART-2 Piano session
played feedback tones on the front headphone jack in real time, and the operator heard the
pitch change during the run. The tee wrote 262 s of 44.1 kHz stereo audio, with sound in 261
of those seconds. Its RIFF data length is still 0 because the guest has not shut down yet, but
the samples are complete.

## Limitations

- The recovered design establishes parameters and graph wiring, not a clinical interpretation or efficacy claim.
- The database contains sensitive material. These documents reproduce no patient names, recovered-session contents, or signal values. They do cite the dates, exercise numbers, and Evaluator control-value traces of the operator's own 2026-11-04 test sessions, which carry no personal data.
- The raw signal blobs are Java-serialized data and have not been semantically decoded in this record.
