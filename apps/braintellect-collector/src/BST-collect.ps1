# BST-collect.ps1
# Grab EVERYTHING about BRAINtellect2 from this tablet in one go -- into a single
# file on your Desktop that you can email. This is the "get it all" version:
#   1. the program/install folder(s)
#   2. the database (raw data files + a readable SQL dump of every table)
#   3. the registry keys that mention BRAINtellect / BST
#
# Safe to read: the database is read from a PRIVATE COPY on the local disk. The
# live database and the live program are never opened for writing, modified, or
# touched. It sends nothing on its own -- you email the resulting file yourself.
# When you're done, you delete the private data OUT OF THIS FILE on your own
# machine.
#
# Installs nothing: it never runs an installer or writes anywhere except the temp
# folder and the Desktop ZIP. The database is read from a temporary copy that is
# removed when the run ends, so the only file left behind is the export ZIP.
#
# Discovers the important paths by searching, so it does not need to know exactly
# where BRAINtellect2 is installed on this machine.
#
# Parameters:
#   -ShowUI   Display a live progress window (default: on). The window runs here;
#             the collection itself runs in a child PowerShell process that this
#             window watches. Pass -ShowUI:$false to run headless / unattended (the
#             window is skipped, but the per-section log is still written to the
#             export).

[CmdletBinding()]
param(
  [switch] $ShowUI = $true
)

# Headless override via environment variable (inherited by child processes), which
# is robust across process boundaries where a -ShowUI:$false switch can get mangled.
# Set BST_NO_UI=1 to skip the progress window.
if($env:BST_NO_UI -eq '1'){ $ShowUI = $false }

$ErrorActionPreference = 'Stop'

$db = 'braintellectds'

# Force an array so `foreach` iterates drives, not characters (a single-drive
# machine returns a scalar string from ExpandProperty Root).
$drives = @(Get-PSDrive -PSProvider FileSystem | Select-Object -ExpandProperty Root)

# Helpers: search, skipping our own temp folder (which is under $env:TEMP).
function Find-Dir($name){
  foreach($d in $drives){
    $hit = Get-ChildItem -Path $d -Recurse -Directory -Filter $name -ErrorAction SilentlyContinue |
           Where-Object { $_.FullName -notmatch '\\bstcollect_\d+(\\|$)' } | Select-Object -First 1
    if($hit){ return $hit.FullName }
  }
  return $null
}
function Find-Exe($name, $preferredRoots){
  $roots = @()
  if($preferredRoots){ $roots += $preferredRoots }   # prefer the app's bundled engine
  $roots += $drives                         # then fall back to any drive
  foreach($r in $roots){
    $hit = Get-ChildItem -Path $r -Recurse -Filter $name -ErrorAction SilentlyContinue | Select-Object -First 1
    if($hit){ return $hit.FullName }
  }
  return $null
}

# ---- log (the work area is created later, only in the process that collects) ----
$work = $null
$logPath = $null
# Every status line is written both to the screen and to the log in the work area,
# which is included in the zip so any error or partial result is visible there.
function Say($m){
  Write-Host $m
  # Tolerant: keep writing to the screen when there is no log yet or it is removed.
  if($logPath -and (Test-Path $logPath)){ Add-Content -Path $logPath -Value $m }
}

$desktop = [Environment]::GetFolderPath('Desktop')
$outZip  = Join-Path $desktop ('BST-full-export-' + $env:COMPUTERNAME + '.zip')

$proc = $null   # our private mysqld, tracked so cleanup can target only us

# ---- per-section outcome tracking (so the log records success OR failure) ----
$sectionStatus = [ordered]@{
  '0. System information' = 'pending'
  '1. Install folder(s)'  = 'pending'
  '2. Database'           = 'pending'
  '3. Registry'           = 'pending'
}
function Get-StatusLabel($status){
  switch($status){ 'ok'{'[OK]'} 'fail'{'[FAIL]'} 'skip'{'[SKIP]'} 'partial'{'[PARTIAL]'} default{'[? ]'} }
}
function Set-SectionStatus($name, $status){
  $sectionStatus[$name] = $status
  $label = Get-StatusLabel $status
  Say ("   $label " + $name + ' --- done')
}
# Writes the per-section results table to the log. Called before the zip so it is
# captured in the export even when a section failed.
function Write-SectionSummary{
  Say ""
  Say "=== SECTION RESULTS ==="
  foreach($kv in $sectionStatus.GetEnumerator()){
    $label = Get-StatusLabel $kv.Value
    Say ("   $label " + $kv.Key)
  }
}

# ---- progress reporting (drives the window AND the log) ----
$uiState = [ordered]@{ Section=0; Total=4; Status='Starting...'; Pct=0; Marquee=$true; Elapsed='0:00'; ETA='---' }
$startTime = Get-Date
function Format-Dur($ts){ '{0:d1}:{1:00}' -f [int]$ts.TotalMinutes, $ts.Seconds }
# Called by the collection. Logs progress and, when a progress window watches this
# run, hands the update over in BST_PROGRESS_FILE ("pct|marquee|status"). Never
# throws -- progress is best-effort.
$reportProgress = {
  param($Section,$Total,$Status,$Pct,$Marquee)
  try{
    Say ("  ... " + $Status + " (" + $Pct.ToString() + "%)")
    if($env:BST_PROGRESS_FILE){
      Set-Content -Path $env:BST_PROGRESS_FILE -Value ("{0}|{1}|{2}" -f $Pct,[int][bool]$Marquee,$Status)
    }
  }catch{ }
}

# ---- progress window (optional) ----
$form = $null
$timer = $null
if($ShowUI){
  try{
    Add-Type -AssemblyName System.Windows.Forms
    $form = New-Object System.Windows.Forms.Form
    $form.Text = 'BRAINtellect2 -- Collecting data'
    $form.Size = New-Object System.Drawing.Size(540,200)
    $form.StartPosition = 'CenterScreen'
    $form.FormBorderStyle = 'FixedDialog'
    $form.MaximizeBox = $false
    $form.MinimizeBox = $false
    $form.ControlBox = $false   # only the end of the collection closes it
    $form.BackColor = [System.Windows.Forms.Color]::White

    $title  = New-Object System.Windows.Forms.Label
    $title.Location = '20,18'; $title.Size = '500,20'
    $title.Text = 'BRAINtellect2 -- gathering everything from this tablet...'

    $statusLabel = New-Object System.Windows.Forms.Label
    $statusLabel.Location = '20,48'; $statusLabel.Size = '500,20'
    $statusLabel.Text = $uiState.Status

    $progressBar = New-Object System.Windows.Forms.ProgressBar
    $progressBar.Location = '20,80'; $progressBar.Size = '500,25'
    $progressBar.Style = 'Marquee'   # starts moving immediately

    $meta = New-Object System.Windows.Forms.Label
    $meta.Location = '20,118'; $meta.Size = '500,40'
    $meta.Text = ('Percent: {0}%   |   Elapsed: {1}   |   Est. time left: {2}' -f $uiState.Pct,$uiState.Elapsed,$uiState.ETA)

    $hint = New-Object System.Windows.Forms.Label
    $hint.Location = '20,165'; $hint.Size = '500,25'
    $hint.Text = 'This is working -- please leave this window open until it finishes.'

    $form.Controls.AddRange(@($title,$statusLabel,$progressBar,$meta,$hint))

    # Paints the controls from the shared state. Runs on the UI thread (from the timer).
    $updateUI = {
      $statusLabel.Text = $uiState.Status
      $progressBar.Value = [Math]::Min(100,[Math]::Max(0,[int]$uiState.Pct))
      $progressBar.Style = if($uiState.Marquee){[System.Windows.Forms.ProgressBarStyle]::Marquee}else{[System.Windows.Forms.ProgressBarStyle]::Blocks}
      $meta.Text = ('Percent: {0}%   |   Elapsed: {1}   |   Est. time left: {2}' -f $uiState.Pct,$uiState.Elapsed,$uiState.ETA)
    }

    # A timer, on the UI thread, keeps the window moving while the collection runs in
    # the child process: it takes the child's latest progress, refreshes elapsed time,
    # recomputes the estimated time left, and closes the window when the child ends.
    $timer = New-Object System.Windows.Forms.Timer
    $timer.Interval = 500
    $timer.add_Tick({
      try{
        if($script:progressFile -and (Test-Path $script:progressFile)){
          $parts = (Get-Content -Path $script:progressFile -TotalCount 1 -ErrorAction Stop) -split '\|', 3
          if($parts.Count -eq 3){
            $uiState.Pct = [int]$parts[0]; $uiState.Marquee = ($parts[1] -eq '1'); $uiState.Status = $parts[2]
          }
        }
      }catch{ }   # the child may be mid-write; the next tick reads it
      $uiState.Elapsed = Format-Dur ((Get-Date) - $startTime)
      if($uiState.Pct -gt 0 -and $uiState.Pct -lt 100){
        $elapsedSec = ((Get-Date) - $startTime).TotalSeconds
        $totalSec = $elapsedSec / ($uiState.Pct/100)
        $uiState.ETA = Format-Dur ([timespan]::FromSeconds([Math]::Max(0, $totalSec - $elapsedSec)))
      } else { $uiState.ETA = '---' }
      $updateUI.Invoke()
      if($script:child.HasExited){ $script:timer.Stop(); $script:form.Close() }
    })
    $form.Controls.Add($timer)
  }catch{
    # If the window can't be shown, keep going without it (logging still works).
    $ShowUI = $false
    $form = $null
    Say ("   (progress window unavailable: " + $_.Exception.Message + " -- continuing)")
  }
}

# Final user-facing messages (screen + STATUS.txt when this process has one).
function Write-Finish{
  Say ""
  Say "================================================================"
  Say "DONE. Created this file on your Desktop:"
  Say ("    " + $outZip)
  if(Test-Path $outZip){ Say ("Archive size: " + [math]::Round(((Get-Item $outZip).Length / 1MB), 1) + " MB") }
  Say ""
  Say "It contains: system information, install folder(s), the database"
  Say "(raw files + SQL), and registry keys. This is EVERYTHING BRAINtellect2"
  Say "on this tablet."
  Say "The tablet itself was never changed and nothing was installed."
  Say ""
  Say "The private data (client, person, session, assessment, results) is"
  Say "inside it -- delete those parts OUT OF THIS FILE on your own machine"
  Say "when you're done."
  Say ""
  Say "Please email that one file to Neal (nealheinecke@gmail.com)."
  Say "If it is too big to email, upload it to Google Drive / WeTransfer"
  Say "and send Neal the link."
  Say "================================================================"
}

# ---- with a window: this process only shows it; a child PowerShell collects ----
if($ShowUI -and $form){
  $progressFile = Join-Path $env:TEMP ('bstprogress_' + $PID + '.txt')
  $childOut     = Join-Path $env:TEMP ('bstchild_' + $PID + '.out')
  $childErr     = Join-Path $env:TEMP ('bstchild_' + $PID + '.err')
  $env:BST_NO_UI = '1'                       # the child runs headless, synchronously
  $env:BST_PROGRESS_FILE = $progressFile
  try{
    $child = Start-Process -FilePath 'powershell' -WindowStyle Hidden -PassThru `
      -RedirectStandardOutput $childOut -RedirectStandardError $childErr `
      -ArgumentList @('-NoProfile','-ExecutionPolicy','Bypass','-File',('"' + $PSCommandPath + '"'))
    $null = $child.Handle                    # keeps ExitCode readable after it exits
    $timer.Start()
    [System.Windows.Forms.Application]::Run($form)
    $timer.Stop()
    $child.WaitForExit()
    $childExit = $child.ExitCode
    if($childExit -ne 0){
      foreach($f in @($childOut,$childErr)){
        if(Test-Path $f){ Get-Content $f -Tail 15 -ErrorAction SilentlyContinue | ForEach-Object { Say ("   " + $_) } }
      }
      Say ""
      Say "The collection FAILED (exit $childExit). If a file was created on your Desktop, send it to Neal anyway."
    }
  }finally{
    Remove-Item $progressFile,$childOut,$childErr -Force -ErrorAction SilentlyContinue
  }
  if($childExit -ne 0){ exit $childExit }
  Write-Finish
  exit 0
}

# ---- work area + log (temp; removed when done; only the Desktop zip survives) ----
$work = Join-Path $env:TEMP ('bstcollect_' + $PID)
$null = New-Item -ItemType Directory -Force -Path $work
$logPath = Join-Path $work 'STATUS.txt'
$null = New-Item -ItemType File -Force -Path $logPath
Say ("=== BST FULL EXPORT  " + (Get-Date -Format 'yyyy-MM-dd HH:mm:ss') + "  ===")
Say ("PID=$PID  COMPUTER=$env:COMPUTERNAME  TEMP=$work")

# =====================================================================
# COLLECTION  -- runs synchronously in the process that collects. With a window,
# the parent runs it in a child PowerShell process (headless), so a failure is that
# process's exit status.
# =====================================================================
$collection = {
  try{
  # =====================================================================
  # 0. SYSTEM INFORMATION  -- identify the tablet: OS version/build + model
  # =====================================================================
  # The tablet owner can't be reached, so this records the machine's identity
  # (OS version/build/edition and the tablet/laptop model + serial) into the
  # export. Every call is guarded -- a failure here must never abort the run.
  Say "Step 0/4: recording system information..."
  & $reportProgress 0 4 'Collecting system information...' 20 $false
  $sysLog = Join-Path $work 'SYSTEM-INFO.txt'
  $sys = @()
  try{
    $os = Get-CimInstance Win32_OperatingSystem
    $cv = Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion' -EA SilentlyContinue
    $sys += "=== OPERATING SYSTEM ==="
    $sys += "OS (Caption): " + $os.Caption
    $sys += "Version:     " + $os.Version
    $sys += "Build:       " + $cv.CurrentBuildNumber + " (SP=" + $cv.CSDVersion + ")"
    $sys += "ReleaseId:   " + $cv.ReleaseId
    $sys += "Edition:     " + $cv.ProductName
    $sys += "Install:     " + $os.InstallDate
    $sys += "Last boot:   " + $os.LastBootUpTime
    $sys += "Arch:        " + $os.OSArchitecture
  }catch{ $sys += "OS capture failed: " + $_.Exception.Message }
  try{
    $cs   = Get-CimInstance Win32_ComputerSystem
    $cpu  = Get-CimInstance Win32_Processor
    $bios = Get-CimInstance Win32_BIOS
    $disk = Get-CimInstance Win32_DiskDrive
    $sys += ""
    $sys += "=== HARDWARE ==="
    $sys += "Manufacturer: " + $cs.Manufacturer
    $sys += "Model:        " + $cs.Model
    $sys += "Serial:       " + $bios.SerialNumber
    $sys += "CPU:          " + $cpu.Name
    $sys += "RAM (GB):     " + ([math]::Round($cs.TotalPhysicalMemory / 1GB, 1))
    $sys += "BIOS:         " + $bios.Manufacturer + " " + $bios.Version
    $sys += "Disks:        " + (($disk | ForEach-Object { $_.Model + " " + [math]::Round($_.Size/1GB,0) + "GB" }) -join ', ')
  }catch{ $sys += "Hardware capture failed: " + $_.Exception.Message }
  try{
    # Dependencies the tool actually needs. Captured raw (not friendly-mapped) so an
    # old machine's build number is unambiguous. $PSVersionTable exists since PS 2.0.
    $dn = (Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\NET Framework Setup\NDP\V4\Full' -EA SilentlyContinue).Release
    if(-not $dn){ $dn = (Get-ItemProperty 'HKLM:\SOFTWARE\WOW6432Node\Microsoft\NET Framework Setup\NDP\V4\Full' -EA SilentlyContinue).Release }
    if(-not $dn){ $dn = 'not found' }
    $sys += ""
    $sys += "=== SOFTWARE / DEPENDENCIES ==="
    $psVersion = $PSVersionTable.PSVersion
    $cim = Get-Command Get-CimInstance -ErrorAction SilentlyContinue
    $reg = Get-Command reg.exe -ErrorAction SilentlyContinue
    try{ Add-Type -AssemblyName System.IO.Compression.FileSystem -ErrorAction Stop; $zipApi = 'available' }catch{ $zipApi = 'unavailable: ' + $_.Exception.Message }
    $sys += "PowerShell:    " + $psVersion.ToString() + " (required: 3.0+)"
    $sys += ".NET Release:  " + $dn + " (required: 4.5+ for ZIP support)"
    $sys += "CIM cmdlet:    " + $(if($cim){ 'available' }else{ 'MISSING' })
    $sys += "reg.exe:       " + $(if($reg){ $reg.Path }else{ 'MISSING' })
    $sys += "ZIP API:       " + $zipApi
    $sys += "Arch:          " + $os.OSArchitecture
  }catch{ $sys += "Dependency capture failed: " + $_.Exception.Message }
  $sys | Set-Content -Path $sysLog -Encoding UTF8
  Set-SectionStatus '0. System information' 'ok'

  # =====================================================================
  # 1. INSTALL FOLDER(S)  -- BRAINtellect2 / BST folders anywhere
  # =====================================================================
  Say "Step 1/4: locating the BRAINtellect program folder(s)..."
  & $reportProgress 1 4 'Locating install folder(s)...' 40 $false
  $nameHits = @(Find-Dir 'BRAINtellect2')
  $nameHits += @(Find-Dir 'BST')
  $instRoots = $nameHits | Sort-Object -Unique
  if(-not $instRoots){
    Say "   (no BRAINtellect2/BST folder found -- skipping install capture)"
    Set-SectionStatus '1. Install folder(s)' 'skip'
  }else{
    $instOut = Join-Path $work 'install'
    $instStatus = 'ok'
    foreach($r in $instRoots){
      Say ("   found: " + $r)
      Copy-Item $r (Join-Path $instOut (Split-Path $r -Leaf)) -Recurse -Force -ErrorAction SilentlyContinue -ErrorVariable copyErrors
      if($copyErrors){
        $instStatus = 'partial'
        Say ("   WARNING: " + @($copyErrors).Count + " item(s) in $r could not be copied:")
        @($copyErrors) | Select-Object -First 10 | ForEach-Object { Say ("      " + $_.Exception.Message) }
      }
    }
    Say ("   install captured under: $instOut")
    Set-SectionStatus '1. Install folder(s)' $instStatus
  }

  # =====================================================================
  # 2. DATABASE  -- find datadir, dump every table, keep raw files too
  # =====================================================================
  Say "Step 2/4: locating the BRAINtellect database..."
  & $reportProgress 2 4 'Dumping database...' 60 $false
  $braintellectds = Find-Dir 'braintellectds'
  if(-not $braintellectds){ throw "Could not find the BRAINtellect database (no 'braintellectds' folder). Is the software installed on this tablet?" }
  # The MySQL datadir is the PARENT of the per-schema folder (e.g.
  # ...\<datadir>\braintellectds\).
  $dataDir = Split-Path $braintellectds -Parent
  Say ("   database data folder: " + $dataDir)

  # copy the raw data files (belt-and-suspenders; the SQL dump is the readable copy)
  $dataOut = Join-Path $work 'database'
  $dbStatus = 'ok'
  Copy-Item $dataDir $dataOut -Recurse -Force -ErrorAction SilentlyContinue -ErrorVariable copyErrors
  if($copyErrors){
    $dbStatus = 'partial'
    Say ("   WARNING: " + @($copyErrors).Count + " database file(s) could not be copied:")
    @($copyErrors) | Select-Object -First 10 | ForEach-Object { Say ("      " + $_.Exception.Message) }
  }
  Get-ChildItem $dataOut -Filter '*.pid' -Recurse -ErrorAction SilentlyContinue | Remove-Item -Force -ErrorAction SilentlyContinue
  Get-ChildItem $dataOut -Filter '*.err' -Recurse -ErrorAction SilentlyContinue | Remove-Item -Force -ErrorAction SilentlyContinue

  # find the bundled engine so we can read the copy privately
  $mysqld    = Find-Exe 'mysqld.exe' $instRoots
  $mysqldump = Find-Exe 'mysqldump.exe' $instRoots
  $mysqlcli  = Find-Exe 'mysql.exe' $instRoots
  if(-not $mysqld){
    Say "   (no mysqld.exe found on this tablet -- skipping SQL dump, raw files kept)"
    try{ Add-Content -Path (Join-Path $work 'SYSTEM-INFO.txt') -Encoding UTF8 "MySQL engine:   NOT FOUND" }catch{ }
    $dbStatus = 'partial'
  }else{
    Say ("   engine found: " + $mysqld)
    try{ Add-Content -Path (Join-Path $work 'SYSTEM-INFO.txt') -Encoding UTF8 ("MySQL engine:   " + $mysqld + " (version " + (Get-Item $mysqld).VersionInfo.ProductVersion + ")") }catch{ }
    # start a private read-only copy on a named pipe; live DB untouched. The pipe
    # name carries our PID so it can never collide with the live server or another run.
    $pipe = 'bstcollectpipe' + $PID
    $basedir = Split-Path (Split-Path $mysqld -Parent) -Parent

    # --no-defaults must come first: it keeps mysqld from reading any my.ini/my.cnf,
    # whose settings could point it back at the live data or log files.
    $mysqlArgs = @('--no-defaults',"--datadir=$dataOut","--basedir=$basedir",
          '--skip-networking','--enable-named-pipe',"--socket=$pipe",
          '--skip-grant-tables','--innodb-force-recovery=1')
    $errmsg = Find-Exe 'errmsg.sys' $instRoots
    if($errmsg){ $mysqlArgs += ("--lc-messages-dir=" + (Split-Path $errmsg -Parent)) }
    $proc = Start-Process -FilePath $mysqld -ArgumentList $mysqlArgs -WindowStyle Hidden -PassThru
    Start-Sleep -Seconds 2

    $up = $false
    # Native stderr becomes a terminating error under Stop; the client calls below
    # are judged by their exit codes instead.
    $ErrorActionPreference = 'Continue'
    for($i=0;$i -lt 60;$i++){
      Start-Sleep -Seconds 2
      if($mysqlcli){
        $probe = & $mysqlcli --user=root --protocol=pipe --socket=$pipe -N -e "SELECT 1" 2>$null
        if("$probe" -match '1'){ $up = $true; break }
      }else{ $up = $true; break }
    }
    if(-not $up){
      Say "   WARNING: database copy did not start cleanly. Keeping raw files; SQL dump may be partial."
      Say "   Engine error log (send this to Neal):"
      Get-ChildItem $dataOut -Filter '*.err' -Recurse -ErrorAction SilentlyContinue |
        ForEach-Object { Get-Content $_.FullName -ErrorAction SilentlyContinue | Select-Object -Last 15 } | ForEach-Object { Say ("      " + $_) }
      $dbStatus = 'partial'
    }else{
      # dump the ENTIRE schema (every table, every row) as UTF-8 (not ASCII).
      $sqlOut = Join-Path $dataOut 'BST-full-export.sql'
      Say "   dumping every table to SQL..."
      # mysqldump writes the file itself (--result-file): a PowerShell pipeline would
      # re-encode its UTF-8 bytes.
      if($mysqldump){
        & $mysqldump --user=root --protocol=pipe --socket=$pipe --skip-lock-tables --hex-blob --routines --default-character-set=utf8 "--result-file=$sqlOut" $db 2>$null
        $dumpExit = $LASTEXITCODE
      }else{
        Say "   (no mysqldump.exe found on this tablet)"
        $dumpExit = -1
      }
      if($dumpExit -ne 0){
        Say "   WARNING: mysqldump did not finish (exit $dumpExit); the SQL dump may be incomplete. Raw data files are still included in the export."
        $dbStatus = 'partial'
      }
      if(-not ((Test-Path $sqlOut) -and ((Get-Item $sqlOut).Length -gt 0))){
        Say "   WARNING: the SQL dump came back empty. Raw data files are still included in the export."
        $dbStatus = 'partial'
      }else{
        # report row counts per table so private data is obvious at a glance.
        # @() forces an array even when the schema has a single table.
        if($mysqlcli){
          $tables = @(& $mysqlcli --user=root --protocol=pipe --socket=$pipe -N -e "SELECT table_name FROM information_schema.tables WHERE table_schema='$db' ORDER BY table_name" 2>$null)
          $total = 0
          foreach($t in $tables){
            $quoted = '`' + $db + '`.`' + ("$t".Replace('`','``')) + '`'
            $c = & $mysqlcli --user=root --protocol=pipe --socket=$pipe -N -e "SELECT COUNT(*) FROM $quoted" 2>$null
            $n = 0
            if($LASTEXITCODE -ne 0 -or -not [int]::TryParse(("$c").Trim(), [ref]$n)){
              Say ("      {0,-24} ? (count failed)" -f $t)
              $dbStatus = 'partial'
            }else{
              $total += $n
              Say ("      {0,-24} {1}" -f $t, $n)
            }
          }
          Say ("      {0,-24} {1} (grand total)" -f 'ALL', $total)
        }
      }
    }
  }
  $ErrorActionPreference = 'Stop'
  Set-SectionStatus '2. Database' $dbStatus

  # =====================================================================
  # 3. REGISTRY  -- any key mentioning BRAINtellect / BST
  # =====================================================================
  Say "Step 3/4: exporting relevant registry keys..."
  # The registry sweep is the slow, unpredictable part, so keep the window in
  # marquee mode (constantly moving) until it is done.
  & $reportProgress 3 4 'Scanning registry (this can take a few minutes)...' 70 $true
  $regOut = Join-Path $work 'registry'
  $null = New-Item -ItemType Directory -Force -Path $regOut
  $regHives = @('HKCU:\Software','HKLM:\Software','HKCU:\Software\Wow6432Node','HKLM:\Software\Wow6432Node')
  $exported = 0
  # reg.exe writes to stderr on any failure; under ErrorActionPreference=Stop that
  # would abort the whole export, so run this section as Continue (as the DB probe does).
  $prevEAP = $ErrorActionPreference; $ErrorActionPreference = 'Continue'
  foreach($hive in $regHives){
    if(-not (Test-Path $hive)){ continue }
    Get-ChildItem -Path $hive -Recurse -ErrorAction SilentlyContinue |
      Where-Object { $_.Name -match '(?i)(\\BST($|\\)|BRAINtellect)' } |
      ForEach-Object {
        # reg.exe needs the plain key path (HKEY_...\...), not the provider-qualified
        # PSPath. Use the full Name as the filename too, so keys that share a leaf name
        # never overwrite each other.
        $safe = ($_.Name -replace '[\\:*?"<>|]', '_')
        reg export $_.Name (Join-Path $regOut ("$safe.reg")) /y 2>$null
        if($LASTEXITCODE -eq 0){ $exported++ }
      }
  }
  $ErrorActionPreference = $prevEAP
  if($exported -eq 0){
    Say "   (no BRAINtellect/BST registry keys found)"
    Set-SectionStatus '3. Registry' 'skip'
  }else{
    Say ("   exported $exported registry key(s)")
    Set-SectionStatus '3. Registry' 'ok'
  }
  & $reportProgress 3 4 'Registry done' 95 $false
  }
  catch{
    # Name the stage that was running, so the export and the caller both see the failure.
    Say ("   ERROR: " + $_.Exception.Message)
    $running = @($sectionStatus.Keys | Where-Object { $sectionStatus[$_] -eq 'pending' })[0]
    if($running){ Set-SectionStatus $running 'fail' }
    throw
  }
  finally{
    try{
      # Release our private mysqld first: on Windows it holds the copied datadir's
      # ibdata1/ib_logfile open exclusively, which would prevent archive creation.
      if($proc -and -not $proc.HasExited){
        Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
        if(-not $proc.WaitForExit(30000)){ Say "   WARNING: the private database engine did not stop within 30 seconds." }
      }
      # Always write the per-section results BEFORE zipping, so the export records
      # success OR failure for every section even if one failed.
      Write-SectionSummary
      if(Test-Path $outZip){ Remove-Item $outZip -Force }
      Say "Writing the archive to your Desktop (this can take a bit)..."
      # ZipFile is available with .NET 4.5+, including the inbox PowerShell on
      # Windows 8/8.1. Compress-Archive would require PowerShell 5.0+.
      Add-Type -AssemblyName System.IO.Compression.FileSystem -ErrorAction Stop
      [System.IO.Compression.ZipFile]::CreateFromDirectory($work, $outZip, [System.IO.Compression.CompressionLevel]::Optimal, $false)
    }finally{
      # Never leave the private database or install copy behind, even on failure.
      Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue
    }
  }
}

# A headless PowerShell host has no UI message loop; run directly so failures
# remain on its error stream and propagate a useful exit status.
& $collection

Write-Finish
