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
  channel.
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
  channel then sits near the minimum, below a `0.0` dB ceiling, and the pairs differ,
  so no fault is raised and the overlay goes green. This was not tested against
  values. It predicts "Adjust Fit" when one sensor is off while the others are on, or
  when a pair's inputs are shorted together or read exactly zero (identical default
  levels).
- `Engine::State` values `3` and `4` are read as paused/stopped and running from their
  callers. The enum names were not recovered.

## Observed: band objects and floors (static, `0x560f40`, `0x562400`, `0x568db5`)

- Each channel has five band levels, `20 * log10(bin)` of the spectrum bin nearest the frequency (a bin of exactly 0.0 keeps the previous level): 50 Hz and 60 Hz (mains), and 12, 40 and 70 Hz.
- The line-noise level `m` is the mains level. The pair test compares the vector [70, 40, 12] Hz of each channel with its partner (order does not affect the count).
- Head2D `+0x94` is `max(mean 50 Hz level, mean 60 Hz level)` over the channels (`0x562400`); the dispatcher compares it with the ground item's thresholds.
- `deviceMinFloor` and `deviceOffHeadFloor` are stored at Head2D `+0x44` / `+0x48` (moderate confidence) and logged. They are read only at `0x569e61`, on the path for a data source other than "Line noise" (red when a level is below `+0x48`). They are unused for BT-LOD "Line noise".
- The clone feeds this through `SensorMonitor` (`src/sensor.rs`): 512-sample blocks per channel, Bartlett-windowed FFT, these five levels, then the evaluator. The scale and bin rule are in the next section.

## Observed: levels against the original (2026-11-07 sensors-off replay)

Run 8's no-wearer stream was replayed into `braintellect.exe` under `frida-inject`
(`replay-rig.md`), with hooks on the FFT (`DSPFFT::process`), the Head2D update, the
evaluator exit and the pair test, over the first 78 blocks per channel. The clone reproduces every
level in the 11 committed blocks (blocks 0 to 3, 15 to 17, 20, 31, 58 and 77) within 1e-3 dB and the evaluator's fault flag in each
(`tests/parity_sensor_status.rs`, fixtures `sensor_inputs.tsv` / `sensor_levels.tsv`):

- FFT: Bartlett window (order 9), 512-sample blocks, output scale `8/511` of the mean-
  normalized magnitude (the clone's `1/128 / mean(window)`); level = `20 log10(bin / 0.04)`,
  that is the bin plus 27.9588 dB.
- A band reads the bin `trunc(freq / 0.9766)`: 51 (50 Hz), 61 (60 Hz), 12 (12 Hz), 40
  (40 Hz), 71 (70 Hz).
- The line-noise level is the -120 dB floor for the first two blocks (8 channel updates),
  then the 60 Hz level. Head2D picks 60 Hz when the channel-mean 60 Hz level exceeds the
  70 Hz level.
- The pair test faulted T3/T4 on blocks 16, 20 and 31 and FP1/FP2 on blocks 58 and 77;
  the outlier and ceiling tests never fired.

Not observed: the overlay state itself (the `setVisible` hook did not fire), so the
`stateChangeDelay` timing is checked on synthetic levels only; the 50 Hz branch of the
line rule and its operand order (decompile only; the captured stream picks 60 Hz).

## Unresolved

- The overlay state and its debounce timing on a captured stream, and whether a ground
  item sets an `errorThreshold`.
- The 50 Hz branch and the operand order of the 50/60 Hz rule (`0x565300`).
- The source string compared with `"placebo"` in `launchExercise` (`0x4e4410`).
