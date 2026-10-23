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
$env:BST_SEARCH_ROOTS = 'C:\braintest'
$fixtureRoot = 'C:\braintest\BRAINtellect2'
$programDataRoot = Join-Path ([Environment]::GetFolderPath('CommonApplicationData')) 'BST'

function Invoke-Collector($case, $withWindow = $false, $retryOnly = $false){
  Get-ChildItem $desktop -Filter 'BST-*-*.zip' -ErrorAction SilentlyContinue | Remove-Item -Force
  $before = @(Get-ChildItem $env:TEMP -Filter 'bst*' -ErrorAction SilentlyContinue | ForEach-Object { $_.FullName })
  if($withWindow){ Remove-Item Env:BST_NO_UI -ErrorAction SilentlyContinue } else { $env:BST_NO_UI = '1' }
  if($retryOnly){ $env:BST_RETRY_ONLY = '1' } else { Remove-Item Env:BST_RETRY_ONLY -ErrorAction SilentlyContinue }
  $env:BST_SEARCH_ROOTS = 'C:\braintest'
  L "$case retry=$env:BST_RETRY_ONLY search=$env:BST_SEARCH_ROOTS"
  # Mirror the real owner flow: the file lives on the Desktop (local disk) and is
  # launched through cmd, exactly as a double-tap does -- cmd cannot use the UNC
  # share as a working directory, and neither would a tablet. BST_NO_UI still
  # toggles window vs headless.
  $cmd = Join-Path $desktop 'Collect-BRAINtellect2-Data.cmd'
  Copy-Item (Join-Path $share 'Collect-BRAINtellect2-Data.cmd') $cmd -Force
  try {
    & $env:ComSpec /c "`"$cmd`"" *>&1 |
      Out-File -FilePath (Join-Path $share "$case.out") -Encoding utf8
    $exitCode = $LASTEXITCODE
  } finally { Remove-Item $cmd -Force -ErrorAction SilentlyContinue }
  L "$case exit=$exitCode"
  $zip = Get-ChildItem $desktop -Filter 'BST-*-*.zip' -ErrorAction SilentlyContinue | Select-Object -First 1
  if($zip){ Copy-Item $zip.FullName (Join-Path $share "$case.zip") -Force }
  $after = @(Get-ChildItem $env:TEMP -Filter 'bst*' -ErrorAction SilentlyContinue | ForEach-Object { $_.FullName })
  L "$case temp leftovers=$(@($after | Where-Object { $before -notcontains $_ }).Count)"
}

# Registry keys and ProgramData state the collector must export, as the real install writes them.
reg add 'HKCU\Software\BST\BRAINtellect2' /v 'license.expiry' /t REG_SZ /d '2099-12-31' /f | Out-Null
reg add 'HKLM\Software\Brain State Technologies\BRAINtellect2' /v 'InstallPath' /t REG_SZ /d $fixtureRoot /f | Out-Null
$syntheticState = Join-Path ([Environment]::GetFolderPath('CommonApplicationData')) 'BST\BRAINtellect2'

# The restored VM may contain a real ProgramData installation. Hide it for this
# synthetic-fixture test so collector precedence and assertions are deterministic.
$parkedProgramData = $null
if(Test-Path $programDataRoot){
  $parkedProgramData = "$programDataRoot.collector-test-hidden"
  if(Test-Path $parkedProgramData){ Remove-Item $parkedProgramData -Recurse -Force }
  Rename-Item $programDataRoot (Split-Path $parkedProgramData -Leaf)
}
New-Item -ItemType Directory -Force -Path $syntheticState | Out-Null
Set-Content -Path (Join-Path $syntheticState 'connection.ini') -Value 'fixture-state=1' -Encoding ASCII
try {
Remove-Item (Join-Path $share 'guest.log') -Force -ErrorAction SilentlyContinue
Invoke-Collector 'ok'

# Retry mode must collect the database and state without duplicating the install.
Invoke-Collector 'retry' $false $true

# The default (window) mode: the parent shows the window and a child collects.
# It must end by itself (the window closes), exit zero and leave a valid ZIP.
Invoke-Collector 'ui' $true

# A missing MySQL datadir must fail the calling process, not just the log. Hide
# its InnoDB marker for each run, then restore it before the next case.
$marker = 'C:\braintest\BRAINtellect2\state\embedded-store\ibdata1'
Rename-Item $marker 'ibdata1.hidden'
try {
  $env:BST_SEARCH_ROOTS = 'C:\braintest\BRAINtellect2'
  Invoke-Collector 'fail'
} finally { Rename-Item "$marker.hidden" 'ibdata1' }
Rename-Item $marker 'ibdata1.hidden'
try {
  $env:BST_SEARCH_ROOTS = 'C:\braintest\BRAINtellect2'
  Invoke-Collector 'uifail' $true
} finally { Rename-Item "$marker.hidden" 'ibdata1' }
} finally {
  Remove-Item $programDataRoot -Recurse -Force -ErrorAction SilentlyContinue
  if($parkedProgramData -and (Test-Path $parkedProgramData)){
    Rename-Item $parkedProgramData (Split-Path $programDataRoot -Leaf)
  }
}

L 'DONE'
