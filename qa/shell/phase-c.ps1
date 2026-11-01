# Phase C: the REAL production frontend (design agent's Island) in the real installed build,
# plus the real system-tray menu (right-click, native popup, keyboard / mouse), Alt+Tab-away
# collapse, and a final uninstall that must leave nothing running.
param([Parameter(Mandatory)][string]$Setup, [switch]$SkipInstall)
. D:\krovi\qa\shell\lib.ps1
$ErrorActionPreference = 'Continue'
$script:Shots = Join-Path $Out 'shots'
function S([string]$n, [int]$w = 1500, [int]$h = 900) { ShotIsland $n $w $h }
Add-Content (Join-Path $Out 'results.log') "`n===== PHASE C (production frontend) $(Get-Date -Format s) ====="

$install = Join-Path $env:LOCALAPPDATA 'Krovi'
$exe = Join-Path $install 'Krovi.exe'
$logf = Join-Path $env:TEMP 'krovi-shell.log'; Remove-Item $logf -ErrorAction SilentlyContinue
$pill = Expected-Geom 'pill'; $panel = Expected-Geom 'panel'; $s = $pill.scale
function Wd { if (Test-Path $logf) { @(Select-String -Path $logf -Pattern 'WATCHDOG').Count } else { 0 } }
function Mode { $k = Get-KroviMain; if (-not $k -or -not $k.visible) { 'hidden' } elseif (Same-Rect $k $pill) { 'pill' } elseif (Same-Rect $k $panel) { 'panel' } else { 'other' } }

if (-not $SkipInstall) {
  $p = Start-Process $Setup -ArgumentList '/S' -PassThru -Wait
  Report 'C0 install /S' ((Test-Path $exe) -and $p.ExitCode -eq 0) "exit=$($p.ExitCode)"
}
if (-not (Test-Path $exe)) { throw "not installed: $exe" }

function Open-TrayMenu {
  [void][KQ.U32]::SetCursorPos(1700, 1079); Start-Sleep -Milliseconds 300; [void][KQ.U32]::SetCursorPos(1700, 1060); Start-Sleep -Milliseconds 900
  # scoped UIA lookups (a root-wide search is slow/flaky on a busy desktop)
  $root = [System.Windows.Automation.AutomationElement]::RootElement
  $tray = $root.FindFirst([System.Windows.Automation.TreeScope]::Children, (New-Object System.Windows.Automation.PropertyCondition ([System.Windows.Automation.AutomationElement]::ClassNameProperty), 'Shell_TrayWnd'))
  $btn = Find-Uia 'Show Hidden Icons' $null $tray; if (-not $btn) { return $false }
  Click-Rect $btn.Current.BoundingRectangle; Start-Sleep -Milliseconds 900
  $ov = $root.FindFirst([System.Windows.Automation.TreeScope]::Children, (New-Object System.Windows.Automation.PropertyCondition ([System.Windows.Automation.AutomationElement]::ClassNameProperty), 'TopLevelWindowForOverflowXamlIsland'))
  if (-not $ov) { return $false }
  $k = Find-Uia 'Krovi*' $null $ov
  if (-not $k) { return $false }
  Click-Rect $k.Current.BoundingRectangle -right; Start-Sleep -Milliseconds 800
  $true
}
function Menu-Pick([int]$upCount) { for ($i = 0; $i -lt $upCount; $i++) { Key 0x26; Key 0x26 -up; Start-Sleep -Milliseconds 120 }; Key 0x0D; Key 0x0D -up }

$cw = Start-ClickWindow 300 0 1320 400 (Join-Path $Out 'clicks.log'); Start-Sleep -Seconds 2
$env:KROVI_SHELL_LOG = '1'
$null = Start-Process $exe -PassThru
$ms = Wait-Until { (Mode) -eq 'pill' } 10000
Report 'C1 launch: pill at PILL_WIN with the production UI' ($ms -ge 0) "after ${ms}ms"
Report 'C1b launch: no focus theft' ((Get-FgInfo).proc -notmatch '^(Krovi|krovi-app)') "fg=$((Get-FgInfo).proc)"
Start-Sleep -Seconds 3; S 'c01-pill'
$wd0 = Wd

Chord; $ms = Wait-Until { (Mode) -eq 'panel' } 4000
Report 'C2 shortcut expands the production Island (UI handshake, no watchdog)' ($ms -ge 0 -and (Wd) -eq $wd0) "latency ${ms}ms; watchdog=$((Wd) - $wd0)"
Start-Sleep -Milliseconds 1500; S 'c02-panel'
Esc; $ms = Wait-Until { (Mode) -eq 'pill' } 4000
Report 'C3 Esc collapses (production UI morph then commit)' ($ms -ge 0 -and (Wd) -eq $wd0) "latency ${ms}ms"
Start-Sleep -Milliseconds 800; S 'c03-collapsed'

# click the pill (real mouse) -> expands; click outside -> collapses
$w = Get-KroviMain
Click ([int]($w.x + $w.w / 2)) ([int]($w.y + 30 * $s)); $ms = Wait-Until { (Mode) -eq 'panel' } 4000
Report 'C4 clicking the production pill expands it' ($ms -ge 0) "latency ${ms}ms"
Start-Sleep -Milliseconds 1200
Click 330 380; $ms = Wait-Until { (Mode) -eq 'pill' } 4000
Report 'C5 click outside the production panel collapses it' ($ms -ge 0 -and (Wd) -eq $wd0) "latency ${ms}ms"
Start-Sleep -Milliseconds 700

# Alt+Tab away from the open panel
Chord; [void](Wait-Until { (Mode) -eq 'panel' } 4000); Start-Sleep -Milliseconds 1200
Key 0x12; Key 0x09; Start-Sleep -Milliseconds 120; Key 0x09 -up; Key 0x12 -up
$ms = Wait-Until { (Mode) -eq 'pill' } 5000
Report 'C6 Alt+Tab away from the panel collapses it (focus loss)' ($ms -ge 0) "latency ${ms}ms; fg=$((Get-FgInfo).proc)"
Start-Sleep -Milliseconds 600

# ---- real tray: Settings... ----
$ok = Open-TrayMenu; Shot 'c-tray-menu' 1100 500 820 580 | Out-Null
Report 'C7a tray menu opens (native popup)' $ok 'right-click on the real tray icon'
Menu-Pick 2   # Quit(1) -> Settings...(2)
$ms = Wait-Until { (Mode) -eq 'panel' } 5000
Start-Sleep -Milliseconds 1800; S 'c04-settings-via-tray'
Report 'C7b tray -> Settings... expands the panel' ($ms -ge 0) "latency ${ms}ms"

# ---- real tray: Show pill toggle ----
Esc; [void](Wait-Until { (Mode) -eq 'pill' } 4000)
$ok = Open-TrayMenu; Menu-Pick 3   # Show pill
$ms = Wait-Until { (Mode) -eq 'hidden' } 4000
Report 'C8a tray -> Show pill (uncheck) hides the pill' ($ms -ge 0) "mode=$(Mode)"
Chord; $ms = Wait-Until { (Mode) -eq 'panel' } 4000
Report 'C8b ...shortcut still summons the panel with the pill off' ($ms -ge 0) "latency ${ms}ms"
Esc; [void](Wait-Until { (Mode) -eq 'hidden' } 4000)
$ok = Open-TrayMenu; Menu-Pick 3
$ms = Wait-Until { (Mode) -eq 'pill' } 4000
Report 'C8c tray -> Show pill (re-check) brings it back' ($ms -ge 0) "mode=$(Mode)"

# ---- real tray: Open Krovi, then left-click toggle ----
$ok = Open-TrayMenu; Menu-Pick 4   # Open Krovi
$ms = Wait-Until { (Mode) -eq 'panel' } 5000
Report 'C9 tray -> Open Krovi expands' ($ms -ge 0) "latency ${ms}ms"
Start-Sleep -Milliseconds 1200

# ---- quit via tray with the production UI, relaunch, second launch ----
$ok = Open-TrayMenu
$root = [System.Windows.Automation.AutomationElement]::RootElement
$cur = Get-Cursor
[void][KQ.U32]::SetCursorPos($cur.X + 79, $cur.Y - 16); Start-Sleep -Milliseconds 300     # "Quit Krovi" row of the popup
[KQ.U32]::mouse_event(2, 0, 0, 0, [UIntPtr]::Zero); [KQ.U32]::mouse_event(4, 0, 0, 0, [UIntPtr]::Zero)
$ms = Wait-Until { -not (KroviProc) } 6000
Report 'C10 tray -> Quit Krovi (mouse) with the production UI: process gone' ($ms -ge 0) "gone after ${ms}ms"
$wv = @(Get-CimInstance Win32_Process -Filter "Name='msedgewebview2.exe'" | Where-Object { $_.CommandLine -match 'tech\.krovi\.app' }).Count
Start-Sleep -Milliseconds 2500
$wv = @(Get-CimInstance Win32_Process -Filter "Name='msedgewebview2.exe'" | Where-Object { $_.CommandLine -match 'tech\.krovi\.app' }).Count
Report 'C10b no orphan WebView2 processes after quit' ($wv -eq 0) "left=$wv"

$null = Start-Process $exe -PassThru
$ms = Wait-Until { (Mode) -eq 'pill' } 10000
Report 'C11 reopen after quit works' ($ms -ge 0) "pill after ${ms}ms"
Start-Sleep -Seconds 2
$null = Start-Process $exe -PassThru
Start-Sleep -Milliseconds 3000
Report 'C12 second launch while running: still 1 process, panel opens' (@(KroviProc).Count -eq 1 -and (Mode) -eq 'panel') "procs=$(@(KroviProc).Count) mode=$(Mode)"
Start-Sleep -Milliseconds 1500; S 'c05-after-second-launch'
Report 'C13 watchdog count across production-UI session' $true "WATCHDOG lines=$(Wd) (baseline $wd0)"

Stop-Process -Id $cw.Id -Force -ErrorAction SilentlyContinue
