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
| `B-2-Circle-Balance-fromBAGSD-v5.1.1` | 1,162 | balance display design |
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
- **Window:** the last `rate·thresholdEpoch/1000` values of `v`, kept as a ring and a sorted multiset (`0x62e850`). `thresholdEpoch` must be at least 50 ms.
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
- **Counter clock:** the timer is read once per block, as the static reading said. The clone reproduces every recorded fire given the observed per-block timer reading and a timer start at least 250 ms before the first block (the baseline's first fire is 250 ms after the first block). The exact start offset is unresolved.
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

**Design mismatch:** for the Ch3/Ch4 pair the Evaluator produces 112/113 and 182/183. `ExprNoteSelection` lists 102/103 and 101 instead, so 112/113 also fall to "otherwise" (average of channels 3 and 4). `ExprTriggerAll` does decode 112/113. The clone must keep this mismatch to stay at parity.

### Observed sessions

The stored type-5 logs (2026-11-04) show:

- exercise 125 (worn): `ControlLog` 111 → 111 at 01:00, `ControlChange` 111 → 82 at 02:00, then 82 held;
- exercise 126 (no sensors): 111 held throughout.

### Inferred

- With `valuesToCheck = 2`, no series is full at the 01:00 epoch, so 01:00 always reports the default. A change can first appear at 02:00.
- With no sensors, floating inputs give near-equal band amplitudes and low coherence, so neither test passes and 111 holds.
- Which path produced 82 in exercise 125 is not determined; it needs the per-epoch input values.
- No dispatch on `style` or `evaluatorLogic` was found in `processLogic`. Whether another style changes the logic is unresolved.

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
- **ExprTriggerAll:** `J == 111` gives `E + F + G + H`, `J == 183` gives `H`; the Threshold input is that value in volts, smoothed in microvolts by a 62-sample smoother. Exact against the logged inputs. Inputs A..D are the Band 10 (1-3 Hz) then Band 80 (23-36 Hz) BPF amplitudes of Ch1, Ch2 and E..H the same for Ch3, Ch4 (A, B = Ch1, Ch2 Band 10; C, D = Ch1, Ch2 Band 80; E, F = Ch3, Ch4 Band 10; G, H = Ch3, Ch4 Band 80).
- **Evaluator J:** 111 until 120000 ms, 183 afterwards (first block with 183 is 14999).

### Not resolved

- The first about 600 logged samples of Band 10 and Band 80 (BandPassFilter start-up) do not match: the replay with a warm Source chain and a zero-state band-pass reproduces neither the size nor the timing of the early transient. Candidate causes (initial filter state, gating of the first blocks, a different start offset) were tried only coarsely and none fits.
- The exact IIR design behind the Source notch and high-pass (IPP generator internals) and whether the 150 Hz low-pass FIR is applied: the replay cannot tell it from a 4-sample change of alignment.
- Why the log records `dataAlreadyFiltered: 1` while the software filters demonstrably act (`+0x10c` flag; see "Source-side software filters").
- The Aggregator numeric replay against logged output (the code reading is below).
- The trigger path cannot yet be replayed end to end from recorded EEG; the stages from the logged amplitudes onward, and the note path from logged PeakFreq, are verified.

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
- The BandPassFilter element (`0x66dc00`) picks FIR (`0x650ed0`, mode 0) or IIR (`0x6536a0`, mode 1) from `this+0x198`; the run 4 design gives every BandPassFilter windowType Hamming and filterOrder 128.

Inferred:
- Type 5 is Butterworth and the IIR generators prewarp and use the bilinear transform; the replay builds the band-stop from an order-2 analogue Butterworth prototype (a 4th-order digital filter, two biquads) and the high-pass as a first-order bilinear Butterworth. The order-1-prototype variant fits worse (Band 80 correlation 0.73 against 0.79 on the first 1600 samples).
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
- Not matched: the first about 600 samples of both bands (see "Not resolved"). Zero-state Source filters at sample 0 make the early part worse, not better.

### PeakFrequency (decompile, matched)

Vtable `0x18bc414` (secondary `0x18bc400`); `DSPPkFreq::filter` `0x632d90`; constructor about `0x633400`-`0x633f23`; process `0x6340d0`; free `0x632c10`.

- `N = 2^fftOrder = 512` (`fftOrder` 9), `df = 500 / 512`. Bins `ftol(low / df)`..`ftol(high / df)`: Ch1, Ch2 6..14; Ch3, Ch4 7..15.
- The input is a ring of `this+0x18c` samples, newest at the end and the older part zero until filled, windowed by a Hann window (`ippsWinHann_64f_I` over ones; window mean at `this+0x68` is sum / N).
- Unscaled real FFT (flag 8), conj-pack expansion to complex, magnitude, first maximum over the bin range. Frequency = `(lowbin + idx) * df`; amplitude = `(4.0 / N) * max / mean(window)` (constant `4.0` at `0x1895398`).
- Thunks (inferred): `0x66fd90` Zero_64f, `0x66fd80` Mul_64f_I, `0x66fd70` FFTFwd_RToPack_64f, `0x66fd60` ConjPack_64fc, `0x66fd50` Magnitude_64fc, `0x66fd40` MaxIndx_64f; window selectors `0x66fda0` Hann, `0x66fdb0` Hamming, `0x66fdc0` Kaiser.
- Offline match: 1955 of 2000 samples (97.8 %) with a Hann denominator `N` (the `N - 1` and `N + 1` variants and an added high-pass were no better). The mismatches are near-ties between adjacent bins, so the true input differs slightly from the raw frames, consistent with the unlocated Source-side stage.

### Aggregator (static, partial)

`AggregatorFilter` vtable `0x18bf91c` (secondary `0x18bf90c`). Observed in code: reset on message type 3 (`0x649c70`); property setter `0x649b60` creates the aggregation object at `this+0x1e4` from the flow-style string; setup `0x64a3e0` passes `rate * [0x62]` (sample rate times `averageEpoch`) to that object's virtual `+0x18`.

- Style strings (static initialisers at `0x13f8620`, `0x13f8640`, `0x13f8660`): `0x1fbf194` = `"absolute"`, `0x1fbf3c4` = `"continuous"`, `0x1fbf178` = `"discrete"`. `"absolute"` creates the `0x18bf978` object (0x70 bytes; running sum in a double at `+0x60`, count at `+0x68`; value = sum / count, `0x649850`). `"continuous"` and `"discrete"` create the `0x6542a0` object (0x60 bytes, vtable `0x18c1a98`). The run 4 design uses `"continuous"` with `averageEpoch` 60 for all six Aggregators, so the `0x18bf978` object is not on the replayed path.
- `0x18c1a98` per-sample (virtual `+8`, `0x6543f0`): `v' = scale * v + offset` (floats at `+0x50`, `+0x54`, copied by the constructor from the owning property block at `+0x18c`, `+0x190`). When the window length at `+0x58` is above zero (set by virtual `+0x18`, `0x654510`, which also rebuilds the buffer through `+0x1c`, `0x654530`), `v'` goes through the moving average `0x654fe0`; otherwise `v'` is the output. The block entry (virtual `+4`, `0x6544d0`) just repeats the per-sample call. The output is held as a float at `+0x5c`.
- Moving average `0x654fe0` (object `0x654f20(window, 0, 1)`): circular buffer of doubles. Running sum += `x`, then -= the overwritten oldest entry (float arithmetic, stored in a double), the slot is overwritten with `x`, and the result is sum / `min(samples seen, window)`. This is a sliding boxcar mean with a partial-fill divisor, window `rate * averageEpoch` = 30000 samples at 500 Hz, matching the 60 s boxcar read earlier from the logged sum and window. Inferred, not replayed: `scale` = 1 and `offset` = 0 (not read from the design); the Evaluator-side `0x654440` (sign times log) variant is not used by this path.

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
