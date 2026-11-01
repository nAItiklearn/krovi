# Phase A2 (continues phase A, blank frontend): REAL tray interaction, quit, relaunch,
# `Krovi.exe --quit`, and uninstall while running. Expects the app running (phase A leaves it up).
. D:\krovi\qa\shell\lib.ps1
$ErrorActionPreference = 'Continue'
$script:Shots = Join-Path $Out 'shots'
Add-Content (Join-Path $Out 'results.log') "`n===== PHASE A2 (blank frontend: tray / quit / uninstall) $(Get-Date -Format s) ====="
$install = Join-Path $env:LOCALAPPDATA 'Krovi'
$exe = Join-Path $install 'Krovi.exe'
$pill = Expected-Geom 'pill'; $panel = Expected-Geom 'panel'
function Mode { $k = Get-KroviMain; if (-not $k -or -not $k.visible) { 'hidden' } elseif (Same-Rect $k $pill) { 'pill' } elseif (Same-Rect $k $panel) { 'panel' } else { 'other' } }
function Open-Flyout {
  [void][KQ.U32]::SetCursorPos(1700, 1079); Start-Sleep -Milliseconds 300; [void][KQ.U32]::SetCursorPos(1700, 1060); Start-Sleep -Milliseconds 900
  $btn = Find-Uia 'Show Hidden Icons'; Click-Rect $btn.Current.BoundingRectangle; Start-Sleep -Milliseconds 1000
  Find-Uia 'Krovi*' ([System.Windows.Automation.ControlType]::Button)
}
if (-not (KroviProc)) { $env:KROVI_SHELL_LOG = '1'; $null = Start-Process $exe; [void](Wait-Until { (Mode) -ne 'hidden' } 10000) }

# ensure pill, then LEFT-CLICK the real tray icon
if ((Mode) -eq 'panel') { Chord; [void](Wait-Until { (Mode) -eq 'pill' } 6000) }
$k = Open-Flyout
Report '4a tray icon present with tooltip' ($null -ne $k) "name='$($k.Current.Name)'"
Click-Rect $k.Current.BoundingRectangle
$ms = Wait-Until { (Mode) -eq 'panel' } 6000
Report '4b real tray LEFT-click toggles the panel open' ($ms -ge 0) "after ${ms}ms (blank UI: watchdog path)"
Start-Sleep -Milliseconds 600; ShotIsland 'a2-tray-left-click' 1500 900 | Out-Null

# RIGHT-click -> native menu -> mouse click on "Quit Krovi"
$k = Open-Flyout
Click-Rect $k.Current.BoundingRectangle -right; Start-Sleep -Milliseconds 900
Shot 'a2-tray-menu' 1100 500 820 580 | Out-Null
$cur = Get-Cursor
$before = @(KroviProc | ForEach-Object { "$($_.ProcessName)/$($_.Id)" })
[void][KQ.U32]::SetCursorPos($cur.X + 79, $cur.Y - 16); Start-Sleep -Milliseconds 300
[KQ.U32]::mouse_event(2, 0, 0, 0, [UIntPtr]::Zero); [KQ.U32]::mouse_event(4, 0, 0, 0, [UIntPtr]::Zero)
$ms = Wait-Until { -not (KroviProc) } 8000
Start-Sleep -Milliseconds 2500
$wv = @(Get-CimInstance Win32_Process -Filter "Name='msedgewebview2.exe'" | Where-Object { $_.CommandLine -match 'tech\.krovi\.app' }).Count
Report '4c tray menu -> "Quit Krovi" (real mouse): process gone, no orphans' ($ms -ge 0 -and @(KroviProc).Count -eq 0 -and $wv -eq 0) "was [$($before -join ',')]; gone after ${ms}ms; krovi procs=$(@(KroviProc).Count); orphan webview2=$wv"
$k = Open-Flyout
Report '4d tray icon removed on quit (no ghost icon)' ($null -eq $k) "ghost present: $([bool]$k)"; Shot 'a2-tray-after-quit' 1100 600 820 480 | Out-Null
Esc; Start-Sleep -Milliseconds 400

# relaunch
$env:KROVI_SHELL_LOG = '1'
$null = Start-Process $exe
$ms = Wait-Until { (Mode) -eq 'pill' } 10000
Report '4e relaunch after quit works' ($ms -ge 0) "pill after ${ms}ms"
Start-Sleep -Seconds 1

# `Krovi.exe --quit` second-instance handoff
$null = Start-Process $exe -ArgumentList '--quit'
$ms = Wait-Until { -not (KroviProc) } 8000
Report '4f `Krovi.exe --quit` closes the running instance entirely' ($ms -ge 0) "gone after ${ms}ms; procs=$(@(KroviProc).Count)"

# idle CPU of the running shell (sampler is 30 Hz): relaunch, sample 10 s
$null = Start-Process $exe
[void](Wait-Until { (Mode) -eq 'pill' } 10000); Start-Sleep -Seconds 4
$p = KroviProc | Select-Object -First 1
$t0 = (Get-Process -Id $p.Id).TotalProcessorTime.TotalMilliseconds; Start-Sleep -Seconds 10
$t1 = (Get-Process -Id $p.Id).TotalProcessorTime.TotalMilliseconds
$cpu = [Math]::Round(($t1 - $t0) / 10000 * 100, 2)
Report '4g idle CPU of the app (30 Hz sampler + blank page)' ($cpu -lt 3) "$cpu % of one core over 10s (Krovi.exe only, excludes WebView2 children)"

# ---- 5. UNINSTALL with the app running ----
$un = Join-Path $install 'uninstall.exe'
$sw = [Diagnostics.Stopwatch]::StartNew()
$u = Start-Process $un -ArgumentList '/S' -PassThru
$ms = Wait-Until { -not (KroviProc) } 20000
[void](Wait-Until { (-not (Test-Path $exe)) -and (-not (Get-Process -Id $u.Id -ErrorAction SilentlyContinue)) } 30000)
Start-Sleep -Seconds 2
$reg = @(Get-ChildItem 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall' | ForEach-Object { Get-ItemProperty $_.PSPath } | Where-Object { $_.DisplayName -eq 'Krovi' }).Count
$left = if (Test-Path $install) { @(Get-ChildItem $install -Recurse -File).Count } else { 0 }
Report '5 uninstall.exe /S WHILE RUNNING: kills app, removes files + registry' ((@(KroviProc).Count -eq 0) -and -not (Test-Path $exe) -and $reg -eq 0) "app gone after ${ms}ms; total $($sw.ElapsedMilliseconds)ms; files left=$left; uninstall reg entries=$reg"
