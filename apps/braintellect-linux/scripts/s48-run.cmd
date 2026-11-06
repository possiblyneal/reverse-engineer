@echo off
rem Guest side of the replay rig (docs/reverse-engineering/braintellect2/replay-rig.md).
rem Spawns the original under Frida with the run-8 hook set. The B-2 is the emulator on COM2
rem (preference B-2-BTCOMPort=COM2); nothing here touches a real B-2 or Bluetooth.
rem Expects the volume labelled CAP to hold frida-inject.exe and hook11.js; hook11.js writes C:\cap\run8.bin.
for /f %%d in ('powershell -NoProfile -Command "(Get-Volume -FileSystemLabel CAP).DriveLetter"') do set O=%%d:
if not exist C:\cap mkdir C:\cap
set PATH=C:\bsttest\lib;%PATH%
cd /d C:\bsttest\bin
%O%\frida-inject.exe -f C:\bsttest\bin\braintellect.exe -s %O%\hook11.js > C:\cap\fi11.out 2>&1
