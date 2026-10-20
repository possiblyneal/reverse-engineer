## Purpose

`Collect-BRAINtellect2-Data.cmd`: one file a BRAINtellect2 tablet owner double-taps on
Windows 8, 8.1 or 10 to get a single Desktop ZIP of the installation, database, relevant
registry keys and diagnostics, without changing the tablet. It is a batch/PowerShell polyglot:
a short batch header (a PowerShell block comment) relaunches PowerShell on the same file, so
one tappable icon carries the whole program. The unit runs nothing here and ships nothing
(`.unit.json`); `scripts/build-delivery` builds the owner archive, which is sent by hand
(`docs/adrs/0002-add-braintellect-collector-unit.md`).

## Ownership

| Path | What |
|---|---|
| `src/Collect-BRAINtellect2-Data.cmd` | the collector (a tap-to-run batch/PowerShell polyglot) |
| `assets/INSTRUCTIONS-for-tablet-owner.txt` | the owner's instructions, delivered beside it |
| `scripts/build-delivery` | builds the owner archive: those two files only |
| `tests/test_delivery-archive.sh` | checks the archive membership |
| `tests/integration/` | the VM test and its guest half |

## Local Contracts

- **One tap-to-run file, two modes.** The file is a polyglot: its first ten lines are a batch
  header wrapped as a PowerShell block comment (`<# : … : #>`). A double-tap (or `cmd /c`) runs
  the header, which sets `BST_SELF=%~f0` and relaunches `powershell -Command` reading the whole
  file as a script block; PowerShell skips the header as a comment. Because `-Command` leaves
  `$PSCommandPath` empty, the self-relaunch path resolves to `BST_SELF`. Interactive by default:
  the parent shows the progress window and runs the collection synchronously in a hidden child
  (`cmd /c "<self>"`, headless, progress handed back through `BST_PROGRESS_FILE`), closes the
  window when the child ends and exits with the child's status. `-ShowUI:$false` or `BST_NO_UI=1`
  skips the window and runs the collection in the calling process, so a failure is an unhandled
  error that exits non-zero. Never run the collection on a .NET thread: it has no PowerShell
  runspace and its errors are lost.
- **Nothing is at `$script:` scope.** The header relaunches via `powershell -Command "& <block>"`,
  so the whole program runs inside one invoked script block; a `$script:`-qualified name resolves
  to `$null`. The window's timer tick must reference `$child`, `$timer`, `$form`, and
  `$progressFile` unqualified. Use `System.Drawing.Color` (not `System.Windows.Forms.Color`) for
  colors, and a WinForms `Timer` is a Component, so never add it to `Form.Controls`.
- **The whole file is CRLF.** `.gitattributes` forces `*.cmd eol=crlf` and the `mixed-line-ending`
  pre-commit hook rejects mixed endings, so the batch header and the PowerShell body must share
  one ending. Author and save it CRLF throughout.
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
- The delivery archive holds the collector and instructions only: no tests, no exports.

## Work Guidance

- The Windows VM is a test fixture only. Its Windows 11 details say nothing about the tablet.
- Never run the collector against a physical tablet from here.

## Verification

- `tests/test_delivery-archive.sh`: builds the archive and asserts it holds exactly the
  collector and the instructions. Needs no VM; run it after changing `scripts/build-delivery`.
- `tests/integration/test_collector-headless.sh` (needs the VM; run by hand, since
  `scripts/integration` finds no project manifest to dispatch to). It stages the
  script in a fresh `/home/neal/windows-data/bsttest/r<epoch>/` folder (guest
  `\\host.lan\data\bsttest\…`), types one command into the guest's focused admin PowerShell
  window through the QEMU monitor (no guest SSH; guest-run.ps1 launches the collector via
  `cmd /c`, the real double-tap path), and inspects the returned ZIPs: zero exit,
  valid archive, every stage, SQL rows, registry export, `SYSTEM-INFO.txt` fields, and a
  non-zero exit with `[FAIL] 2. Database` when the schema folder is hidden. Both run headless
  and in the default window mode (the window must close by itself). About 10 minutes.
- Fixture precondition: container `windows` on `server` running, a synthetic install at
  `C:\braintest` (embedded MySQL 5.5 `mysqld.exe`; tables `person`=3, `session`=4,
  `flowdesign`=2, `montagesystem`=3 rows), and an admin PowerShell window focused on the guest
  desktop. The fixture setup is not tracked here.
- The window's contents have no automated check: the VM test only proves the window mode
  completes and closes. To see it, run `Collect-BRAINtellect2-Data.cmd` without `BST_NO_UI` on
  the guest desktop and watch it.

## Child Index

None.
