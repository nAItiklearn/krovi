# Reminders QA helpers (owner: reminders). Dot-source:  . D:\krovi\qa\reminders\qa.ps1
#
# Runs an ISOLATED dev build of Krovi (identifier tech.krovi.remqa: its own data dir, WebView2
# profile, single-instance mutex and Start-menu identity), never the installed one in
# %LOCALAPPDATA%\Krovi. Nothing here touches, launches or quits the installed app.
#
#   Build:  (WinLibs on PATH, see docs/CONTRACTS.md §0)
#             npx vite build --outDir D:/krovi/qa/reminders/dist --emptyOutDir
#             $env:CARGO_TARGET_DIR = 'D:\krovi\qa\reminders\build\target'      # gitignored (**/target/), ~6 GB
#             npx tauri build --debug --no-bundle --config D:\krovi\qa\reminders\tauri.remqa.conf.json
#           (frontendDist in the config MUST be a relative path: an absolute "D:/..." parses as a URL.)
#   Copying Krovi.exe elsewhere also needs WebView2Loader.dll next to it.
#   Windows banners are held back while Do Not Disturb is on; evidence then is the notification centre (Win+N) and
#   %LOCALAPPDATA%\Microsoft\Windows\Notifications\wpndatabase.db (copy it first), as in test7-installed-identity.ps1.
#   Exe:    qa\reminders\build\target\debug\Krovi.exe   (a "target\debug" path => dev toast identity)

Add-Type -AssemblyName System.Drawing, System.Windows.Forms
if (-not ('KQR.U32' -as [type])) {
  Add-Type -Namespace KQR -Name U32 -MemberDefinition '[DllImport("user32.dll")] public static extern bool SetProcessDPIAware();'
}
[void][KQR.U32]::SetProcessDPIAware()

$QR = 'D:\krovi\qa\reminders'
$Exe = Join-Path $QR 'build\target\debug\Krovi.exe'   # built by: tauri build --debug --no-bundle --config qa\reminders\tauri.remqa.conf.json
$Port = 9333
$Ident = 'tech.krovi.remqa'
$Data = Join-Path $env:APPDATA $Ident
$PidFile = Join-Path $QR 'qa.pid'
$Log = Join-Path $QR 'reminders.log'

function Prep-RemQA {
  # Hidden pill (no second pill over the user's real one), a shortcut the installed app does not hold.
  New-Item -ItemType Directory -Force $Data | Out-Null
  $cfg = '{"shortcut":"Ctrl+Alt+Shift+F11","showPill":false,"collapseOnBlur":true,"launchAtLogin":false,"hideInFullscreen":true}'
  [IO.File]::WriteAllText((Join-Path $Data 'shell.json'), $cfg)
}

function Start-RemQA([string]$exe = $Exe) {
  if (Test-Path $PidFile) {
    $old = [int](Get-Content $PidFile)
    if (Get-Process -Id $old -ErrorAction SilentlyContinue) { "already running: pid $old"; return }
  }
  Prep-RemQA
  $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=$Port"
  $env:KROVI_REMINDER_LOG = $Log
  $p = Start-Process -FilePath $exe -PassThru -WindowStyle Hidden
  Remove-Item Env:\WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS, Env:\KROVI_REMINDER_LOG
  $p.Id | Set-Content $PidFile
  # wait for the webview to answer on the debug port
  for ($i = 0; $i -lt 60; $i++) {
    try { $null = Invoke-RestMethod "http://127.0.0.1:$Port/json" -TimeoutSec 1; break } catch { Start-Sleep -Milliseconds 500 }
  }
  "started pid $($p.Id)"
}

# Stops ONLY the process this script started (pid file), and only if it is still the QA exe.
function Stop-RemQA {
  if (-not (Test-Path $PidFile)) { 'no pid file'; return }
  $id = [int](Get-Content $PidFile)
  $p = Get-Process -Id $id -ErrorAction SilentlyContinue
  if ($p -and $p.Path -like "$QR\*") { Stop-Process -Id $id -Force; $p.WaitForExit(5000) | Out-Null; "stopped pid $id" } else { "pid $id is not the QA exe; left alone" }
  Remove-Item $PidFile -ErrorAction SilentlyContinue
}

# Evaluate JS inside the QA webview (awaits promises). Tauri commands: __TAURI_INTERNALS__.invoke(cmd, args)
function Cdp([string]$js) { node D:\krovi\qa\cdp.mjs $Port $js }
function Inv([string]$cmd, [string]$argsJson = '{}') { Cdp "window.__TAURI_INTERNALS__.invoke('$cmd', $argsJson)" }

function FullShot([string]$name) {
  $b = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
  $bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.CopyFromScreen($b.X, $b.Y, 0, 0, $bmp.Size)
  $p = Join-Path $QR "$name.png"; $bmp.Save($p); $g.Dispose(); $bmp.Dispose(); $p
}

# The persisted calendar rows straight from the QA database (read-only), via Python's sqlite3.
function Db-Events {
  $py = @'
import sqlite3, sys, datetime
c = sqlite3.connect("file:" + sys.argv[1].replace("\\", "/") + "?mode=ro", uri=True)
fmt = lambda ms: "-" if ms is None else datetime.datetime.fromtimestamp(ms / 1000).strftime("%H:%M:%S.%f")[:-3]
print("schema_version =", c.execute("select version from schema_version").fetchone()[0])
print("id  date        time   remind sound missed  fired_at       snoozed_until  title")
for r in c.execute("select id,date,time,remind,sound,missed,fired_at,snoozed_until,title from calendar_events order by id"):
    print(f"{r[0]:<3} {r[1]}  {r[2] or '-':<6} {r[3]:<6} {r[4]:<5} {r[5]:<6} {fmt(r[6]):<14} {fmt(r[7]):<14} {r[8]}")
'@
  $py | python -I - (Join-Path $Data 'krovi.db')
}

# "today" local date and HH:MM `n` minutes from now (rounded UP to the next whole minute, so the
# reminder time is always in the future)
function In-Minutes([int]$n) {
  $t = (Get-Date).AddMinutes($n)
  if ($t.Second -gt 0 -or $t.Millisecond -gt 0) { $t = $t.AddSeconds(60 - $t.Second).AddMilliseconds(-$t.Millisecond) }
  [pscustomobject]@{ date = $t.ToString('yyyy-MM-dd'); time = $t.ToString('HH:mm'); at = $t }
}
