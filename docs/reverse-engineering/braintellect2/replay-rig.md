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

## Attempt: capturing the Evaluator inputs and bands A..D (#53, 2026-11-07, not completed)

The goal was a replay of the run-8 rx stream with hooks on the Evaluator (each Balance and
Coherence series value per epoch, the chosen code) and on bands A..D. The capture was not made.

- Static (observed, Ghidra `analyzeHeadless` over a copy of the `bt2` project): the series push
  (`0x63d9c0`), the series evaluation (`0x63e540`), the Evaluator decision (`0x63eb70`, code at
  object offset `+0x274`) and the Evaluator code setter (`0x63ea70`). A hook set for them,
  `hook12.js` (`hook11.js` plus those four), was written and loaded under `frida-inject -f`
  with the app connected to the emulator and idle on the Optimize tab.
- The blocker is driving the guest GUI to Play (gear, OK, Play). With the guest on the QEMU
  USB tablet: HMP `mouse_move` moves only the PS/2 mouse (relative, accelerated, not
  addressable), an RFB pointer event moves nothing, and the keyboard reaches the focused
  window only (Tab and Enter did not open the gear dialog; Alt+Tab focuses the injector's
  console). A hot-added helper image with a PowerShell `SetCursorPos`/`mouse_event` clicker run
  from the Win+R dialog is the untried-to-completion route: the screen lags the guest by
  several seconds and HMP `sendkey` text drops characters when typed at 0.12 s per key, so the
  Run dialog did not launch it reliably.
- Next try: launch a clicker script from the CAP image's own `s53-run.cmd` (start it before the
  app, with waits tuned to the Start time in `emu.log`) so no typing is needed in the guest.
