# BRAINtellect2 Device Connection and COM-Port Selection

How the BRAINtellect2 app (`braintellect.exe` v5.1.57) selects the Bluetooth
serial port for the B-2 controller, and the stale-preference failure mode.

Sources: live application log (`C:\ProgramData\BST\BRAINtellect2\log\braintellect.log`),
Windows port enumeration (`SERIALCOMM`, `Get-PnpDevice -Class Ports`), and binary
class/string symbols. Observed configuration and inferred behavior are kept separate below.

## Runbook: connect the B-2 in the offline Windows guest

Verified working procedure (2026-11-04). B-2 Bluetooth MAC `886B0F8FB710`; the
analysis below explains each step. Only `braintellect-offline` passes the Bluetooth
adapter through (#38); pairing the B-2 in another guest stales this guest's bond.

1. **Launch the app.** On the guest console (noVNC `http://10.10.10.10:8006/`,
   session 1), run `Z:\BRAINtellect2\bin\start-braintellect.bat`. It sets the
   HKCU license keys, maps `Z:`, runs `set-b2-port.ps1` (resolves the B-2's
   *current* outgoing COM by MAC and writes `B-2-BTCOMPort` into the ini — this
   absorbs COM renumbering across power-cycles), and starts the host-patched
   `braintellect.exe`.
2. **Confirm the bond is authenticated.** The B-2 must be paired *and*
   authenticated, not merely remembered. Check from the guest:
   `Get-ChildItem 'HKLM:\SYSTEM\CurrentControlSet\Services\BTHPORT\Parameters\Devices'`
   lists bonded MACs; a `886B0F8FB710` entry whose link key is present is a real
   bond. If connect hangs ~25% (see table), the bond is stale — remove and
   re-pair (step 3).
3. **Re-pair only if needed.** Windows Settings → Bluetooth & devices → Remove
   `B-2`, then Add device → Bluetooth → `B-2` → "Your device is ready to go!".
   SSP just-works/yes-no, no PIN. (Pairing is a Windows bond; the app does not
   pair.)
4. **Connect in the app.** Click the "Connect to device" plug icon at the top of
   the left toolbar. Success: status bar reads `Device: B-2` and the Optimize tab
   enables. Coordinates shift with the noVNC window size — screenshot before
   clicking.

| Symptom | Cause | Fix |
|---|---|---|
| Connect hangs ~25%; log `open(): >COMn<` never returns | Stale bond: remembered but not authenticated | Remove the bond and re-pair via Settings (step 3), then reconnect |
| Semaphore timeout; RFCOMM never completes | Transient first-open failure of the passthrough link (2026-11-02 log: connects succeeded after 1 and 4 failed opens) | Retry Connect; check the box's light is on |
| Pairing says "We didn't get any response from the device"; HCI capture shows `Disconnection_Complete` reason `0x17` (Repeated Attempts) right after `Authentication_Requested` | Stale bond | Remove the bond in the guest, then pair once. Do not retry back-to-back: each retry extends the back-off |
| Opens a COM that exists but no data arrives | `B-2-BTCOMPort` points at a stale/renumbered or incoming port | Re-run `start-braintellect.bat` so `set-b2-port.ps1` re-resolves by MAC |
| Connects, streams ~6 s, drops with `Device not registered to this host computer` | App host-binding gate | Requires the NOP patch (already applied to the guest `braintellect.exe`; original is `braintellect.exe.orig`) — see below |

Keep the app's COM port held open: once connected, the sustained open keeps the
controller awake, so no separate keepalive is needed (final section).

## Observed: normal connect path reads a stored preference

On a device connect, the app takes the port name from a stored preference, not
from live hardware resolution. Log sequence (names verbatim):

- `trainerTool.DeviceConnectRunnable: CTOR(): deviceName: B-2, portName: COM3, preferenceName: B-2-BTCOMPort`
- `device.BSTAmplifier: createDevice(): portName: COM3`
- `device.SerialPortDriver: open(): >COM3<, buffered: 0, openMode: 0x3`
- `device.SerialPortDriver: open(): About to configure device: baud rate: 9600, data bits: 8, parity: 0, stop bits: 1, flow control: 0`
- `device.SerialPortDriver: open(): successful open n configure`

So the connect path opens exactly the port named by preference `B-2-BTCOMPort`,
at 9600 8N1, with no Bluetooth-address re-resolution.

## Observed: `B-2-BTCOMPort` is not persisted anywhere findable

The port value `COM3` is not stored in any location checked. Searches (all
read-only):

- **Database.** A portable MySQL 5.5 `mysqld` was pointed read-only at the live
  datadir `C:/ProgramData/BST/BRAINtellect2/data` (matching the embedded engine's
  geometry: `innodb_log_file_size=56623104`, two 54 MB log files, 58 MB `ibdata1`).
  `braintellectds.property` has exactly **5 rows**, all version metadata
  (`B2_LATEST_VERSION`, `B2_LATEST_VERSION_DESC`, `B2_LATEST_VERSION_EXE_URL`,
  `B2_LATEST_VERSION_URL`, `DB_VERSION_PROP`) — no `B-2-BTCOMPort` and no `COM*`
  value. `SHOW TABLES` lists a standard clinical ORM schema (person, session,
  staff, montagesystem, placementsystem, …) with **no** `preference`/`setting`/
  `config`/`device` table. So the preference is not DB-backed.
- **Filesystem.** `findstr /s /i "BTCOMPort"` under `C:\Users\Docker\AppData` and
  `C:\ProgramData\BST` matched nothing (only this session's PowerShell history).
  The literal key is not a plaintext token in any config file there.
- **Registry.** Recursive `reg query` for `B-2-BTCOMPort` under `HKCU\Software`
  and `HKLM\Software` returned 0 matches (prior session), despite QSettings
  registry code in the binary.

Binary symbols for the store: `com.bst.optimizer.model.preferences.Preferences`
(`src\model\preferences\Preferences.cpp`), and device config bound to XML types
`model::configFile::configurationDeviceType` /
`configurationDeviceListType` (namespace `http://optimizer.bst.com/model/configFile`,
parsed by `trainerTool::util::ConfigurationFile`). Log string
`run(): using BT COM port from preference:` confirms the connect path reads the
preference by that name.

## Inferred: `COM3` is a default, not a stored value

Given every candidate store (DB, filesystem, registry) lacks the key yet the app
still logs `portName: COM3`, `COM3` is most likely the value the `Preferences`
getter returns when `B-2-BTCOMPort` is **unset** — i.e. a default, not a value a
prior setup persisted. Consequence: there is no existing row/file/key to edit to
change it; a DB `UPDATE braintellectds.property … WHERE name='B-2-BTCOMPort'`
matches 0 rows (verified).

## Observed: Windows Bluetooth SPP port identity and direction

`HKLM\HARDWARE\DEVICEMAP\SERIALCOMM` maps `\Device\BthModem<NN>` to `COM<x>`; the
`<NN>` instance counter increments every time a BT serial port is (re)created.
Direction is read from the Ports-class `InstanceId`:

- Outgoing (client) port to a remote device:
  `BTHENUM\{00001101-0000-1000-8000-00805F9B34FB}_VID&..._PID&...\...&<REMOTE_MAC>_C00000000`
  — carries the remote device MAC and the `_C` suffix.
- Local incoming (server) port:
  `BTHENUM\{00001101-...}_LOCALMFG&0000\...&000000000000_0000000C`
  — zero MAC, `LOCALMFG`.

Because the outgoing port is a Bluetooth SPP port, its `COM` number can change
across controller power-cycles / re-pairing (the `BthModem` counter advances),
while the stored preference does not update itself.

## Inferred: the failure mode

If `B-2-BTCOMPort` names a port that is no longer the B-2's current outgoing
port (e.g. it now points at the local incoming port, or at a stale number), the
app's `open()` still **succeeds** — the named port exists and is a valid local
COM device — but no controller data ever arrives. The app then surfaces the
generic error "make sure the light on the B-2 controller box is illuminated …
select 'Connect to device' icon … to reconnect", with `Device: <none>`.

Because no editable store holds the port (see above), the durable remedy is **not**
a DB/registry/file edit — it is the app's device-setup flow, which re-resolves the
port from the live Bluetooth stack by MAC and uses it (below).

## Observed: the self-healing resolver is `findPortName` (MAC via registry)

The binary has `DeviceUtilities::findPortName(deviceAddress)`, which returns a
vector of port names resolved by matching the device's Bluetooth address against
the Windows registry serial-port list. It is backed by
`flowEngine::util::WinReg::findDevice(address, …)` / `WinReg::findDeviceDetails`
and the `BluetoothFindFirstDevice`/`…Next`/`…Close` APIs (log markers
`findPortName(): newPort via registry:`, `findPortName(): didn't find Bluetooth
device:`, `findPortName(): Unable to find device address in registry Serial port
list:`). Because it keys on the remote MAC, it returns the controller's *current*
outgoing port even after a power-cycle renumber.

No `findPortName` line appears on the observed connect path (which took `COM3`
from the preference), so this MAC-based resolver runs on the device-setup /
reconnect path, not on every connect. It is the self-healing mechanism for port
drift; the connect entry point is `connectToDeviceSlot()` /
`DeviceConnectRunnable`.

## Observed: connect succeeds fully, then an app-level host-registration gate rejects it

On a correctly-pointed port, the link and data path complete before any
registration verdict. Log sequence (names verbatim, one observed connect):

- `connectToDevice(B-2): portName: COM4` → `SerialPortDriver open(): >COM4<, openMode: 0x3` — link up
- `isDeviceValid(B-2): Connected. Checking device ID` → `setChannel(4 channels): success`
- `DataBuffer DTOR: packetCounterErrors: 0, sampleValidationErrors: 0` — clean 4-channel EEG streaming
- ~6 s later: `SerialPortDriver close(): COM4`
- `deviceConnectionChangedSlot(): status: disconnected, have device? no, isRegistered: 1, isExpired: 0`
- `ERROR connectToDevice(): Device not registered to this host computer`

So the drop is **app-initiated host-binding enforcement**, not a link loss,
timeout, or controller sleep. `isRegistered:1`/`isExpired:0` show the license
itself is valid; the rejection comes from a separate per-host identity check — a
controller registered to one host (the original tablet) is refused on a different
host.

## Observed: the host check and its call site (static)

Addresses are for the captured original `braintellect.exe`
(SHA-256 `af7ff5430ee10c2d548450819f54a3f939af474953a42b959fd99739519c2560`;
image base `0x400000`). Both "Device not registered to this host computer" C
strings (VA `0x14e8e7c`, and a trailing-period variant `0x14e8f30`) are each
referenced from exactly one site inside the connect routine at VA `0x41fe30`
(file offset `0x1f230`):

- VA `0x420348` (fo `0x1f748`) pushes `0x14e8e7c` — failure path A, taken when
  `call 0x41b9d0` returns `al == 0`.
- VA `0x42000d` (fo `0x1f40d`) pushes `0x14e8f30` — failure path B, same validator.

The validator `0x41b9d0` is **not** a pure predicate: on the success path it
initializes device runtime properties (`mov [eax+0x3c], ecx`; `call 0x406540`;
log `connectToDeviceSlot(): setting runtimeProperties on device`). The verdict
is decided by the inner host-ID comparison `call 0x4f9310; test al,al;
je 0x41bbc5` at VA `0x41bae8` (fo `0x1aee8`): a failed comparison jumps past the
init and returns `al == 0` ("not registered"); `[esi+0xa8] != 0` short-circuits
to success before the comparison.

## Inferred: the minimal side-effect-preserving bypass

Forcing `0x41b9d0` to return true would skip the runtime-property init. The
surgical change is to make the inner host-ID comparison always treated as passed,
so the success path (init + return true) always runs:

- File offset `0x1aee8`: `0F 84 D7 00 00 00` (`je 0x41bbc5`) → `90 90 90 90 90 90`
  (6× NOP).

Observed effect in the offline VM (patched guest copy; original kept as a
backup): the connect establishes and **stays** — Device shows B-2, SENSOR STATUS
"Ready", the Optimize tab enables, the app holds the outgoing COM port
exclusively, and no "not registered" error is logged. This is the same class of
host-binding bypass as the earlier startup machine-profile patch (file offset
`0x3583b`), and is reversible.

## Observed: holding the port open keeps the controller awake

The B-2 auto-sleeps when idle, but an open outgoing SPP channel alone keeps it
awake — a read-only `open()` of the port from the guest held the RFCOMM link up
for ~2m41s with zero bytes written and no sleep. So once the app opens and holds
the COM port, no separate keepalive/ping is needed; the sustained open prevents
the idle-sleep drop that looks like a connection failure.
