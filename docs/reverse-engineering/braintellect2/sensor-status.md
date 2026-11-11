# BRAINtellect2 Optimize-Tab Sensor Status

How the Optimize tab's sensor status ("Ready", "Adjust Fit", "Not Available") is
computed, and why it reads "Ready" with nothing in the sensor bar.

Sources: static analysis of `braintellect.exe` v5.1.57 (sha256 `af7ff543…2560`) with
`objdump` and Ghidra headless; the `BT-LOD-v5.1.11` flow design decoded from the
2026-11-04 session records (`session-storage.md`), whose values stay under the
repository `tmp/`; the 2026-11-04 VM sessions (exercises 125 and 126) and the operator's
report that the production tablet behaves the same way.

## Observed behavior

- With nothing in the sensor bar and the Optimize tab open before Play, the status
  reads green "Ready" (VM, exercise 126). The operator sees the same on the production
  tablet.
- During a run the VM showed "Not Available". The operator reports that the tablet
  shows nothing during a run.
- After the last exercise of a session, the status stays inactive until the app is
  relaunched.

## Observed: the status text is three overlay images

The words are baked into Qt resource images, not drawn as text: `:/head2d-offline-*`
("Not Available"), `:/head2d-red-*` ("Adjust Fit"), and `:/head2d-green-*` ("Ready"),
in large and small sizes. The Head2D sensor-display object holds the three overlay
widgets at `+0xa4` (offline), `+0xa8` (red), and `+0xac` (green), and the code shows
exactly one at a time with `QWidget::setVisible` (`0x9b16e0`).

## Observed: the BT-LOD design

The design is a four-channel spectral pipeline. There is no lead-off input.

```
Source 1 ─Out1..Out4→ FFT Ch1..Ch4 (InSignal) ─OutFrequency→ Sensors (InFrequency)
```

- Each `FFT ChN` is a discrete FFT with a Bartlett window.
- The four connections into `Sensors` name the channels `FP1`, `FP2`, `T3`, and `T4`
  (`flowEngine.property.name`). No ground channel (`G`, `G1`, `G2`) is connected.
- `Sensors` is a `flowDisplayElementType` with `sensorLogicStyle = Image overlay`,
  `LODataSource = Line noise`, `hideGraphicsWhenRunning = true`,
  `testAndSetLineNoiseFilter = true`, `deviceMinFloor = -120.0`,
  `deviceOffHeadFloor = -50`, `lineNoiseDifference = 15.5`,
  `lineNoiseChannelPairDifference = 0.5`, and `stateChangeDelay = 1000`. These are
  `type="design"` constants, not signal values. `frequencyTest` is a `type="runtime"`
  property, likely written by the 50/60 Hz line-noise test (`0x565300`), so its stored
  value is not recorded here.

The design properties reach the Head2D object through the display element's property
setter (`0x56d100`–`0x56dc30`, keyed by property id) and the Head2D constructor
(`0x565890`, called at `0x56e266`):

| Property (id) | Element field | Head2D field |
|---|---|---|
| `hideGraphicsWhenRunning` (`0x1f`) | `+0x200` (`0x56dab6`) | `+0x84` |
| `lineNoiseDifference` (`0x20`) | `+0x214` (`0x56db29`) | `+0x88` |
| `lineNoiseChannelPairDifference` (`0x21`) | `+0x218` (`0x56dba3`) | `+0x8c` |
| `stateChangeDelay` (`0x22`, ms) | `+0x21c` (`0x56dc24`) | `+0x90` |

Binary defaults when the design omits them: `hideGraphicsWhenRunning = true` and
`stateChangeDelay = 1000` (`0x56c7ea`, `0x56c7f7`).

## Observed: the path from the B-2 to the design input

1. `processData_v5` decodes each channel's 24-bit sample (`0x556c46`), converts it to
   volts through the channel processor (`[ebx+0x2c0]` vtable `+0x18`, `0x556c7b`), and
   forwards it to the engine queue (`0x5773a0`, called at `0x556c9e`). That is the
   design's `Source 1`.
2. `processData_v5` has no lead-off decode. Byte `[+0]` of each channel group is not
   read. The branch at `0x556c2c` that `b2-protocol.md` once called a lead-off channel
   is placebo mode: when `[ebx+0x1ec]` equals `1` (`0x188e204`), the sample is
   replaced by `0x3fff - rand()` (`MSVCR90!rand` through `0x14c5a3c`, at `0x556c34`).
   `OptimizeControls::launchExercise` sets that field from a string compare with
   `"placebo"` (`0x440574`–`0x44059b`).
3. Each FFT feeds a Head2D channel object (vtable `0x1898154`). Its update
   (`0x560f40`) takes `20·log10` of the spectrum bin at the line-noise test frequency
   (`+0x68`) and of five band bins. Each value passes through a smoothing scaler
   (`0x6543f0`), which stores the result at `+0x5c`, read through vtable `+0x10`. The
   band levels land at `+0x264`…`+0x274`. A bin that is exactly `0.0` skips the log, so
   the level keeps its previous or default value.
4. `receiveMessage` (`0x565300`) runs the 50/60 Hz test and writes the chosen
   line-noise frequency into every channel's `+0x68` (`0x564270`).

## Observed: the decision ("Image overlay" style)

The dispatcher `0x569c60` runs on each display update:

1. If the engine-running flag `+0x99` is false and the style is "Image overlay", it
   shows the offline overlay ("Not Available") and returns (`0x569ca2`–`0x569ce9`).
2. If `hideGraphicsWhenRunning` (`+0x84`) is set and `+0x99` is false, it returns
   without drawing (`0x569d02`–`0x569d1f`). `+0x9a` is always `1`, and its only writer
   is the constructor (`0x5659ef`).
3. With `LODataSource = Line noise` (`0x569ff6`), it scans every channel's line-noise
   level `m` for the minimum, seeded with `0.0`, and the maximum, seeded with `-200.0`
   (`0x56a00c`–`0x56a0b8`). It then calls the Image-overlay evaluator `0x568d30`
   (`0x56a0ed`).

The evaluator `0x568d30` sets a fault flag if any of three tests fails:

- **Outlier channel**: `|min − m| > lineNoiseDifference` for any channel
  (`0x568e05`–`0x568e52`).
- **Level ceiling**: `m` exceeds the ground item's `errorThreshold` (`+0xc0` `+0x64`,
  set by `errorThresholdChanged` messages at `0x566487`). With no ground item the
  ceiling stays `0.0` (`0x568d39`–`0x568d51`). The BT-LOD design connects no ground
  channel; the worn replays show the ceiling at -10 dB ("Observed: worn replays").
- **Bridged pair**: for `FP1`/`FP2` (`+0xb0`/`+0xb4`) and `T3`/`T4` (`+0xb8`/`+0xbc`),
  `0x565cb0` counts the band levels where `|a − b| < lineNoiseChannelPairDifference`
  and reports a fault when the count is at least half of them (`0.5` at `0x18943f0`).
  The test runs only when `lineNoiseChannelPairDifference > -1.0` (`0x568e8d`). The
  channel names come from the static strings at `0x1f79594`, `0x1f7a350`, `0x1f7a388`,
  and `0x1f7a7fc`.

Debounce (`0x568ee2`–`0x568f27`): the first faulted update starts a timer at `+0x13c`
and sets `+0x138`. A clean update clears `+0x138`. Once the fault has lasted at least
`stateChangeDelay` ms, the red overlay "Adjust Fit" is shown (`0x568f29`). Every other
case shows the green overlay "Ready" (`0x568f59`).

## Observed: engine lifecycle

`OptimizeControls::setLODEngineState(Engine::State, bool running)` (`0x43bee0`) stores
the state in the engine (`0x5410d0` → `+0x80`), then emits an `OptimizeControls`
signal carrying `running` (`0x453900`). Head2D's slot 0 (`0x572c72`) is
`LODEngineRunningSlot(bool)` (`0x565f00`). It writes `running` into `+0x99` and shows or
hides the per-channel sensor widgets (`0x560ce0`, `0x5628e0`).

| Caller | Call | When |
|---|---|---|
| `setTabVisible(true)` | `(4, true)` at `0x43c202` | Optimize tab shown, engine exists, and `+0x1e0 != 2` |
| `setTabVisible(false)` | `(3, false)` at `0x43c252` | Optimize tab hidden |
| `playSlot` | `(3, false)` at `0x43d395` | Play pressed |
| `timerEvent` | `(3, false)` at `0x43d1f1` | LOD-processing timer stop ("attempting to stop LOD processing") |
| `pauseSlot` | `(4, true)` at `0x43d7d5` | Pause ("starting LOD engine") |
| `startLODEngine` | `(4, true)` at `0x441153` | Engine started while the tab is visible |
| `nextExerciseSlot` | `(3, false)` at `0x4423cf` | Last exercise done; first sets `+0x1e0 = 2` (`0x4423c1`) |

Only `setupSessionSlot` resets `+0x1e0`, to `1` at `0x447157`. The constructor zeroes
it at `0x444a8a`.

## Inferred: why the status reads as it does

- **"Not Available" during a run** follows from the code: Play calls
  `setLODEngineState(3, false)`, which clears `+0x99`, and the dispatcher's first test
  then shows the offline overlay. The tablet's blank status is consistent with step 2,
  or with the sensor widgets that `LODEngineRunningSlot(false)` hides. Which of the two
  the tablet hits was not observed.
- **Inactive after the last exercise**: `nextExerciseSlot` sets `+0x1e0 = 2`, so
  `setTabVisible(true)` no longer restarts the engine until `setupSessionSlot` runs
  again.
- **"Ready" with an empty sensor bar**: "Ready" means that none of the three fault
  tests failed. None of them checks that a sensor touches skin. They flag one channel
  that differs from the others, a level above a ceiling, or two paired channels that
  look identical. Floating inputs pick up similar, low, uncorrelated noise. Every
  channel then sits near the minimum, below the ceiling, and the pairs differ, so no
  fault is raised and the overlay goes green (sensors-off replay, and "Observed: worn
  replays" below for a wearer). The T3 and T4 inputs shorted together read "Adjust Fit"
  in every block ("Observed: worn replays"; an FP1-FP2 short was not captured). Identical
  default levels (all zero) raise the pair test, which is the pre-block "Adjust Fit" below.
- `Engine::State` values `3` and `4` are read as paused/stopped and running from their
  callers. The enum names were not recovered.

## Observed: band objects and floors (static, `0x560f40`, `0x562400`, `0x568db5`)

- Each channel has five band levels, `20 * log10(bin)` of the spectrum bin nearest the frequency (a bin of exactly 0.0 keeps the previous level): 50 Hz and 60 Hz (mains), and 12, 40 and 70 Hz.
- The line-noise level `m` is the mains level. The pair test compares the vector [70, 40, 12] Hz of each channel with its partner (order does not affect the count).
- Head2D `+0x94` is `max(mean 50 Hz level, mean 60 Hz level)` over the channels (`0x562400`); the dispatcher compares it with the ground item's thresholds.
- `deviceMinFloor` and `deviceOffHeadFloor` are stored at Head2D `+0x44` / `+0x48` (moderate confidence) and logged. They are read only at `0x569e61`, on the path for a data source other than "Line noise" (red when a level is below `+0x48`). They are unused for BT-LOD "Line noise".
- The clone feeds this through `SensorMonitor` (`src/sensor.rs`): blocks of 500 samples per channel padded with 12 zeros to the 512-point Bartlett-windowed FFT, these five levels, then the evaluator on a display tick. The scale and bin rule are in the next section.

## Observed: levels against the original (2026-11-07 sensors-off replay)

Run 8's no-wearer stream was replayed into `braintellect.exe` under `frida-inject`
(`replay-rig.md`), with hooks on the FFT (`DSPFFT::process`), the Head2D update, the
evaluator exit and the pair test, over the first 78 blocks per channel. The clone reproduces the
evaluator's fault flag, its Head2D levels and the 256-bin spectrum of every channel exactly,
to the bit, in each of the 11 committed blocks (blocks 0 to 3, 15 to 17, 20, 31, 58 and 77),
all asserted by `tests/parity_sensor_status.rs` (fixtures `sensor_inputs.tsv`,
`sensor_spectra.tsv` and `sensor_levels.tsv`; see "Level bit exactness" below). The
"state sequence" held is the evaluator's per-block fault flag; the overlay state itself was
not observed:

- FFT: Bartlett window (order 9), 512-point blocks of 500 samples plus 12 zeros (the
  committed `sensor_inputs.tsv` rows are 512 long with the last 12 zero), output scale `8/511` of the mean-
  normalized magnitude (the clone's `1/128 / mean(window)`); level = `20 log10(bin / 0.04)`,
  that is the bin plus 27.9588 dB.
- A band reads the bin `trunc(freq / 0.9766)`: 51 (50 Hz), 61 (60 Hz), 12 (12 Hz), 40
  (40 Hz), 71 (70 Hz).
- The line-noise level is the -120 dB floor for the first two blocks (8 channel updates),
  then the 60 Hz level. Head2D picks 60 Hz when the channel-mean 60 Hz level exceeds the
  70 Hz level.
- The pair test faulted T3/T4 on blocks 16, 20 and 31 and FP1/FP2 on blocks 58 and 77;
  the outlier and ceiling tests never fired.

The overlay state was not observed on that replay (the `setVisible` hook never fired; see
`replay-rig.md`). The 2026-11-08 replay of the same stream below observes it. Not observed:
the 50 Hz branch of the line rule and its operand order (decompile only; the captured stream
picks 60 Hz).

## Level bit exactness (2026-11-07, static and emulated, no VM)

Checked against the same capture (FFT input and output per call, the window, the Head2D
spectrum and levels):

- Level path (Head2D `0x560f40`, x87 at 53-bit precision): `x = f32(bin / f32(0.04))`,
  `y = f32(log10(x))` (CRT `log10`), `level = f32(20.0 * y)`, then the identity scaler
  `0x6543f0` (a = 1, b = 0, no filter). From the original's own spectrum this reproduces all
  2100 captured levels bit exact. A zero bin skips the log and reuses the last level computed
  in the same call (initially `+0x70`, -120); the clone keeps the band's own previous level
  instead.
- Window: IPP Bartlett (`0x66fea0`, variant-1 kernel `0x6b53f0`) accumulates four SSE lanes:
  `s = f32(2/(n-1))`, lane j starts at `f32(j*s)` and adds `f32(4*s)` per step, front half
  mirrored. This reproduces the captured window bit exact (the clone's `DspFft` builds it so;
  an f64 `1 - |2i/(n-1) - 1|` rounded to f32 differs in 96 entries). Its mean (`+0x50`) is the sequential f32 sum / 512,
  0.499022156, which the clone matches.
- `DSPFFT::process` (`0x637940`): window multiply (`0x66ff30`), IPP order-9 real FFT
  (`0x66ff20`, the Coherence spec), `0x66ff10`, magnitude (`0x66ff00`), then x87
  `f32((mag * f32(+0x54 * scale)) / +0x50)` with `+0x54 * scale` = 1/128.
- The earlier mismatch (about 40% of bins off by 1 to 3 f32 ulp) had two causes, found by
  a stage capture on the rig (2026-11-07, hooks on `0x66ff20`, `0x66ff10` and `0x66ff00`
  inside `DSPFFT::process`, 388 calls):
  - The captured "input" (tag 30) is the raw block, not what the FFT reads: the app windows
    a copy, and `f32(raw * window)` equals the FFT's input in all 388 calls. Fed that, the
    clone's `RealFft512` reproduces the FFT output bit exact (41,120 of 41,120 words over 80
    calls); `0x66ff10` leaves it unchanged.
  - The magnitude (`0x66ff00`, kernel `0x6b8650`) is not a square root. It forms
    `s = re^2 + im^2` in f32, takes the hardware `rsqrtps` estimate `r`, and refines it once:
    `q = s * r`, `mag = ((3 - r * q) * 0.5) * q`. The estimate is only 12 bits, so values sit
    1 to 3 ulp from the correctly rounded root (8,090 of 20,560 differ from `sqrt`). Intel's
    `rsqrtps` depends only on the exponent's parity and the top 11 mantissa bits; the clone
    carries that 4096-entry table (`src/feedback/rsqrt_table.txt`, read from an Intel Core
    i3-10100T, the rig host) and reproduces all 20,560 captured magnitudes. Unicorn's `rsqrtps`
    is exact, which is why its emulation disagreed. AMD's `rsqrtps` differs; not checked.
  - The final scale from the captured magnitude reproduces all 20,480 final bins.
  The clone's `DspFft` (`src/feedback/dsp_fft.rs`) is this chain, and its window is the IPP
  recipe above; `FftSpectrum` uses it too.

## Observed: worn replays (2026-11-08 and 2026-11-10, #61)

Three captures from a worn B-2 ("good fit" and "T4 lifted" on 2026-11-08, "T3-T4 shorted" on
2026-11-10; 89 blocks each) were replayed into the original through the emulator
(`replay-rig.md`) with hooks on the consumed samples, the Head2D level update and the overlay
sites. The state is the overlay shown when each block's interval ended; levels are the Head2D
line-noise, 12, 40 and 70 Hz levels. The clone matches the original in every block, levels bit
exact and state equal, on all three captures over the frames the original consumed
(`tests/replay_s61.rs`, ignored, inputs outside Git).

- Good fit: "Adjust Fit" appears 14 times, with the fault first seen at blocks 9, 24, 28, 31,
  34, 40, 44, 50, 55, 63, 67, 74, 79 and 87. Every one is the T3/T4 pair test (2 of 3 bands
  within 0.5 dB); the outlier and ceiling tests do not fire. The 1000 ms debounce holds the
  red overlay into the following blocks. Otherwise "Ready".
- T4 lifted: "Ready" throughout. T4 sits about 1.5 dB above the minimum, inside the 15.5 dB
  outlier limit, and no pair test fires. A lifted sensor does not raise "Adjust Fit" here.
- T3-T4 shorted: "Adjust Fit" in all 89 blocks. Only the pair test can fault there: in every
  block the four line-noise levels sit within 5.1 dB of each other (from block 2 all between
  -33.9 and -27.8 dB; blocks 0 and 1 are at the -120 dB floor), inside the 15.5 dB outlier
  limit and below the -10 dB ceiling. T3/T4 meets the pair test in all 89 blocks (at least 2
  of its 12, 40 and 70 Hz levels within 0.5 dB). FP1/FP2 also meets it in 58 of the 89 blocks
  and in none of the good fit's; which pair raised each fault was not logged. Median
  difference over the three bands and all 89 blocks: T3/T4 0.10 dB (good fit 0.45 dB), FP1/FP2
  0.39 dB (good fit 9.07 dB). Every channel's median 12 Hz level is 13.6 to 27.6 dB above the
  good fit's.
- Block: the FFT input is 500 consecutive samples plus 12 zeros, not 512 samples.
- Source filters: the LOD Source runs with the 45 Hz low-pass off and the 25 Hz band-stop on.
- Ceiling: the ground ceiling is -10 dB.
- Debounce: driven by the display timer, about 60 ms a tick, not by block completion. Before
  block 0 the default (all zero) levels fail the pair test, "Adjust Fit" shows at about
  1031 ms, and "Ready" shows at block 0.
- The original did not consume a few received frames (3 in good fit, 4 in T4 lifted, 24 in
  T3-T4 shorted; observed). The cause is inferred as host timing: the streams show no counter
  gap or failed validation there, so they cannot be found from the bytes; the first level
  difference of a naive replay is exactly the first skipped frame.
  The replay leaves them out, as `replay_live` does. Not reproducible by the clone.

## Observed: run 8 no-wearer overlay state (2026-11-08, #57)

Run 8's no-wearer stream was replayed again with the #61 hook set, 97 blocks logged over the
Optimize-tab period (Play not pressed; the engine runs, the overlay is never "Not Available"
after block 0). The clone matches the original's Head2D levels bit exact in all 97 blocks
(line noise, 12, 40 and 70 Hz of the four channels) and its overlay state in every block
but two, over the frames the original consumed (14 skipped; `tests/replay_s61.rs`, ignored,
inputs outside Git).

- State: "Ready" from block 0, except "Adjust Fit" at blocks 16 and 20. Before block 0 the
  default levels fail the pair test and "Adjust Fit" shows, as in the worn replays.
- The pair test faulted T3/T4 on blocks 16, 20 and 31, each for one block. The 1000 ms debounce
  needs 16 display ticks (62.5 ms each, the 60 ms timer rounded to the Windows clock), exactly
  one block interval, so the delay is reached at the tick at which the next block's update
  clears the fault. Observed: at blocks 16 and 20 the tick came first and showed the red
  overlay; at block 31 the update came first and the overlay stayed green.
- Inferred: which comes first is a race between the display timer and the engine thread, set
  by sub-tick host timing, so the clone cannot reproduce it from the bytes (as for the skipped
  frames). The test leaves blocks 16 and 20 (the delay reached in the clock reading of the next
  update; `ties.txt` of `worn-sensor-extract.py`) out of the state comparison. The clone shows
  "Ready" there. Only the tick-first outcome is detectable from the log (the tick logs the
  delay reached); block 31, the update-first outcome, is compared and matches.

## Unresolved

- Whether a ground item sets an `errorThreshold` in the design (the -10 dB ceiling is
  observed, its source is not).
- Whether the LOD Source's low-pass and high-pass cutoffs follow the session's: the worn
  replays ran only with the defaults (150 Hz, 1.5 Hz), and the clone takes the session's.
- The 50 Hz branch and the operand order of the 50/60 Hz rule (`0x565300`).
- The source string compared with `"placebo"` in `launchExercise` (`0x4e4410`).
