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
(on `neal@10.10.10.10`). Backup before editing: `braintellect-offline.container.bak-20261106`;
the rig version is kept as `braintellect-offline.container.rig-s48`. The restored file is the
live quadlet; the running container still has the rig arguments until its next restart.

```
-PublishPort=127.0.0.1:8007:8006          (8007 was bound by another service)
+PublishPort=127.0.0.1:8017:8006
-AddDevice=/dev/bus/usb                   (no adapter passthrough while the rig runs)
-Environment="ARGUMENTS=-device usb-host,vendorid=0x0a5c,productid=0x21e8 -audiodev alsa,id=snd0,out.dev=bstout,out.frequency=44100 -device ich9-intel-hda -device hda-output,audiodev=snd0"
+Environment="ARGUMENTS=-audiodev none,id=snd0 -device ich9-intel-hda -device hda-output,audiodev=snd0 -chardev socket,id=b2,path=/b2/b2.sock,server=on,wait=off -device isa-serial,chardev=b2,iobase=0x2f8,irq=3"
+Volume=/srv/local/windows/braintellect-offline/b2:/b2
```

The rig run has no `usb-host` device and no `AddDevice`, so the physical adapter and the B-2
bond (`device-connection.md`) are never touched. Restoring the original adds the adapter back;
do that only when the VM is free to own it. The serial port is guest COM2 (16550 at 0x2f8).
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
7. Copy `C:\cap\run8.bin` out through the CAP image and parse it with the run-8 parsers in
   `apps/braintellect-linux/scripts/make-fixtures.py` (`run8_records`, `run8_blocks`).

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

## Capture size note

`rx8.raw` (the RX bytes extracted from run 8's `run8.bin` tag-1 records) holds about 639k
contiguous frames, against about 149k received frames recorded in `audio-feedback.md`. The
emulator replays from the start of the file; whether the extra frames are earlier sessions
appended to the log has not been checked.

## Not run

The clone's live-session capture from a real B-2 needs the physical B-2 and adapter and was not run; it is covered by the fixture
transport tests (`tests/live_session.rs`).
