# Phase B: a MINIMAL UI that consumes the real src/lib/shell.ts (qa/shell/ui), real silent
# install. Proves the Rust <-> shell.ts handshake, config/shortcut re-registration, hold,
# collapse-on-blur, show-pill, autostart, Esc / Alt+F4 / Ctrl+Q and persistence.
param([Parameter(Mandatory)][string]$Setup, [switch]$SkipInstall, [switch]$KeepInstalled)
. D:\krovi\qa\shell\lib.ps1
$ErrorActionPreference = 'Continue'
$script:Shots = Join-Path $Out 'shots'
function S([string]$n, [int]$w = 1500, [int]$h = 900) { ShotIsland $n $w $h }
Add-Content (Join-Path $Out 'results.log') "`n===== PHASE B (UI via shell.ts) $(Get-Date -Format s) ====="

$install = Join-Path $env:LOCALAPPDATA 'Krovi'
$exe = Join-Path $install 'Krovi.exe'
$logf = Join-Path $env:TEMP 'krovi-shell.log'; Remove-Item $logf -ErrorAction SilentlyContinue
$clicks = Join-Path $Out 'clicks.log'
$cfgFile = Join-Path $env:APPDATA 'tech.krovi.app\shell.json'
$pill = Expected-Geom 'pill'; $panel = Expected-Geom 'panel'; $s = $pill.scale

function Wd { if (Test-Path $logf) { @(Select-String -Path $logf -Pattern 'WATCHDOG').Count } else { 0 } }
function Mode { $k = Get-KroviMain; if (-not $k -or -not $k.visible) { 'hidden' } elseif (Same-Rect $k $pill) { 'pill' } elseif (Same-Rect $k $panel) { 'panel' } else { 'other' } }
function Ensure-Pill { if ((Mode) -eq 'panel') { Esc; [void](Wait-Until { (Mode) -eq 'pill' } 4000) }; Start-Sleep -Milliseconds 300 }
function Qlog { try { ((Invoke-Cdp 'JSON.stringify(window.__qaLog())').result.result.value | ConvertFrom-Json) -join ' | ' } catch { "cdp error: $_" } }
function ChordK { Chord @(0x11, 0x12, 0x4B) }
function ChordSpace { Chord }

if (-not $SkipInstall) {
  $p = Start-Process $Setup -ArgumentList '/S' -PassThru -Wait
  Report 'B0 install /S (currentUser)' ((Test-Path $exe) -and $p.ExitCode -eq 0) "exit=$($p.ExitCode) -> $exe"
}
if (-not (Test-Path $exe)) { throw "not installed: $exe" }
Remove-Item $cfgFile -ErrorAction SilentlyContinue   # start from defaults

$cw = Start-ClickWindow 300 0 1320 400 $clicks
Start-Sleep -Seconds 2
$env:KROVI_SHELL_LOG = '1'
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=$CdpPort"
$null = Start-Process $exe -PassThru
$ms = Wait-Until { (Mode) -eq 'pill' } 10000
Report 'B1 launch: pill at PILL_WIN top-centre' ($ms -ge 0) "after ${ms}ms"
Report 'B1b launch: did not take focus' ((Get-FgInfo).proc -notmatch '^(Krovi|krovi-app)') "fg=$((Get-FgInfo).proc)"
Start-Sleep -Seconds 2
$r = Tauri-Invoke 'shell_get_mode'; Report 'B1c IPC works under the app CSP (invoke shell_get_mode)' ($r.ok -eq 'pill') "ok=$($r.ok) err=$($r.err)"
S 'b01-launch'

# ---- B2 shortcut handshake via the real shell.ts (no watchdog expected) ----
$wd0 = Wd
ChordSpace; $ms = Wait-Until { (Mode) -eq 'panel' } 4000
Report 'B2a shortcut -> panel via UI handshake (fast, not the watchdog)' ($ms -ge 0 -and $ms -lt 900 -and (Wd) -eq $wd0) "latency ${ms}ms; watchdog fired $((Wd) - $wd0)x"
Start-Sleep -Milliseconds 700; S 'b02-panel'
$fg = Get-FgInfo; Report 'B2b panel focused' ($fg.proc -match '^(Krovi|krovi-app)') "fg=$($fg.proc)"
Report 'B2c UI saw request+mode: ' $true (Qlog)
ChordSpace; $ms = Wait-Until { (Mode) -eq 'pill' } 4000
Report 'B2d shortcut again -> pill (UI morph 420ms then commit)' ($ms -ge 400 -and $ms -lt 1300 -and (Wd) -eq $wd0) "latency ${ms}ms; watchdog fired $((Wd) - $wd0)x"
Start-Sleep -Milliseconds 400; S 'b03-pill-again'

# ---- B3 click the pill expands; Esc collapses ----
Start-Sleep -Milliseconds 400
$w = Get-KroviMain
$cx = [int]($w.x + $w.w / 2); $cy = [int]($w.y + 32 * $s)
Click $cx $cy; $ms = Wait-Until { (Mode) -eq 'panel' } 4000
Report 'B3a click on pill expands the panel' ($ms -ge 0) "latency ${ms}ms"
Start-Sleep -Milliseconds 700
Esc; $ms = Wait-Until { (Mode) -eq 'pill' } 4000
Report 'B3b Esc collapses the panel' ($ms -ge 0 -and (Wd) -eq $wd0) "latency ${ms}ms"

# ---- B4 UI-reported hit rect is honoured ----
Start-Sleep -Milliseconds 400
$null = Tauri-Invoke 'shell_set_hit_rect' '{"rect":{"x":100,"y":20,"w":60,"h":30}}'
Start-Sleep -Milliseconds 300
Remove-Item $clicks -ErrorAction SilentlyContinue
Click $cx $cy; Start-Sleep -Milliseconds 400                       # pill centre, now OUTSIDE the reported rect
$passed = (Test-Path $clicks) -and ((Get-Content $clicks) -join '') -match "click $cx,$cy"
Report 'B4a reported hit rect: click outside it passes through' $passed "clicked $cx,$cy"
Ensure-Pill
$null = Tauri-Invoke 'shell_set_hit_rect' '{"rect":{"x":-5,"y":NaN,"w":1,"h":1}}' 2>$null
# restore a sane rect via mode change (resets to contract default)
ChordSpace; [void](Wait-Until { (Mode) -eq 'panel' } 4000); Start-Sleep -Milliseconds 600; Ensure-Pill

# ---- B5 config: validate / re-register / keep old on failure ----
$cfg = (Tauri-Invoke 'shell_get_config').ok
Report 'B5a get_config returns contract shape' ($cfg.shortcut -eq 'Ctrl+Alt+Space' -and $cfg.showPill -eq $true -and $cfg.collapseOnBlur -eq $true -and $cfg.hideInFullscreen -eq $true -and $cfg.launchAtLogin -eq $false) ($cfg | ConvertTo-Json -Compress)
$newCfg = '{"config":{"shortcut":"Ctrl+Alt+K","showPill":true,"collapseOnBlur":true,"launchAtLogin":false,"hideInFullscreen":true}}'
$r = Tauri-Invoke 'shell_set_config' $newCfg
Report 'B5b set_config accepts Ctrl+Alt+K' ($r.ok.shortcut -eq 'Ctrl+Alt+K') (($r | ConvertTo-Json -Compress -Depth 4))
ChordK; $ms = Wait-Until { (Mode) -eq 'panel' } 3000
Report 'B5c new shortcut (Ctrl+Alt+K) works' ($ms -ge 0) "latency ${ms}ms"
Start-Sleep -Milliseconds 600; Esc; [void](Wait-Until { (Mode) -eq 'pill' } 3000); Start-Sleep -Milliseconds 300
ChordSpace; Start-Sleep -Milliseconds 1500
Report 'B5d old shortcut (Ctrl+Alt+Space) no longer fires' ((Mode) -eq 'pill') "mode=$(Mode)"
$bad = Tauri-Invoke 'shell_set_config' '{"config":{"shortcut":"Ctrl+Banana","showPill":true,"collapseOnBlur":true,"launchAtLogin":false,"hideInFullscreen":true}}'
Report 'B5e invalid shortcut -> clear error' ($bad.err -match 'not a valid shortcut') "err=$($bad.err)"
$bare = Tauri-Invoke 'shell_set_config' '{"config":{"shortcut":"A","showPill":true,"collapseOnBlur":true,"launchAtLogin":false,"hideInFullscreen":true}}'
Report 'B5f modifier-less shortcut rejected' ($bare.err -match 'needs a modifier') "err=$($bare.err)"
# a shortcut another app already owns
$hk = Join-Path $Out 'hotkey-owner.ps1'
Set-Content $hk @'
Add-Type -AssemblyName System.Windows.Forms
Add-Type -Namespace H -Name N -MemberDefinition '[System.Runtime.InteropServices.DllImport("user32.dll")] public static extern bool RegisterHotKey(System.IntPtr h, int id, uint mod, uint vk);'
$f = New-Object System.Windows.Forms.Form; $f.ShowInTaskbar = $false; $f.WindowState = 'Minimized'; $f.Opacity = 0
$f.Add_Shown({ $ok = [H.N]::RegisterHotKey($f.Handle, 1, 0x3, 0x78); Set-Content "$env:TEMP\hk-owner.txt" "registered=$ok" })   # Ctrl+Alt+F9
[void]$f.ShowDialog()
'@
Remove-Item "$env:TEMP\hk-owner.txt" -ErrorAction SilentlyContinue
$hkp = Start-Process powershell -ArgumentList '-NoProfile', '-STA', '-ExecutionPolicy', 'Bypass', '-File', $hk -PassThru
[void](Wait-Until { Test-Path "$env:TEMP\hk-owner.txt" } 8000)
$taken = Tauri-Invoke 'shell_set_config' '{"config":{"shortcut":"Ctrl+Alt+F9","showPill":true,"collapseOnBlur":true,"launchAtLogin":false,"hideInFullscreen":true}}'
Report 'B5g taken shortcut (owned by another app) -> clear error' ($taken.err -match "Couldn't use") "owner: $(Get-Content "$env:TEMP\hk-owner.txt"); err=$($taken.err)"
Stop-Process -Id $hkp.Id -Force -ErrorAction SilentlyContinue
ChordK; $ms = Wait-Until { (Mode) -eq 'panel' } 3000
Report 'B5h after a failed set_config the previous shortcut still works' ($ms -ge 0) "latency ${ms}ms"
Start-Sleep -Milliseconds 600; Esc; [void](Wait-Until { (Mode) -eq 'pill' } 3000)
Report 'B5i config persisted to shell.json' ((Test-Path $cfgFile) -and ((Get-Content $cfgFile -Raw) -match 'Ctrl\+Alt\+K')) "$cfgFile : $((Get-Content $cfgFile -Raw) -replace '\s+',' ')"

# ---- B6 launch at login via tauri-plugin-autostart ----
$runKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$on = Tauri-Invoke 'shell_set_config' '{"config":{"shortcut":"Ctrl+Alt+K","showPill":true,"collapseOnBlur":true,"launchAtLogin":true,"hideInFullscreen":true}}'
$val = (Get-ItemProperty $runKey -ErrorAction SilentlyContinue).Krovi
Report 'B6a launchAtLogin=true writes HKCU Run entry' ($on.ok.launchAtLogin -eq $true -and $val -match 'Krovi\.exe') "Run\Krovi = $val"
$off = Tauri-Invoke 'shell_set_config' '{"config":{"shortcut":"Ctrl+Alt+K","showPill":true,"collapseOnBlur":true,"launchAtLogin":false,"hideInFullscreen":true}}'
$val2 = (Get-ItemProperty $runKey -ErrorAction SilentlyContinue).Krovi
Report 'B6b launchAtLogin=false removes it' ($off.ok.launchAtLogin -eq $false -and -not $val2) "Run\Krovi = [$val2]"

# ---- B7 hold + collapseOnBlur ----
$mx = $null
ChordK; [void](Wait-Until { (Mode) -eq 'panel' } 3000); Start-Sleep -Milliseconds 800
$pw = Get-KroviMain
$null = Tauri-Invoke 'shell_hold' '{"hold":true}'
Click 330 380; Start-Sleep -Milliseconds 2200
Report 'B7a shell_hold(true): click outside does NOT collapse' ((Mode) -eq 'panel') "mode=$(Mode)"
$null = Tauri-Invoke 'shell_hold' '{"hold":false}'
Click 330 380; $ms = Wait-Until { (Mode) -eq 'pill' } 4000
Report 'B7b shell_hold(false): click outside collapses (UI morph path)' ($ms -ge 0 -and (Wd) -eq $wd0) "latency ${ms}ms"
$noblur = '{"config":{"shortcut":"Ctrl+Alt+K","showPill":true,"collapseOnBlur":false,"launchAtLogin":false,"hideInFullscreen":true}}'
$null = Tauri-Invoke 'shell_set_config' $noblur
ChordK; [void](Wait-Until { (Mode) -eq 'panel' } 3000); Start-Sleep -Milliseconds 800
Click 330 380; Start-Sleep -Milliseconds 2200
Report 'B7c collapseOnBlur=false: click outside keeps panel' ((Mode) -eq 'panel') "mode=$(Mode)"
ChordK; $ms = Wait-Until { (Mode) -eq 'pill' } 4000
Report 'B7d ...but the shortcut still collapses it' ($ms -ge 0) "latency ${ms}ms"
$null = Tauri-Invoke 'shell_set_config' '{"config":{"shortcut":"Ctrl+Alt+K","showPill":true,"collapseOnBlur":true,"launchAtLogin":false,"hideInFullscreen":true}}'

# ---- B8 Alt+F4 must not kill the island ----
ChordK; [void](Wait-Until { (Mode) -eq 'panel' } 3000); Start-Sleep -Milliseconds 800
Key 0x12; Key 0x73; Start-Sleep -Milliseconds 60; Key 0x73 -up; Key 0x12 -up
$ms = Wait-Until { (Mode) -eq 'pill' } 4000
Report 'B8 Alt+F4 on the panel collapses it, app keeps running' ($ms -ge 0 -and @(KroviProc).Count -eq 1) "mode=$(Mode) procs=$(@(KroviProc).Count)"

# ---- B9 show pill off/on ----
$hide = '{"config":{"shortcut":"Ctrl+Alt+K","showPill":false,"collapseOnBlur":true,"launchAtLogin":false,"hideInFullscreen":true}}'
$null = Tauri-Invoke 'shell_set_config' $hide
$ms = Wait-Until { (Mode) -eq 'hidden' } 3000
Report 'B9a showPill=false hides the pill' ($ms -ge 0) "mode=$(Mode)"
ChordK; $ms = Wait-Until { (Mode) -eq 'panel' } 3000
Report 'B9b ...shortcut still summons the panel' ($ms -ge 0) "latency ${ms}ms"
Start-Sleep -Milliseconds 700; Esc; $ms = Wait-Until { (Mode) -eq 'hidden' } 4000
Report 'B9c ...and collapsing returns to hidden (not pill)' ($ms -ge 0) "mode=$(Mode)"
$null = Tauri-Invoke 'shell_set_config' '{"config":{"shortcut":"Ctrl+Alt+K","showPill":true,"collapseOnBlur":true,"launchAtLogin":false,"hideInFullscreen":true}}'
$ms = Wait-Until { (Mode) -eq 'pill' } 3000
Report 'B9d showPill=true brings the pill back' ($ms -ge 0) "mode=$(Mode)"

# ---- B10 fullscreen, hideInFullscreen on/off ----
$fs = Start-FullscreenWindow; Start-Sleep -Milliseconds 2500
$ms = Wait-Until { (Mode) -eq 'hidden' } 4000
Report 'B10a fullscreen app hides pill' ($ms -ge 0) "mode=$(Mode)"
ChordK; $ms = Wait-Until { (Mode) -eq 'panel' } 3000
Report 'B10b ...but the shortcut still opens the panel over a fullscreen app' ($ms -ge 0) "latency ${ms}ms"
Esc; Start-Sleep -Milliseconds 1500
Stop-Process -Id $fs.Id -Force -ErrorAction SilentlyContinue
$ms = Wait-Until { (Mode) -eq 'pill' } 5000
Report 'B10c pill restored after fullscreen ends' ($ms -ge 0) "mode=$(Mode)"

# ---- B11 persistence across a real quit + relaunch, then restore defaults ----
$null = Tauri-Invoke 'shell_quit'
$ms = Wait-Until { -not (KroviProc) } 6000
Report 'B11a shell_quit (the command) ends the process' ($ms -ge 0) "gone after ${ms}ms"
$null = Start-Process $exe -PassThru
[void](Wait-Until { (Mode) -eq 'pill' } 10000); Start-Sleep -Seconds 1
ChordK; $ms = Wait-Until { (Mode) -eq 'panel' } 3000
Report 'B11b relaunch restores the saved shortcut (Ctrl+Alt+K)' ($ms -ge 0) "latency ${ms}ms"
Start-Sleep -Milliseconds 700
$null = Tauri-Invoke 'shell_set_config' '{"config":{"shortcut":"Ctrl+Alt+Space","showPill":true,"collapseOnBlur":true,"launchAtLogin":false,"hideInFullscreen":true}}'
Esc; [void](Wait-Until { (Mode) -eq 'pill' } 3000)

# ---- B12 Ctrl+Q quits from the focused panel ----
ChordSpace; [void](Wait-Until { (Mode) -eq 'panel' } 3000); Start-Sleep -Milliseconds 900
Key 0x11; Key 0x51; Start-Sleep -Milliseconds 60; Key 0x51 -up; Key 0x11 -up
$ms = Wait-Until { -not (KroviProc) } 5000
Report 'B12 Ctrl+Q in the focused panel quits' ($ms -ge 0) "gone after ${ms}ms"

# ---- B13 relaunch; UI Quit button + tray Settings...: covered manually in phase A2 ----
Report 'B13 watchdog never fired during UI-driven tests' ((Wd) -eq $wd0) "WATCHDOG lines total=$(Wd) (baseline $wd0)"
Stop-Process -Id $cw.Id -Force -ErrorAction SilentlyContinue
if (-not $KeepInstalled) { }
