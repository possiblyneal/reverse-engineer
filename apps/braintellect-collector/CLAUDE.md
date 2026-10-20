## Purpose

`BST-collect.ps1`: one PowerShell script a BRAINtellect2 tablet owner runs on Windows 8, 8.1
or 10 to get a single Desktop ZIP of the installation, database, relevant registry keys and
diagnostics, without changing the tablet. The unit runs nothing here and ships nothing
(`.unit.json`); `scripts/build-delivery` builds the owner archive, which is sent by hand
(`docs/adrs/0002-add-braintellect-collector-unit.md`).

## Ownership

| Path | What |
|---|---|
| `src/BST-collect.ps1` | the collector |
| `assets/INSTRUCTIONS-for-tablet-owner.txt` | the owner's instructions, delivered beside it |
| `scripts/build-delivery` | builds the owner archive: those two files only |
| `tests/test_delivery-archive.sh` | checks the archive membership |
| `tests/integration/` | the VM test and its guest half |

## Local Contracts

- **One script, two modes.** Interactive by default (progress window, collection on a
  background thread). `-ShowUI:$false` or `BST_NO_UI=1` skips the window and runs the collection
  synchronously, so
  a failure is an unhandled error in the caller and `powershell -File` exits non-zero. Never
  put the headless path on a thread: the error is then lost and the exit stays 0.
- **Windows 8 floor: PowerShell 3, .NET 4.5.** The ZIP comes from
  `[System.IO.Compression.ZipFile]`, never `Compress-Archive` (PowerShell 5). Do not use
  syntax or cmdlets newer than PowerShell 3.
- **The tablet is never modified and nothing is sent.** The database is read from a private
  copy in `$env:TEMP` by the tablet's own bundled `mysqld`; the only artifact left is the
  Desktop ZIP, which the owner sends. The script makes no network call.
- **The ZIP is the diagnostic.** It carries `STATUS.txt` (each stage `[OK]`, `[PARTIAL]`,
  `[SKIP]` or `[FAIL]`, written even on failure) and `SYSTEM-INFO.txt` (OS, hardware,
  PowerShell, .NET release, CIM, `reg.exe`, ZIP API, MySQL engine path and version). A new
  dependency the script relies on gets a line in `SYSTEM-INFO.txt`.
- `INSTRUCTIONS-for-tablet-owner.txt` is read by a non-technical owner: plain words, no repo
  vocabulary. It must match what the script prints and creates.
- The delivery archive holds the script and instructions only: no tests, no exports.

## Work Guidance

- The Windows VM is a test fixture only. Its Windows 11 details say nothing about the tablet.
- Never run the collector against a physical tablet from here.

## Verification

- `tests/test_delivery-archive.sh`: builds the archive and asserts it holds exactly the script
  and the instructions. Needs no VM; run it after changing `scripts/build-delivery`.
- `tests/integration/test_collector-headless.sh` (needs the VM; run by hand, since
  `scripts/integration` finds no project manifest to dispatch to). It stages the
  script in a fresh `/home/neal/windows-data/bsttest/r<epoch>/` folder (guest
  `\\host.lan\data\bsttest\…`), types one command into the guest's focused admin PowerShell
  window through the QEMU monitor (no guest SSH), and inspects the returned ZIPs: zero exit,
  valid archive, every stage, SQL rows, registry export, `SYSTEM-INFO.txt` fields, and a
  non-zero exit with `[FAIL] 2. Database` when the schema folder is hidden. About 6 minutes.
- Fixture precondition: container `windows` on `server` running, a synthetic install at
  `C:\braintest` (embedded MySQL 5.5 `mysqld.exe`; tables `person`=3, `session`=4,
  `flowdesign`=2, `montagesystem`=3 rows), and an admin PowerShell window focused on the guest
  desktop. The fixture setup is not tracked here.
- The progress window has no automated check: run `BST-collect.ps1` without `BST_NO_UI` on the
  guest desktop and watch it.

## Child Index

None.
