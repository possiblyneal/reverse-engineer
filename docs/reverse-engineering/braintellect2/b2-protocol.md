# BRAINtellect2 B-2 Controller Wire Protocol

Static reconstruction of the serial protocol the BRAINtellect2 app
(`braintellect.exe` v5.1.57) uses to talk to the `B-2` EEG controller, as the
basis for a Linux client. Addresses are for the captured original
`braintellect.exe` (SHA-256
`af7ff5430ee10c2d548450819f54a3f939af474953a42b959fd99739519c2560`,
image base `0x400000`).

Source: static disassembly only (capstone over the PE). No controller, serial
port, or runtime capture was involved. Observed facts cite a VA/file offset;
inferred behavior is marked. Items needing a live capture to confirm are called
out explicitly under Unresolved.

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
static bytes: `BT_Device::setStart(bool)` (symbol `0x14934b8`),
`setPower(string,bool,int)` (`0x1493190`),
`setSignalInjection(string)` (`0x1493280`),
`setLODFrequency(float)` (`0x1493368`), `setDeviceFrequency`, `setMode`, and
`clearIO` (`0x14923a0`). `setStart` concatenates ~12 QString fragments (12×
`call 0xf51410`) separated by `","`, drawing the command number and field values
from runtime-initialized BSS QString globals (e.g. `0x1f789b4`, `0x1f77ae0`,
`0x1f7784c`), then writes via the device vtable `+0x28`. Its log template
`setStart(delay: ` (fo `0x1491828`) shows the field set: delay, number of
channels, device frequency. Recovering its exact command code needs a live capture (Unresolved): the first
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
  skipped) into `[ebx+0x14…]`.
- Requests 110 and 112 (`getRequest for SYSTEM_INFO_1 / _2`, log fo `0x1493038`,
  `0x1492fe0`) follow. Their reply field layouts were not fully decoded
  (Unresolved).
- `setRequest(int, const uchar*, bool, int)` logs `setRequest(): Writing: 0x`
  followed by the hex of the bytes written, and `setRequest returned: ` with the
  reply length — useful anchors for a live capture.

## Inbound sample stream (processData_v5, VA `0x556560`, len 2650 bytes)

Packet layout (observed; `edi` = packet pointer from `[ebp+0xbc]`):

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
  used as sample magnitude (status/lead-off); the sample is the next three
  bytes as a **24-bit big-endian signed** integer (VA `0x556c46`:
  `movsx` high byte `[+1]`, `movzx` `[+2]`, `movzx` `[+3]`, two `shl 8`/`or`).
- **Lead-off channel**: when the channel's type field `[ebx+0x1ec]` equals `1`
  (constant at `0x188e204`), the value is instead `0x3fff - f(x)` (a 14-bit LOD
  reading; VA `0x556c2c`).
- Each decoded integer is converted to volts (below) via the per-channel
  processor object `[ebx+0x2c0]` (vtable `+0x18`) and forwarded to `0x5773a0`.

The 0x64 sync, the big-endian counter, and 24-bit big-endian signed samples are
consistent with the live log `setChannel(4 channels): success` and
`DataBuffer DTOR: packetCounterErrors: 0, sampleValidationErrors: 0`
(`device-connection.md`). The overall packet length and whether a
checksum/`sampleValidation` field exists were not located statically; the
`sampleValidationErrors` counter implies one (Unresolved).

## Runtime confirmation (live session 2026-11-03)

A full SMART-2 session run with the B-2 connected in the offline VM confirmed the
stream geometry from the guest app log (`tmp/baseline-2026-11-03/braintellect.log`):

- `sampleRate: 500` and `setDeviceFrequency: 500` (log ll. 758, 762): the device
  streams at **500 Hz**, resolving the per-channel sample cadence. It does **not**
  resolve the device family (12 vs 99); family is still Unresolved.
- `setNumberChannels: 8` (l. 761) is the **hardware maximum**, not the active
  count; the active montage is 4 channels (`FP1FP2T3T4`), matching
  `[ebx+0x1e8] == 4` and `setChannel(4 channels): success`.
- The `DataCapture`/`addDataPacket` buffers are **240000 bytes per 30 s**
  (`receiveMessage` reports 15000 signals). 15000 frames / 30 s = 500 Hz, and
  240000 / 15000 = **16 bytes/frame** = 4 channels × the 4-byte group above. So
  the captured buffer is the **raw wire frames** concatenated, not a re-encoded
  form: the `processData_v5` framing is the on-disk DataCapture framing.

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
| `+0x10` | sensorGain (G_s, ≥ 1000.0) | `setSensorGain(float,int)` VA `0x5579f0`, store VA `0x557a68` |
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
`processData_v5` imply the B-2 is a 24-bit family (12 or 99); which family, and
the actual `sensorGain`/`channelGain` the device reports, come from the 110/112
and 131 replies and were not pinned statically (Unresolved).

## Unresolved (needs live capture or deeper static-init tracing)

- `setStart` command code and exact field/argument order (runtime-built QString).
- `setPower`, `setSignalInjection`, `setLODFrequency`, `setDeviceFrequency`,
  `setMode` command codes and argument encodings (runtime-built).
- Reply field layouts for commands 110, 112, and 131.
- Packet trailer/length and the `sampleValidation` check feeding
  `DataBuffer.sampleValidationErrors`.
- Which device family the B-2 reports, and its per-channel `sensorGain`.

These are the fields a protocol-conformant Linux client cannot yet reproduce
exactly; everything above them (transport, the five fixed commands, the 101
handshake, the sample-stream framing, and the full volts formula with its
constants) is recovered from static evidence.
