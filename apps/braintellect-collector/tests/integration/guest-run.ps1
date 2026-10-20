# Guest half of test_collector-headless.sh. Runs inside the Windows VM from the
# shared run folder next to BST-collect.ps1 (via $PSScriptRoot) and leaves its
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
  & powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $share 'BST-collect.ps1') *>&1 |
    Out-File -FilePath (Join-Path $share "$case.out") -Encoding utf8
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
