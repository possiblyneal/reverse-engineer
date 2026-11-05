# BRAINtellect2 B-2 Controller Wire Protocol

Static reconstruction of the serial protocol the BRAINtellect2 app
(`braintellect.exe` v5.1.57) uses to talk to the `B-2` EEG controller, as the
basis for a Linux client. Addresses are for the captured original
`braintellect.exe` (SHA-256
`af7ff5430ee10c2d548450819f54a3f939af474953a42b959fd99739519c2560`,
image base `0x400000`).

Source: static disassembly (capstone over the PE), plus two live runs (issue #26)
traced with Frida hooks: 2026-11-04 on `BT_Device::write` and the `readAsync`
reply reader, and a second run on the Win32 COM handle (`CreateFile`/`ReadFile`/
`GetOverlappedResult`) cross-checked against a host usbmon capture (see "Live
capture (2026-11-04)" and "Observed stream frame (COM-handle capture)"). Observed facts cite a VA/file
offset or the live method; inferred behavior is marked. Items still needing a
capture are called out explicitly under Unresolved.

Verification: every instruction-level claim below was independently re-confirmed
against this binary (the five command literals and their file offsets, the
`processData_v5` sync/counter/sample sites, and the `_convertValue`/recompute and
`setDeviceFamily` arithmetic). The `grep` over `!*#` returns one extra, non-wire
match (noted under Command framing), and the parametric senders' runtime
construction was confirmed to have no static-initializer source.

## Transport

- Classic Bluetooth SPP → RFCOMM, **9600 baud, 8 data bits, no parity, 1 stop
  bit, no flow control** (observed, `device-connection.md`: live
  log `baud rate: 9600, data bits: 8, parity: 0, stop bits: 1, flow control: 0`).
- The amplifier class is `BSTAmplifier` with two backends,
  `HID_USB_Device` and `BT_Device`; the B-2 is a `BT_Device` (symbols at fo
  `0x1492310`, `0x14930d0`, `0x1493190`…).
- `BT_Device::write(int, const uchar*, bool, int)` (VA `0x54ffa0`) is a
  mutex-guarded forwarder to the underlying serial object at `[this+0x40]` via
  vtable (`+0x34` write, `+0x44` flush, `+0x38` post-write wait). On Linux this
  maps to a single `AF_BLUETOOTH`/`BTPROTO_RFCOMM` socket bound to the B-2 MAC;
  no COM port and no Windows BT stack are involved (inferred).

## Command framing

Fixed commands are ASCII, framed `!*#<code>,<arg>,<arg>,…` with comma
separators (separator literal `","` at VA `0x18851d0`). All five statically
embedded command literals carry six zero args:

| Code | Literal (fo) | Meaning | Sender / send site |
|---|---|---|---|
| 101 | `!*#101,0,0,0,0,0,0` (`0x149317c`) | serial-number request | `getSystemInformation`, sent VA `0x5525a2` |
| 110 | `!*#110,0,0,0,0,0,0` (`0x1493078`) | system info block 1 | sent VA `0x552843` |
| 112 | `!*#112,0,0,0,0,0,0` (`0x1493020`) | system info block 2 | sent VA `0x552ae7` |
| 131 | `!*#131,0,0,0,0,0,0` (`0x1494e40`) | get channel information | `getChannelInformation` |
| 48  | `!*#48,0,0,0,0,0,0` (`0x1493470`) | stop streaming | `BT_Device::setStop`, VA `0x553f24` |

These are the only `!*#` **wire-command** literals in the image. A `grep -F '!*#'`
over the PE returns six matches: the five above plus `!*#0,1,com.bst.optimizer`
`.flowEngine.device.BSTAmplifierDataProcessor…` (fo `0x1496930`, referenced once at
VA `0x13a0351`), which is an internal object descriptor — its payload is a class path,
not a controller command — and is not sent to the device.

The parametric commands build their payloads at runtime rather than from a
stored literal, so their numeric codes and argument order are **not** present as
static bytes (the live wire codes are in "Live capture (2026-11-04)" below): `BT_Device::setStart(bool)` (symbol `0x14934b8`),
`setPower(string,bool,int)` (`0x1493190`),
`setSignalInjection(string)` (`0x1493280`),
`setLODFrequency(float)` (`0x1493368`), `setDeviceFrequency`, `setMode`, and
`clearIO` (`0x14923a0`). `setStart` concatenates ~12 QString fragments (12×
`call 0xf51410`) separated by `","`, drawing the command number and field values
from runtime-initialized BSS QString globals (e.g. `0x1f789b4`, `0x1f77ae0`,
`0x1f7784c`), then writes via the device vtable `+0x28`. Its log template
`setStart(delay: ` (fo `0x1491828`) shows the field set: delay, number of
channels, device frequency. Static tracing could not recover the code: the first
fragment globals (`0x1f789b4`, `0x1f77ae0`, `0x1f7784c`) are referenced **only**
inside `setStart` (both xrefs at VA `0x5544xx`), so no global constructor writes
them — they are filled from runtime device state, not from a traceable static
literal. Static-initializer tracing therefore cannot recover the code either.

## Handshake (getSystemInformation, VA `0x5524a0`)

On connect the app issues 101 → 110 → 112 and parses fixed-layout replies
(observed):

- Request 101 (serial number) sent at VA `0x5525a2`. Reply expected length
  `0x38` (56 bytes). Magic header `AC BC CC 03` at reply offsets `[+0x54..+0x57]`;
  then `[+0x5c]==0x65`, `[+0x5d]==3`. Serial-number bytes are read from reply
  offsets `0x5e,0x5f,0x61,0x62,0x64,0x65,0x67,0x68,0x6a,0x6b` (every third byte
  skipped) into `[ebx+0x14…]`. The live reply carries six such pairs (a 12-byte
  id), one more than this static offset list shows.
- Requests 110 and 112 (`getRequest for SYSTEM_INFO_1 / _2`, log fo `0x1493038`,
  `0x1492fe0`) follow. Reply layouts: see "Live capture (2026-11-04)".
- `setRequest(int, const uchar*, bool, int)` logs `setRequest(): Writing: 0x`
  followed by the hex of the bytes written, and `setRequest returned: ` with the
  reply length — useful anchors for a live capture.

## Inbound sample stream

The wire stream the device sends after start is the 32-byte frame in "Observed
stream frame (COM-handle capture)" below. The `processData_v5` layout in this
section (0x64 sync, 4-byte channel groups) is the static reading of a different
parser. It was **not** the layout seen on the wire in the traced run, so treat it
as unconfirmed legacy and do not implement it.

### processData_v5 (VA `0x556560`, len 2650 bytes), static reading

Packet layout (static; `edi` = packet pointer from `[ebp+0xbc]`):

- **Sync byte**: `[edi+0]` must equal `0x64`; otherwise the app logs
  `bad header of packet […]? 0x` with `edi[0]`,`edi[1]` (check at VA `0x556aa3`).
- **Packet counter**: 16-bit big-endian at `[edi+3]<<8 | [edi+4]`
  (VA `0x556ab3`). Compared to the stored previous counter `[ebx+0x3e0]`; a value
  other than previous+1 logs `Packet counter error. Old counter: …, new packet
  counter: …` and increments `DataBuffer.packetCounterErrors`. Stored back at
  VA `0x556bd9`.
- **Samples**: channel count is `[ebx+0x1e8]` (observed 4 on the live device).
  Each channel occupies a **4-byte group**; the per-channel byte offset advances
  by 4 (`add [ebp-0x18], 4` at VA `0x556cd1`). Byte `[+0]` of the group is not
  read; the sample is the next three
  bytes as a **24-bit big-endian signed** integer (VA `0x556c46`:
  `movsx` high byte `[+1]`, `movzx` `[+2]`, `movzx` `[+3]`, two `shl 8`/`or`).
- **Placebo mode**: when `[ebx+0x1ec]` equals `1` (constant at `0x188e204`, VA
  `0x556c2c`), the sample is replaced by `0x3fff - rand()`. `launchExercise` sets the
  field from a compare with `"placebo"` (`0x440574`–`0x44059b`). There is no lead-off
  decode (`sensor-status.md`).
- Each decoded integer is converted to volts (below) via the per-channel
  processor object `[ebx+0x2c0]` (vtable `+0x18`) and forwarded to `0x5773a0`.

The big-endian counter and 24-bit big-endian signed samples were confirmed on the
wire; the 0x64 sync and the 4-byte channel group were not (the wire frame has the
`AC BC CC 03` marker and 3-byte samples). The relationship of this function and the
`processData` path (about VA `0x556ff0`) to the ring-buffer parser at `0x55a280`
(below) is unresolved; neither was on the observed wire path.

## Runtime confirmation (live session 2026-11-03)

A full SMART-2 session run with the B-2 connected in the offline VM confirmed the
stream geometry from the guest app log (`tmp/baseline-2026-11-03/braintellect.log`):

- `sampleRate: 500` and `setDeviceFrequency: 500` (log ll. 758, 762): the device
  streams at **500 Hz**, resolving the per-channel sample cadence. The device
  family is resolved by the 2026-11-04 run (12).
- `setNumberChannels: 8` (l. 761) is the **hardware maximum**, not the active
  count; the active montage is 4 channels (`FP1FP2T3T4`), matching
  `[ebx+0x1e8] == 4` and `setChannel(4 channels): success`.
- The `DataCapture`/`addDataPacket` buffers are **240000 bytes per 30 s**
  (`receiveMessage` reports 15000 signals). 15000 signals / 30 s = 500 Hz. The
  session-1 reading of 16 bytes per signal (4 channels × a 4-byte group) is
  superseded: 240000 B / 30 s = 8000 B/s = 250 observed 32-byte frames/s, and each
  frame carries 2 samples per channel (500 Hz per channel). The byte total matches
  the raw wire frames concatenated (inferred from the arithmetic; the buffer
  contents themselves were not read).

No raw sample stream is persisted past the session; the DataCapture buffers are
processed down to the stored result blobs (`session-storage.md`).

## ADC → volts conversion

`BSTAmplifierDataProcessor::_convertValue(const int&, const int&)` (VA
`0x555930`) returns, per channel:

```
volts = rawADC / divisor[ch]            ; fild raw; fdiv [entry+0x20]  (VA 0x555984-0x555988)
```

`divisor[ch]` is a `double` at offset `+0x20` in a per-channel calibration entry
held in a vector at `DataProcessor + 0xC0`, indexed by the bounds-checked
accessor at VA `0x616080`. `getGainVoltsPP(ch)` (VA `0x5557a0`) returns the
entry's `+0x28` field (the peak-to-peak input range, "voltsPP").

Per-channel calibration entry fields (observed from the setters/recompute):

| Offset | Field | Set by |
|---|---|---|
| `+0x10` | sensorGain (G_s; observed 10 live, so the static `1000.0` constant is not a floor on it) | `setSensorGain(float,int)` VA `0x5579f0`, store VA `0x557a68` |
| `+0x18` | channelGain (G_c, PGA, in **[1.0, 24.0]**) | `setChannelGain(float,int)` VA `0x557da0`, store VA `0x557e18` |
| `+0x20` | divisor (counts per volt) | recompute VA `0x5558fd` |
| `+0x28` | voltsPP (p-p input range) | recompute VA `0x55590e` |

The recompute (VA `0x5558c0`, run after any setter) derives, per channel, from
two device-level doubles `D1 = [proc+0xd8]` and `D2 = [proc+0xe0]`:

```
divisor = channelGain * D2 * sensorGain / D1        ; VA 0x5558ea-0x5558fd
voltsPP = 2 * D1 / (channelGain * sensorGain)        ; VA 0x555900-0x55590e
```

Therefore:

```
volts = rawADC * D1 / (channelGain * D2 * sensorGain)
```

`D1` (reference voltage) and `D2` (ADC full-scale counts) are set by
`setDeviceFamily(int)` (VA `0x557b50`), which accepts families **11, 12, 99**
(others → `setDeviceFamily(): invalid family`). `D2` is computed as
`pow(2, N) - 1` (CRT `pow` at `0x1073896`, minus `1.0` at `0x187c9b0`):

| Family (`[proc+0xe8]`) | D1 (Vref) | D2 (full-scale) | Sample width |
|---|---|---|---|
| 11 | 2.4 V | `2^15 - 1` = 32767 | 16-bit |
| 12 | 2.4 V | `2^23 - 1` = 8388607 | 24-bit |
| 99 | 4.0 V | `2^23 - 1` = 8388607 | 24-bit |

Constants read from `.rdata`: `2.4` (`0x18953b0`), `4.0` (`0x1895398`),
`2.0` (`0x18953a8`), `15.0` (`0x18953a0`), `23.0` (`0x1895390`),
`1.0` (`0x187c9b0`); channelGain max `24.0` (`0x18954dc`), sensorGain min
`1000.0` (`0x1895360`). The 24-bit big-endian signed samples in
`processData_v5` imply a 24-bit family (12 or 99); the live run pins family 12
(next section).

## Live capture (2026-11-04)

Method: one complete SMART-2 exercise (240 s, 4 channels, 500 Hz) in the offline
guest with no wearer; `frida-inject` (spawn mode) hooks logged every
`BT_Device::write` (VA `0x54ffa0`; length at `[esp+4]`, buffer at `[esp+8]`) and
every `BT_Device::readAsync` return (VA `0x550430`) with millisecond timestamps,
alongside the app log. The streamed sample frames did **not** pass through the
hooked read functions, so this run captured only the handshake replies on the RX
side; the second run's COM-handle hook captured the stream (next section).

### Commands sent (observed, `write` hook)

Frame `!*#<code>,<a1>,…\r\n`; the six-argument form is 19-20 bytes on the wire.

| Operation | Bytes after `!*#` | Notes |
|---|---|---|
| setStop | `48,0,0,0,0,0,0` | sent first, before the handshake, and again before start |
| setMode(Normal EEG) | `40,1,0,0,0,0,0` | after the first 101-138 query pass |
| setSignalInjection(OFF) | `46,0,0,0,0,0,0` | with setMode; resent during the exercise |
| setStart(delay 0, 4 channels) | `48,1,0,4,0,0,0` | a1 = start flag; a3 = channel count and a2 = delay (inferred from delay 0 / 4 channels) |
| setPower(OFF) | `48,0,1,0,0,0,0,0` | seven arguments, 21 bytes; sent at app exit |

Code 48 is shared: a1 = 1 starts streaming, a2 = 1 powers off. `setDeviceFrequency`
(500) and `setLODFrequency` were called in the app log but produced **no** wire
traffic in this run; the start frame carries no frequency. Every set command
returned no reply; each of 101, 110, 112, 131-138 returned exactly one 56-byte
reply. The query sequence 101, 110, 112, 131-138 ran twice (before and after
setMode/setSignalInjection).

### Reply layout, commands 101/110/112/131-138 (observed)

56 bytes: `AC BC CC 03`, then `03 03 03 03`; byte `[8]` echoes the request code
(`0x65`, `0x6e`, `0x70`, `0x83`…`0x8a` = 101, 110, 112, 131…138); `[9]` = `03`;
from `[10]` the payload is two-byte pairs each followed by a `03` separator, then
`03` fill to the end.

- 101: six pairs (12-byte device id), matching the app's `id` log field.
- 110: six pairs. Pairs 3-4 give the firmware version bytes 17, 1, 1 (pair 3 =
  17, 1; pair 4 starts 1); pairs 5-6 give the model digits `0002`, `0004` (app
  log `model: 0002.0004`, `FW version: 17.1.1`). Pairs 1-2 are unidentified.
- 112: one non-zero pair, unidentified.
- 131-134 (active channels): pair 1 = (channel number 1-4, state byte); pair 2 =
  (12, 0), where 12 equals the `channelGain` of 12 in the app's `dumpDebug`
  (inferred mapping). The state byte was 2 in the first pass and 1 in the second,
  after setMode(Normal EEG) (inferred: current mode).
- 135-138 (inactive): pair 1 = (channel number 5-8, 0), pair 2 = (0, 0).
- The app then logs `family: 12` and `numberChannels (from device): 4`, matching
  the four active channel replies.

### Calibration confirmed (observed, app log `dumpDebug`)

Per channel: `channelGain: 12`, `sensorGain: 10` (from the app setting
`flowEngine.device.sensorGain`, not from the device), `voltageRef: 2.4`,
`voltsPP: 0.04`, `conversion factor: 419430350`. These match the formulas above
for family 12: `2*2.4/(12*10) = 0.04` and `(2^23-1)*12*10/2.4 = 419430350`.

### Stream counters (observed, app log)

`DataBuffer` reports `samplesPerPacket: 2`, `samplesToStore: 20`,
`devicePacketSize: 56`, and ends the run with `packetCounterErrors: 0,
sampleValidationErrors: 0`. `devicePacketSize: 56` matches the 56-byte handshake
reply size, not the 32-byte stream frame (inferred from the sizes).

### Observed stream frame (COM-handle capture)

Method: second traced SMART-2 session (guest profile, no wearer, sensors off). A
Frida hook on the Win32 COM handle (`GetOverlappedResult` completions) logged every
received chunk with a millisecond timestamp. About 2.9 thousand frames were also
checked against a host usbmon capture of the Bluetooth adapter. usbmon text keeps
only the first 32 bytes of each URB, so that check is partial; every frame it
covered matched the hooked bytes, none differed.

- **Stream start (observed)**: the stream begins after `!*#48,1,0,4,0,0,0` and
  runs **continuously**. It was not stopped at session end (the app was left
  connected and frames kept arriving); the end of a session is an app-side event.
- **Frame, 32 bytes (observed)**: marker `AC BC CC 03`, a 16-bit big-endian
  counter, the two bytes `03 03`, then a 24-byte payload. There is no trailer or
  checksum: the next marker follows the payload directly. All 194250 frames in
  the capture are 32 bytes with `03 03` at header bytes +2/+3 (the last is cut by
  the end of the log).
- **Counter (observed)**: increments by 1 on every consecutive frame (no gap in
  194250 frames) and wraps at 65536. The rate is 250.0 frames/s; the 240 s
  exercise window held exactly 60000 frames.
- **Payload (observed)**: 8 samples of 24-bit big-endian signed, 3 bytes each.
  Channel-interleaved with 2 samples x 4 channels (inferred, see Unresolved). This
  is 500 Hz per channel, matching `sampleRate: 500`.
- **Low bits (observed)**: the low 2 bits of every sample's third byte are always
  `11`. They are marker bits, so the usable resolution is 22 bits plus sign
  (inferred: the app does not mask them before converting; not checked).
- **Chunking (observed)**: ReadFile completions deliver arbitrary byte runs that do
  not align with frames; a parser has to resync on the marker.

#### sampleValidation and counter errors (static, plus trace)

The ring-buffer parser at VA `0x55a280`, called from `0x55b08b`, finds the marker
(header sync at `0x55a374`-`0x55a416`), checks the `03` bytes of the header
(the byte-at-+3 check at `0x55a499` and `0x55a512`; the +2 check is inferred by
symmetry, not confirmed), and decodes the samples at `0x55a8a2`-`0x55a8bb`.

- **sampleValidation**: `(third byte of each sample & 3) == 3` (loop at
  `0x55a69c`/`0x55a69e`), plus the two header `03` bytes. A failure increments
  `sampleValidationErrors`, stored at `[obj+0x50]` (VA `0x55aa3b`).
- **packetCounterErrors**: stored at `[obj+0x4c]` (VA `0x55a549`); a counter that
  is not previous+1 increments it. The trace showed no counter gaps, matching the
  logged `packetCounterErrors: 0, sampleValidationErrors: 0`.

## Unresolved

- 110 pairs 1-2 and the 112 payload meaning.
- Whether the device reports family itself or the app derives it from the
  model/firmware fields.
- Channel and sample order inside the 24-byte payload: channel-interleaved (two
  samples of each channel back to back vs. one sample of each channel twice) is
  inferred from per-slot means only, not from a known test signal.
- Why the DataCapture header reports `samplesPerPacket 4` while `DataBuffer` logs 2.
- How `processData_v5` (`0x556560`) and `processData` (about `0x556ff0`) relate to
  the ring-buffer parser at `0x55a280`; neither ran on the observed path.

These are the fields a protocol-conformant Linux client cannot yet reproduce
exactly; the transport, the commands above, the handshake, and the volts formula
with its constants are recovered.
