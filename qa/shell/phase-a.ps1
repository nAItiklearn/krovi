# Phase A: BLANK frontend (no JS at all) against a real silent install.
# Proves the Rust-side guarantees hold even if the UI never answers a shell-request.
param([Parameter(Mandatory)][string]$Setup, [switch]$SkipInstall)
. D:\krovi\qa\shell\lib.ps1
$ErrorActionPreference = 'Continue'
$Shots2 = Join-Path $Out 'shots'; New-Item -ItemType Directory -Force $Shots2 | Out-Null
function S([string]$n, [int]$w = 1500, [int]$h = 900) { $script:Shots = $Shots2; ShotIsland $n $w $h }
Add-Content (Join-Path $Out 'results.log') "`n===== PHASE A (blank frontend) $(Get-Date -Format s) ====="

$install = Join-Path $env:LOCALAPPDATA 'Krovi'
$exe = @('Krovi.exe','krovi-app.exe') | ForEach-Object { Join-Path $install $_ } | Where-Object { Test-Path $_ } | Select-Object -First 1; if (-not $exe) { $exe = Join-Path $install 'Krovi.exe' }
$logf = Join-Path $env:TEMP 'krovi-shell.log'; Remove-Item $logf -ErrorAction SilentlyContinue
$clicks = Join-Path $Out 'clicks.log'

# ---- install silently (per-user, no admin prompt) ----
if (-not $SkipInstall) {
  $sw = [Diagnostics.Stopwatch]::StartNew()
  $p = Start-Process $Setup -ArgumentList '/S' -PassThru -Wait
  Report 'install /S (currentUser, no UAC)' ((Test-Path $exe) -and $p.ExitCode -eq 0) "exit=$($p.ExitCode) $($sw.ElapsedMilliseconds)ms -> $exe"
}
if (-not (Test-Path $exe)) { throw "not installed: $exe" }

# ---- a foreign focused window, so we can prove Krovi does not steal focus on launch ----
$cw = Start-ClickWindow 300 0 1320 400 $clicks
Start-Sleep -Seconds 2
$fg0 = Get-FgInfo
Report 'precondition: click window has foreground' ($fg0.pid -eq $cw.Id -or $fg0.proc -match 'powershell') "fg=$($fg0.proc)/$($fg0.pid)"

# ---- 1. LAUNCH ----
$env:KROVI_SHELL_LOG = '1'
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=$CdpPort"
$kp = Start-Process $exe -PassThru
$pill = Expected-Geom 'pill'
$ms = Wait-Until { $w = Get-KroviMain; $w -and $w.visible } 10000
$w = Get-KroviMain
Start-Sleep -Milliseconds 800
$w = Get-KroviMain
$fg1 = Get-FgInfo
Report '1a launch: window visible' ($ms -ge 0) "after ${ms}ms"
Report '1b launch: PILL_WIN size + top-centre anchor (not 1080x700, not centred)' (Same-Rect $w $pill) ("actual {0},{1} {2}x{3} | expected {4},{5} {6}x{7} (scale {8})" -f $w.x, $w.y, $w.w, $w.h, $pill.x, $pill.y, $pill.w, $pill.h, $pill.scale)
Report '1c launch: did NOT take focus' ($fg1.proc -notmatch '^(Krovi|krovi-app)') "fg=$($fg1.proc)/$($fg1.pid)"
$tray0 = KroviProc | Measure-Object | Select-Object -ExpandProperty Count
S '01-launch-pill'

# ---- 2. SHORTCUT: expand to PANEL_WIN, same anchor; again -> collapse (watchdog) ----
$panel = Expected-Geom 'panel'
Chord
$ms = Wait-Until { Same-Rect (Get-KroviMain) $panel } 6000
$w2 = Get-KroviMain
$cx = $w2.x + $w2.w / 2; $cp = $w.x + $w.w / 2
Report '2a shortcut: expands to PANEL_WIN' ($ms -ge 0) ("after ${ms}ms (blank UI => watchdog path); actual {0},{1} {2}x{3} expected {4},{5} {6}x{7}" -f $w2.x, $w2.y, $w2.w, $w2.h, $panel.x, $panel.y, $panel.w, $panel.h)
Report '2b shortcut: same top-centre anchor (pill never jumps)' (([Math]::Abs($cx - $cp) -le 1) -and $w2.y -eq $w.y) "centreX pill=$cp panel=$cx; top pill=$($w.y) panel=$($w2.y)"
Start-Sleep -Milliseconds 400
$fg2 = Get-FgInfo
Report '2c panel has keyboard focus' ($fg2.proc -match '^(Krovi|krovi-app)') "fg=$($fg2.proc)"
S '02-panel-expanded'
Start-Sleep -Milliseconds 400
Chord
$ms = Wait-Until { Same-Rect (Get-KroviMain) $pill } 6000
Report '2d shortcut again: collapses to PILL_WIN (watchdog)' ($ms -ge 0) "after ${ms}ms"
S '03-collapsed-again'

# ---- 3a. CLICK-THROUGH in pill mode ----
Start-Sleep -Milliseconds 500
$w = Get-KroviMain
$s = $pill.scale
$marginPt = @([int]($w.x + 5), [int]($w.y + 40 * $s))                                  # inside window, left transparent margin
$insidePt = @([int]($w.x + $w.w / 2), [int]($w.y + (10 + 22) * $s))                    # centre of the default visible pill rect
Remove-Item $clicks -ErrorAction SilentlyContinue
Click $marginPt[0] $marginPt[1]; Start-Sleep -Milliseconds 400
$log1 = if (Test-Path $clicks) { Get-Content $clicks } else { @() }
Report '3a click on pill-window TRANSPARENT MARGIN reaches app underneath' ($log1 -contains "click $($marginPt[0]),$($marginPt[1])") "clicked $($marginPt -join ','); underneath window received: $($log1 -join ' | ')"
Remove-Item $clicks -ErrorAction SilentlyContinue
Click $insidePt[0] $insidePt[1]; Start-Sleep -Milliseconds 400
$log2 = if (Test-Path $clicks) { Get-Content $clicks } else { @() }
Report '3b click INSIDE visible pill rect is captured (not passed through)' ($log2.Count -eq 0) "clicked $($insidePt -join ','); underneath received: $($log2.Count) clicks"
S '04-clickthrough-pill'

# ---- 3c. no focus theft while the cursor crosses the pill (click-through toggling) ----
# put focus on the foreign window, then sweep the cursor in/out of the pill several times
Click 320 350; Start-Sleep -Milliseconds 300
$stolen = 0
for ($i = 0; $i -lt 8; $i++) {
  [void][KQ.U32]::SetCursorPos($insidePt[0], $insidePt[1]); Start-Sleep -Milliseconds 120
  if ((Get-FgInfo).proc -match '^(Krovi|krovi-app)') { $stolen++ }
  [void][KQ.U32]::SetCursorPos($marginPt[0], $marginPt[1]); Start-Sleep -Milliseconds 120
  if ((Get-FgInfo).proc -match '^(Krovi|krovi-app)') { $stolen++ }
}
Report '3c hovering the pill never steals focus (8 sweeps)' ($stolen -eq 0) "focus stolen $stolen times"

# ---- 3d. panel: click outside collapses; margin click passes through; inside click does not collapse ----
Chord; [void](Wait-Until { Same-Rect (Get-KroviMain) $panel } 6000); Start-Sleep -Milliseconds 700
$pw = Get-KroviMain
Click ([int]($pw.x + $pw.w / 2)) ([int]($pw.y + 300 * $s)); Start-Sleep -Milliseconds 2600
Report '3d click INSIDE panel does not collapse it' (Same-Rect (Get-KroviMain) $panel) 'still PANEL_WIN 2.6s after an inside click'
Remove-Item $clicks -ErrorAction SilentlyContinue
$mx = [int]($pw.x + 8); $my = [int]($pw.y + 120 * $s)
Click $mx $my
$ms = Wait-Until { Same-Rect (Get-KroviMain) $pill } 6000
Start-Sleep -Milliseconds 300
$log3 = if (Test-Path $clicks) { Get-Content $clicks } else { @() }
Report '3e click in panel-window margin (outside panel) reaches app beneath' ($log3 -contains "click $mx,$my") "clicked $mx,$my; underneath: $($log3 -join ' | ')"
Report '3f click outside panel collapses it' ($ms -ge 0) "back to PILL_WIN after ${ms}ms"
S '05-after-outside-click'

# ---- 4. Fullscreen auto-hide ----
$fs = Start-FullscreenWindow
$ms = Wait-Until { $k = Get-KroviMain; $k -and -not $k.visible } 5000
Report '4a fullscreen app in foreground hides the pill' ($ms -ge 0) "hidden after ${ms}ms"
S '06-fullscreen' 1500 400
Stop-Process -Id $fs.Id -Force -ErrorAction SilentlyContinue
$ms = Wait-Until { $k = Get-KroviMain; $k -and $k.visible } 5000
Report '4b pill returns when fullscreen ends' ($ms -ge 0) "visible again after ${ms}ms"

# ---- 5. STRESS: random input, then prove we converge to a sane state and stay usable ----
$rnd = New-Object Random 7
for ($i = 0; $i -lt 14; $i++) {
  switch ($rnd.Next(0, 4)) {
    0 { Chord }
    1 { Click ([int]($pw.x + 8)) ([int]($pw.y + 120 * $s)) }
    2 { Click ([int]($pw.x + $pw.w / 2)) ([int]($pw.y + 300 * $s)) }
    3 { }
  }
  Start-Sleep -Milliseconds $rnd.Next(30, 700)
}
Start-Sleep -Milliseconds 3200
$k = Get-KroviMain
$isPill = Same-Rect $k $pill; $isPanel = Same-Rect $k $panel
Report '6a after stress: settled in exactly pill or panel (no stuck/odd state)' (($isPill -or $isPanel) -and $k.visible) ("state={0} {1},{2} {3}x{4}" -f $(if ($isPill) { 'pill' } elseif ($isPanel) { 'panel' } else { '??' }), $k.x, $k.y, $k.w, $k.h)
# shortcut must still toggle both ways
$before = if ($isPanel) { 'panel' } else { 'pill' }
Chord; Start-Sleep -Milliseconds 2600
$k = Get-KroviMain; $after = if (Same-Rect $k $panel) { 'panel' } elseif (Same-Rect $k $pill) { 'pill' } else { '??' }
Report '6b after stress: shortcut still toggles' ($after -ne $before -and $after -ne '??') "$before -> $after"

# ---- 6. Single instance: second launch must not create a second process ----
Chord; Start-Sleep -Milliseconds 2600     # make sure we are in pill first
if (-not (Same-Rect (Get-KroviMain) $pill)) { Chord; Start-Sleep -Milliseconds 2600 }
$null = Start-Process $exe -PassThru
Start-Sleep -Milliseconds 3500
$n = @(KroviProc).Count
$ms = Wait-Until { Same-Rect (Get-KroviMain) $panel } 4000
Report '7a second launch: still exactly 1 Krovi process' ($n -eq 1) "Krovi processes: $n"
Report '7b second launch: expands the existing panel' ($ms -ge 0) "panel after ${ms}ms"
S '07-after-second-launch'

Write-Host "`nPhase A pre-quit done. Krovi pid(s): $((KroviProc).Id -join ',')"
Get-Process | Where-Object { $_.MainWindowTitle -match 'KroviQA' } | ForEach-Object { }
Stop-Process -Id $cw.Id -Force -ErrorAction SilentlyContinue

