# Replaying a recorded B-2 stream into the original

Ticket #48. The clone's `braintellect-linux emulate <capture> <socket>` plays a B-2 over a
QEMU virtual serial port so the original `braintellect.exe` (in the `braintellect-offline`
VM) consumes a recorded stream with no real B-2, no Bluetooth and no wearer. This page holds
the rig, the run steps and the result of replaying run 8.

## What the emulator does

- Reads a raw RX capture (the bytes the app received: reply blocks and stream frames).
- Answers `!*#101` with the capture's serial reply and `!*#110/112/131` with the captured
  replies byte for byte (each reply is the 56-byte block, marker `AC BC CC 03`, echoed code at
  `[8]`); `!*#132..138` get the capture's replies for the same codes.
- Streams the capture's frames from Start (`!*#48,1,...`) at 250 frames/s (500 Hz, two
  samples per channel per frame), and stops on Stop (`!*#48,0,...`).
- Sends nothing for power-off (`!*#48,0,1,0,0,0,0,0`), as the B-2 sleeps.
- The clone's live session now records its raw RX to `<data dir>/captures/b2-<secs>.rx`
  (`src/capture.rs`), the same format the emulator reads.
- Each command read from the app is logged to stderr as `<epoch ms> app> <bytes>` (the
  stream reader splits lines, so a command may appear in fragments).

## VM wiring (before and after)

Changed only in the host quadlet `~/.config/containers/systemd/braintellect-offline.container`
(on `neal@10.10.10.10`); the rig version is kept beside it as
`braintellect-offline.container.rig-s48`. Neither passes the Bluetooth adapter through: the
server's BlueZ is its only owner (`device-connection.md`, #38). Both publish noVNC on 8017
(8007 is bound by another service). The rig differs from the live quadlet only in:

```
-Environment="ARGUMENTS=-audiodev alsa,id=snd0,out.dev=bstout,out.frequency=44100 -device ich9-intel-hda -device hda-output,audiodev=snd0"
+Environment="ARGUMENTS=-audiodev none,id=snd0 -device ich9-intel-hda -device hda-output,audiodev=snd0 -chardev socket,id=b2,path=/b2/b2.sock,server=on,wait=off -device isa-serial,chardev=b2,iobase=0x2f8,irq=3"
+Volume=/srv/local/windows/braintellect-offline/b2:/b2
```

To run the rig, copy the live file to `braintellect-offline.container.pre-rig`, copy `rig-s48`
over the live file, `systemctl --user daemon-reload` and restart the VM; afterwards move
`pre-rig` back. The serial port is guest COM2 (16550 at 0x2f8).
Guest side: `C:\ProgramData\BST\BRAINtellect2\preferences\braintellect.ini` has
`B-2-BTCOMPort=COM2`; the prior file is `braintellect.ini.bak-s48` beside it
(`scripts/s48-prep.cmd`). The original is not patched beyond the existing host-registration
bypass.

Files reach the guest with hot-added `usb-storage` raw images (the NETWORK=N guest has no
share): partitioned MBR, FAT32 (`mkfs.fat -F 32 --offset 2048`), loop-mounted on the host at
`offset=1048576`. An image labelled CAP holds `frida-inject.exe` and `hook11.js`.

## Run steps

0. Turbo on: `echo 0 | sudo tee /sys/devices/system/cpu/intel_pstate/no_turbo` on the host
   (see "Host CPU clock" below); after the run, `sudo systemctl start cpu-turbo-schedule.service`
   puts the schedule's state back.
1. Forward the serial socket to where the emulator runs:
   `ssh -fN -L <local>.sock:/srv/local/windows/braintellect-offline/b2/b2.sock neal@10.10.10.10`.
2. Start the emulator first: `braintellect-linux emulate <rx capture> <local>.sock 2> emu.log`.
3. In the guest run `scripts/s48-run.cmd` (the app under `frida-inject -f` with the run-8 hook
   set `hook11.js`, which writes `C:\cap\run8.bin`; `C:\bsttest\lib` must be on `PATH`). Choose
   user "guest", accept the two EULA pages.
4. The app connects and sends Start immediately; note the Start time in `emu.log`.
5. Optimize tab: gear, OK (fixes the 4-minute Piano session and enables Play), then Play at
   Start + 51.2 s (run 8 pressed Play 12800 frames after Start).
6. After 240 s the app prints "You have completed all exercises". Kill the app and injector
   with `taskkill /F`; never exit it gracefully (that sends power-off).
7. Copy `C:\cap\run8.bin` out through the CAP image and reduce it with
   `python3 -I apps/braintellect-linux/scripts/replay-extract.py <run8.bin> [--out DIR]` (built
   on the run-8 parsers in `make-fixtures.py`). It keeps the complete logged block prefix (up to
   the first block without a clock read, at most 30001) and prints `blocks <n> of 30001`.

## Result: run 8 replayed into the original (2026-11-06)

Not a reproduction. The original's decisions differ from the run-8 baseline from the first
fire on. Nothing was tuned.

| Quantity | Run 8 baseline | Replay |
|---|---|---|
| Consumed ch0 samples before block 0 (graph start) | 25600 | 25952 (352 samples, 0.70 s later) |
| Graph blocks logged | 30001 | 30001 |
| Counter fires (Trigger/Note pairs) | 415 | 437 |
| First fire (clock ms, block, note) | 79, 52, 41 | 67, 53, 40 |
| Second fire | 579, 145, 60 | 576, 149, 43 |
| Evaluator code (last record) | 111 from block 0, 12 from 14999, 82 from 22499 | 111 from block 0, 183 from 14999, no later change |
| Clock at block 30000 (ms) | 240001 | 239371 |
| 16-tap-mean flag rows differing | | 29882 of 30001 |

- Fire spacing is the same ~500 ms cadence early on, but block, note and time already differ
  at fire 0; the note sequences do not match.
- The graph started 352 consumed samples later than run 8 although Play was pressed at
  Start + 51.202 s (run 8's offset): the original takes about 0.7 s from Play to the first graph
  block. A different start index shifts the Source chain's warm-up (the Source chain is warm at
  the graph start, see `audio-feedback.md`), so an exact match is not expected from a shifted
  start; this run cannot separate that effect from the original being non-deterministic against
  the recorded stream (wall-clock counter reads). It was not retried at other offsets.
- At block 14999 the Evaluator code switches in both runs, but to 12 in the baseline and to 183
  in the replay. The parsers reproduce the baseline when run on the original `run8.bin`
  (same transitions, 415 fires), so the difference comes from the replayed run.

## Result: the clone's decisions on the replayed stream (2026-11-06)

The target is the clone's decisions on this replay, not run 8's: the original's graph started
at a different sample, so it saw different EEG. `tests/replay_s48.rs` (ignored; inputs outside
Git) feeds the clone the replayed consumed EEG from its first sample (Source warm-up over the
whole stream before the graph start, 25640 samples, the low-pass 45 Hz switched in cold at
graph start + 4) with the original's
logged Evaluator codes, counter clock reads and 16-tap-mean flags, as
`tests/parity_smart2_exercise.rs` does for run 8, and diffs the Trigger/Note decisions.
`scripts/replay-extract.py <run8.bin>` reduces the replay hook log to the files the test
reads (default `tmp/s48/replay/`): the complete logged block prefix, with the 16-tap-mean flags
of all 18 graph-call objects by address rank. Run
`cargo test --release --test replay_s48 -- --ignored --nocapture` (about 1 min;
`S48_REPLAY_DIR` points it at another extract).

- Logged consumed volts before block 0: 25952. As run 8's 25600 was (count 25912), the graph
  start is located from the original's own logged E..H amplitudes: 25640 is the best start in
  count-1900..count+400.
- Decisions: 874 original (Trigger/Note) against 874 clone; all 874 match in kind, value and
  clock time, and all 30001 blocks of logged E..H amplitudes are bit exact. Two inputs the
  clone could not derive caused the earlier single difference (first Note, block 67: original
  40, clone 42); both are recorded in `audio-feedback.md` ("Replay start-up"): the 45 Hz
  low-pass switch landed at graph start + 4 (run 8: + 1), and the band to band-pass object
  mapping of run 8 does not carry over to another process.
- `replay_s48` locates the start (25640), the switch (25644) and the E..H object ranks
  (7, 3, 1, 5) from the logged E..H amplitudes; all three are unlogged inputs. Start and switch
  are searched under run 8's ranks (a wrong rank costs at most one `float32` ulp on scalar-head
  samples); then each band's rank is the one rank of 18 whose replay reproduces every logged
  amplitude of blocks 0..299, and the test fails if none or several do. A..D are not logged, so
  run 8's ranks (14, 3, 2, 8) stand in for them.

## Why rx8.raw holds 639k frames and `audio-feedback.md` says 149k

`rx8.raw` was extracted from the whole `run8.bin` hook log: 639,284 frames parsed, 639,306
markers, no duplicate frames and no counter breaks, so the emulator does not serve duplicates
or wrong frames. The 149,122 frames (272,793 consumed samples) in `audio-feedback.md` come
from an earlier or shorter log of the same run. The file spans about 42.6 min, of which about
37 min is streaming after the exercise. The replay received 95,676 frames in about 380 s, so
the emulator streams at the right rate (250 frames/s).

## Result: #33, the clone's live capture replayed into the original (2026-11-06)

A 240 s live session through the clone's UI on the server against the real B-2 recorded
59,974 frames (239.9 s, 0 counter and 0 validation errors, 419 notes). That capture was
replayed into the original with this rig and the clone was run on the original's consumed
EEG as `replay_s48` does (`S48_REPLAY_DIR=<extract> cargo test --release --test replay_s48
-- --ignored --nocapture`).

- Play at Start + 31.17 s; 15956 consumed samples before block 0; graph start located at
  15616, the 45 Hz low-pass switch at 15620 (start + 4, as in the run-8 replay).
- The original logged 26022 blocks (about 208 s) and then stalled when the capture ran out:
  a live capture is the exercise's length, and the 240 s exercise needs 120,004 samples after
  the graph start, so a replay never reaches "You have completed all exercises". Kill it once
  the stream stops.
- Decisions: 734 original (367 Trigger/Note pairs) against 734 clone; all match in kind,
  value and clock time. All 26022 blocks of E..H amplitudes are bit exact.
- The E..H band-pass object ranks in this process were (6, 2, 4, 0), not the run-8 replay's
  (7, 3, 1, 5): the rank mapping differs per process, and `replay_s48` locates it per run
  (each band matched 1200/1200 of blocks 0..299 at exactly one rank). With the wrong ranks the
  decisions still matched but amplitudes differed by 1 ulp from block 2.
- Acquisition: the clone's frame parse and µV decode (`live::stream_config`, family 12)
  against the original's logged consumed volts (hook tag 12 logs channel 0 only): 119,704
  ch0 samples, max difference 0 µV. The first logged sample is slot 0 of the first stream
  frame.
- The original consumed fewer frames than it received: 35 gaps totalling 124 frames (248 ch0
  samples), on frame boundaries, the first at stream frames 9138..9143 and most in
  50338..52503. Its own RX log (tag 1) holds all 59,978 frames byte-identical to the capture,
  so the drop is inside the app; the cause is not established. The clone consumes every frame.
  The decision diff is unaffected because `replay_s48` feeds the original's consumed samples,
  but two apps reading one raw stream live would diverge at the first gap.

## Result: the Evaluator inputs and bands A..D captured (#53, 2026-11-07)

A replay of the run-8 rx stream with hooks on the Evaluator (each Balance and Coherence series value per block, the chosen code) and on bands A..D was captured on the third attempt; the findings are in `audio-feedback.md`, "Replay capture of the Evaluator inputs".

- Static (observed, Ghidra `analyzeHeadless` over a copy of the `bt2` project): the series push
  (`0x63d9c0`), the series evaluation (`0x63e540`), the Evaluator decision (`0x63eb70`, code at
  object offset `+0x274`) and the Evaluator code setter (`0x63ea70`). A hook set for them,
  `hook12.js` (`hook11.js` plus those four), was written and loaded under `frida-inject -f`
  with the app connected to the emulator and idle on the Optimize tab.
- Driving the guest GUI to Play (observed): HMP `mouse_move`, RFB pointer events and keystrokes do not reach the QEMU USB tablet's clickable windows, so an in-guest PowerShell clicker (`EnumWindows` title waits,
  `SetCursorPos` plus `mouse_event`) started from the guest's Startup folder, with the files
  placed on the stopped guest's NTFS disk (`ntfs-3g` loop mount at offset 269484032; after a
  forced stop it needs `ntfsfix -d` on the loop device first). It reached user select, the
  licence wizard, the gear dialog, OK and Play unattended. The app's title has two spaces
  (`BRAINtellect 2  Version 5.1.57`), so a single-space `-like` pattern waits out its timeout.
- The guest's slowness was the host's night turbo cap ("Host CPU clock" below). With turbo on, the capture ran in real time: 30004 blocks, `hook12.js` loaded, no Evaluator record dropped.
- The log (`g-s53.bin`, 341 MB) stays in the main checkout's `tmp/s53/r3/`. `replay-extract.py`
  turns it into the replay directory; `replay_s48` reads it through `S48_REPLAY_DIR` and
  asserts the A..H amplitudes, the Balance values and the per-block codes.

## Result: all four channels' consumed volts (#55, 2026-11-07)

Run 8's rx stream replayed with `hook13.js` (`hook11.js` plus tag 13: the converted volts of
channels 1 to 3 at the same site as tag 12's channel 0), host at 3.8 GHz.

- Launch (observed): no gear, OK or Play is needed, as the app consumes the stream from Start.
  A Startup-folder `.cmd` started the app under `frida-inject -f` and a clicker that clicked user
  select and both EULA pages, waited 150 s on the main window and ran `taskkill /F`. The log was
  then copied off the stopped guest's NTFS disk, with no output image.
- Logged: 73,792 consumed samples on each of the four channels, 0 records dropped.
  `scripts/consumed-extract.py <s55.bin>` reduces the log to `tmp/s55/replay/`, and the ignored
  `tests/replay_s55.rs` decodes the consumed frames with the clone. All four channels match bit
  for bit over the whole range. The fixtures `consumed_stream.txt` and `consumed_volts.tsv`
  (`make-fixtures.py --src55`) hold the first 150 frames, and `tests/parity_acquisition.rs`
  asserts them.
- The original again skipped received frames, as in #33: 11 gaps totalling 26 frames among the
  first 36,922, the first after consumed frame 1149. The clone counts each gap as a packet
  counter error.
- The log (`s55.bin`, 80 MB) stays in the main checkout's `tmp/s55/`.

## Result: the stored session against the consumed volts (#56, 2026-11-07)

Run 8's rx stream replayed with a hook file (`hook13.js` plus the aggregator emit 0x649580 and the
series push 0x6497b0/0x649820/0x649840) at 3.8 GHz, then gear, OK and Play through the #53
clicker and a 310 s wait so the exercise stored (exercise 146). The clicker waited on the title
`BRAINtellect 2  Version*` (two spaces); a one-space pattern costs three minutes of timeout.

- Logged: 262,516 consumed samples per channel from the stream start (tag 12 and 13, none
  dropped), and 49 emits for each of 64 aggregators. The push and reset hooks never fired.
- The stored Band-10 and Band-80 series are reproduced within `RECORD_EPSILON` from the consumed
  volts once the Source output is truncated to whole counts, as stored; see `session-storage.md`,
  "Stored series (#56)".
- The datadir was copied off the stopped guest's NTFS disk after `ntfsfix -d`; a throwaway
  `mysql:5.7` container read it. The log (`s56.bin`, 350 MB) stays in the main checkout's
  `tmp/s56/`.

## Result: the Circle-Balance report's chart inputs (#62, 2026-11-07)

The original's report over the stored no-wearer exercise 146, hooked at `0x5c4550`
(`CircleGraphChart` input add: `this`, `float* values`, `count`).

- Trigger: Report tab, double-click the session row. RFB and HMP input do not reach the guest, so
  an in-guest PowerShell clicker (`SetCursorPos` + `mouse_event`) drives it. The host sends the
  guest commands by hot-adding a small FAT usb-storage image (HMP `drive_add`, `device_add
  usb-storage`, label `CMD`) that a guest loop polls. Frida injects with `frida-inject -f`.
- Logged: 8 calls, each one value at scale 1. The bits are in
  `apps/braintellect-linux/tests/fixtures/circle_balance_chart.tsv`; the raw log (`s62-report.bin`)
  stays in the main checkout's `tmp/s62/`.
- The exercise's files were placed on the guest NTFS disk with `ntfs-3g` (loop mount at the
  partition offset, `ntfsfix -d` after a forced stop). The rig was restored to its `.pre-rig`
  state and the lock released.

## Result: the BAGSD discrete FFT against the consumed volts (#59, 2026-11-07)

The #56 run with a `DSPFFT::process` hook added (rig tags 30 and 31: the object, the input at
entry and return, the output, and each new window buffer; `scripts/fft-extract.py` reduces the
log). Exercise 147: 262,116 consumed samples per channel, stop 240040 ms, 120,020 graph samples.

- Logged: 214 `DSPFFT` calls per channel during the exercise (the live display, not stored) and
  241 per channel in the BAGSD run (240 blocks of 500 samples and the 20-sample partial at the
  stop). The stored `FFT-Normal-Order` is the mean of those 241 outputs.
- This guest's graph start and 45 Hz switch were both consumed sample 107196 on every channel.
  They are found by matching the first logged FFT input block to the clone's Source output.
- The log (`s59.bin`, 385 MB) and the extracted inputs stay in the main checkout's `tmp/s59/`
  (`run2/`, `replay/`). See `bagsd-design.md`, "Clone and parity".

## Result: the DSPFFT stages captured (#57, 2026-11-07)

Run 8's rx stream replayed (`hook14.js` over `hook13.js`) with hooks on the three IPP calls
inside `DSPFFT::process` (`0x66ff20` forward FFT: windowed input and output; `0x66ff10`: the
packed output; `0x66ff00`: the magnitudes), each record tagged with the call's sequence number
next to the existing tag 30 (raw block and final spectrum): 388 calls over 97 blocks per channel.

- The windowed input is `f32(raw * window)` in all calls; the FFT output matches the clone's
  `RealFft512`; the magnitude is the `rsqrtps` plus Newton kernel (`sensor-status.md`, "Level
  bit exactness"). The raw log (19 MB) stays in the main checkout's `tmp/s57c/`.
- The app was started by a Startup-folder launch file in the guest with the log on the guest's
  NTFS disk (`C:\cap`), staged and read back with `ntfs-3g` after `ntfsfix -d` while the VM was
  off.

## Result: worn captures and the overlay state (#61, 2026-11-08)

Two worn-B-2 captures replayed (`hook` set `g/s61.js`: tag 12 consumed sample, tag 32 Head2D
level update, tag 41 overlay state), reduced by `scripts/worn-sensor-extract.py`; findings in
`sensor-status.md`, "Observed: worn replays".

- Standard unattended step: a Startup-folder launch file starts the app under `frida-inject -f`
  and the clicker, with the hook files and the log on the guest's NTFS disk, staged and read
  back with `ntfs-3g` after `ntfsfix -d` while the VM is off. Use it for every run.
- The emulator does not retry its socket connect: start it after the socket is forwarded and
  before the app starts, or the run connects to nothing.
- The `QWidget::setVisible` hook never fires, so the overlay state needs instruction-site
  hooks: the overlay show sites `0x568f29` (red), `0x568f59` (green) and `0x569cc8`
  (offline), and the debounce elapsed check at `0x568f21`.
- The original's host skips some received frames; replays feed the clone only the consumed
  frames (`audio-feedback.md`, #33 and #55).

## Host CPU clock: a replay runs at the stream rate only with turbo on (2026-11-07)

The original fell to about 1.5 % of the stream rate in replays run between 22:00 and 07:00
(#53's three runs): GUI clicks took minutes, titles read `(Not Responding)`. The cause is the
host's clock, not the guest, the hooks or Defender.

- Host config (observed): `cpu-turbo-schedule.timer` runs `/usr/local/bin/cpu-turbo-schedule.sh 22 7`,
  which writes `intel_pstate/no_turbo=1` from 22:00 to 07:00. Its comment intends "base clock",
  but this host's firmware reports a base ratio of 8 (MSR `0xCE` bits 15:8 and HWP guaranteed
  performance in `0x771` are both 8), so `no_turbo=1` caps every core at 800 MHz
  (`cpuinfo_max_freq` 800000, HWP request max ratio 8). With `no_turbo=0` the cap is 3.8 GHz.
  Prometheus (`node_cpu_scaling_frequency_max_hertz`, instance `node-exporter:9100`) shows the
  cap at 800 MHz from 22:05 on 2026-11-06; the real-time #48 and #33 replays ran before 22:00.
- A/B in one rig run (observed; run-8 rx stream, the `hook11.js` hook set, tag 12 = one consumed
  ch0 sample, 500/s at real time, binned per 30 s from the first tag 12):

  | Host cap | Tag 12 per s | Serial reads (tag 1) per s | Heartbeats per 30 s |
  |---|---|---|---|
  | 3.8 GHz (`no_turbo=0`), before and after Play | 500 | ~120 | 6 |
  | 800 MHz (`no_turbo=1`), toggled mid-run | 5 to 9 | 0.3 | 1 to 3 |
  | back to 3.8 GHz | 500 from the next bin | ~120 | 6 |
  | 2.3 GHz (`no_turbo=0`, `max_perf_pct=60`), separate run, 60 s past Play | 500 | ~119 | 6 |

  With the cap lifted the gear dialog opened 0.8 s after the click (about one minute at 800 MHz).
- Inferred: at 800 MHz the app needs more than its one busy core and never catches up; the
  drop (about 60-fold for a 4.75-fold clock cut) is not explained further here.
- Fix: run step 0 (turbo on for the run). 2.3 GHz also held the rate, so capping the night
  window with `max_perf_pct=60` instead of `no_turbo=1` would keep both night-time rigs and
  the schedule's "base clock" intent; that host change is not made.

## Result: run 8's no-wearer stream and the overlay state (#57, 2026-11-08)

Run 8's raw stream (`rx8.raw`: 24 bytes of partial read, then the handshake replies, then frames)
replayed with the #61 rig and hook set (`g/s61.js`) for the 100 s the clicker streams, no Play:
97 blocks logged. `scripts/worn-sensor-extract.py` reduces it (the handshake starts at the first
reply marker; a no-wearer stream needs `--min-count 50 --tolerance 3`), `tests/replay_s61.rs`
compares; findings in `sensor-status.md`, "Observed: run 8 no-wearer overlay state".

- Run steps as #61 plus rig-up: the live quadlet had no serial chardev, so `rig-s48` was copied
  over it (`pre-rig` kept) and put back after the run.
- The tick `Date.now()` stamps step by 15.6 ms (62.5 ms ticks); a debounce that reaches its delay
  in the stamp of the next block's update is a thread race (`ties.txt`).
