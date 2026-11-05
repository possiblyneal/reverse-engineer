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

Observed wiring sends the threshold pass/fail state to the counter, whose output triggers the sound. The counter is configured with a 500 ms trigger epoch; whether that means continuously high input and how it resets or repeats remain unverified.

### Static implementation evidence

Static disassembly of `braintellect.exe` (SHA-256 `af7ff5430ee10c2d548450819f54a3f939af474953a42b959fd99739519c2560`) shows:

- The processing loop at `0x62f920`–`0x62fcc6` passes input through `ThresholdSeries` and converts its Boolean result to `0.0` or `1.0` at `0x62fa35`–`0x62fa48`.
- An inspected simple-comparison branch at `0x62ed70` marks a sample successful when it is greater than or equal to its comparison threshold. This does not establish which branch implements the active design's `decrease` goal.
- `timeHigh` selects handler `0x643140` at `0x6431df`–`0x643204`; this identifies dispatch but not the counter's full timing or reset behavior.

If Boolean results are averaged, the average is a success fraction, but the inspected code does not establish that this happens or that `0.65` and `0.75` control it. Their meaning, the automatic-adjustment formula, and the roles of the 5,000 ms and 125 ms epochs remain unresolved. No runtime behavior was verified.

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
- The database contains sensitive material. This document intentionally does not reproduce names, dates, session contents, or signal values.
- The raw signal blobs are Java-serialized data and have not been semantically decoded in this record.
