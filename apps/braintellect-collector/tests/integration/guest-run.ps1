# Guest half of test_collector-headless.sh. Runs inside the Windows VM from the
# shared run folder next to Collect-BRAINtellect2-Data.cmd (via $PSScriptRoot) and leaves its
# evidence there: guest.log (exit codes), <case>.zip and <case>.out per case.
#
# The synthetic BRAINtellect install at C:\braintest is a precondition.
$share = $PSScriptRoot
$log = Join-Path $share 'guest.log'
function L($m){ Add-Content -Path $log -Value $m -Encoding UTF8 }
$ErrorActionPreference = 'Continue'
$desktop = [Environment]::GetFolderPath('Desktop')

function Invoke-Collector($case, $withWindow = $false){
  Get-ChildItem $desktop -Filter 'BST-full-export-*.zip' -ErrorAction SilentlyContinue | Remove-Item -Force
  $before = @(Get-ChildItem $env:TEMP -Filter 'bst*' -ErrorAction SilentlyContinue | ForEach-Object { $_.FullName })
  if($withWindow){ Remove-Item Env:BST_NO_UI -ErrorAction SilentlyContinue } else { $env:BST_NO_UI = '1' }
  # Mirror the real owner flow: the file lives on the Desktop (local disk) and is
  # launched through cmd, exactly as a double-tap does -- cmd cannot use the UNC
  # share as a working directory, and neither would a tablet. BST_NO_UI still
  # toggles window vs headless.
  $cmd = Join-Path $desktop 'Collect-BRAINtellect2-Data.cmd'
  Copy-Item (Join-Path $share 'Collect-BRAINtellect2-Data.cmd') $cmd -Force
  try {
    & $env:ComSpec /c "`"$cmd`"" *>&1 |
      Out-File -FilePath (Join-Path $share "$case.out") -Encoding utf8
  } finally { Remove-Item $cmd -Force -ErrorAction SilentlyContinue }
  L "$case exit=$LASTEXITCODE"
  $zip = Get-ChildItem $desktop -Filter 'BST-full-export-*.zip' -ErrorAction SilentlyContinue | Select-Object -First 1
  if($zip){ Copy-Item $zip.FullName (Join-Path $share "$case.zip") -Force }
  $after = @(Get-ChildItem $env:TEMP -Filter 'bst*' -ErrorAction SilentlyContinue | ForEach-Object { $_.FullName })
  L "$case temp leftovers=$(@($after | Where-Object { $before -notcontains $_ }).Count)"
}

# Registry keys the collector must export, as the real install writes them.
reg add 'HKCU\Software\BST\BRAINtellect2' /v 'license.expiry' /t REG_SZ /d '2099-12-31' /f | Out-Null
reg add 'HKLM\Software\Brain State Technologies\BRAINtellect2' /v 'InstallPath' /t REG_SZ /d 'C:\braintest\BRAINtellect2' /f | Out-Null

Invoke-Collector 'ok'

# The default (window) mode: the parent shows the window and a child collects.
# It must end by itself (the window closes), exit zero and leave a valid ZIP.
Invoke-Collector 'ui' $true

# A missing database must fail the calling process, not just the log. The
# fixture schema folder is hidden for the run and always put back.
$schema = 'C:\braintest\BRAINtellect2\data\braintellectds'
Rename-Item $schema 'braintellectds.hidden'
try {
  Invoke-Collector 'fail'
  Invoke-Collector 'uifail' $true
} finally { Rename-Item "$schema.hidden" 'braintellectds' }

L 'DONE'
