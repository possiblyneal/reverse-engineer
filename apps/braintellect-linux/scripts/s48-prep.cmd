@echo off
rem s48: list serial ports, back up the preference file, point B-2-BTCOMPort at COM2.
rem Output goes to the volume labelled OUT (a FAT image attached to the VM).
for /f %%d in ('powershell -NoProfile -Command "(Get-Volume -FileSystemLabel OUT).DriveLetter"') do set O=%%d:
set OUT=%O%\prep.txt
set INI=C:\ProgramData\BST\BRAINtellect2\preferences\braintellect.ini
echo === PORTS === > "%OUT%"
powershell -NoProfile -Command "Get-PnpDevice -Class Ports | Select-Object Status,Present,InstanceId,FriendlyName | Format-Table -Auto | Out-String -Width 200" >> "%OUT%" 2>&1
powershell -NoProfile -Command "[System.IO.Ports.SerialPort]::GetPortNames()" >> "%OUT%" 2>&1
echo === INI BEFORE === >> "%OUT%"
type "%INI%" >> "%OUT%"
if not exist "%INI%.bak-s48" copy "%INI%" "%INI%.bak-s48" >> "%OUT%"
powershell -NoProfile -Command "(Get-Content '%INI%') -replace 'B-2-BTCOMPort=.*','B-2-BTCOMPort=COM2' | Set-Content '%INI%'" >> "%OUT%" 2>&1
echo === INI AFTER === >> "%OUT%"
type "%INI%" >> "%OUT%"
echo DONE >> "%OUT%"
