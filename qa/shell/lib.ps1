# panel-shell QA helpers. Dot-source:  . D:\krovi\qa\shell\lib.ps1
# Builds on the orchestrator harness (keybd/mouse/EnumWindows/screenshots) and adds
# geometry expectations, foreground checks, test windows, UIA tray driving and a CDP
# bridge that can invoke the app's own commands inside the installed build.
. D:\krovi\qa\harness.ps1
$Out = 'D:\krovi\qa\shell'
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes

if (-not ('KS.W' -as [type])) {
Add-Type -Namespace KS -Name W -MemberDefinition @'
[DllImport("user32.dll")] public static extern uint GetDpiForSystem();
[DllImport("user32.dll")] public static extern bool SystemParametersInfo(uint a, uint b, out RECT r, uint c);
[DllImport("user32.dll")] public static extern int GetWindowLong(IntPtr h, int i);
[DllImport("user32.dll")] public static extern int GetClassName(IntPtr h, System.Text.StringBuilder s, int n);
[DllImport("user32.dll")] public static extern bool GetCursorPos(out POINT p);
public struct RECT { public int Left, Top, Right, Bottom; }
public struct POINT { public int X, Y; }
'@
}

function Get-Scale { [KS.W]::GetDpiForSystem() / 96.0 }
function Get-WorkArea { $r = New-Object KS.W+RECT; [void][KS.W]::SystemParametersInfo(0x30, 0, [ref]$r, 0); $r }
function Get-Cursor { $p = New-Object KS.W+POINT; [void][KS.W]::GetCursorPos([ref]$p); $p }

# Expected window rect (physical px) for a mode, per CONTRACTS Â§1.
function Expected-Geom([string]$mode) {
  $s = Get-Scale; $wa = Get-WorkArea
  if ($mode -eq 'pill') { $lw = 440; $lh = 88 } else { $lw = 960; $lh = 690 }
  $w = [int][Math]::Round($lw * $s, [MidpointRounding]::AwayFromZero)
  $h = [int][Math]::Round($lh * $s, [MidpointRounding]::AwayFromZero)
  $x = $wa.Left + [int][Math]::Floor((($wa.Right - $wa.Left) - $w) / 2)
  $y = $wa.Top + [int][Math]::Round(6 * $s, [MidpointRounding]::AwayFromZero)
  [pscustomobject]@{ x = $x; y = $y; w = $w; h = $h; scale = $s }
}

# The Krovi main window (top-level, visible or not) of the running Krovi process.
function Get-KroviMain {
  $pids = @(Get-Process Krovi, krovi-app -ErrorAction SilentlyContinue | ForEach-Object Id)
  $best = $null
  $cb = [KQ.U32+EnumProc]{ param($h, $l)
    $p = 0; [void][KQ.U32]::GetWindowThreadProcessId($h, [ref]$p)
    if ($pids -contains $p) {
      $sb = New-Object System.Text.StringBuilder 128; [void][KS.W]::GetClassName($h, $sb, 128)
      $r = New-Object KQ.U32+RECT; [void][KQ.U32]::GetWindowRect($h, [ref]$r)
      # tao top-level window class; skip tray/message-only helper windows (tiny / 0 size)
      if ($sb.ToString() -eq 'Tauri Window') {
        $script:found = [pscustomobject]@{ hwnd = $h; pid = $p; cls = $sb.ToString(); visible = [KQ.U32]::IsWindowVisible($h)
          x = $r.Left; y = $r.Top; w = $r.Right - $r.Left; h = $r.Bottom - $r.Top }
      }
    }; $true }
  $script:found = $null
  [void][KQ.U32]::EnumWindows($cb, [IntPtr]::Zero)
  $script:found
}

function Get-FgInfo {
  $h = [KQ.U32]::GetForegroundWindow(); $p = 0; [void][KQ.U32]::GetWindowThreadProcessId($h, [ref]$p)
  $name = (Get-Process -Id $p -ErrorAction SilentlyContinue).ProcessName
  [pscustomobject]@{ hwnd = $h; pid = $p; proc = $name }
}

function Same-Rect($w, $e, [int]$tol = 2) {
  $w -and [Math]::Abs($w.x - $e.x) -le $tol -and [Math]::Abs($w.y - $e.y) -le $tol -and [Math]::Abs($w.w - $e.w) -le $tol -and [Math]::Abs($w.h - $e.h) -le $tol
}

function Wait-Until([scriptblock]$cond, [int]$timeoutMs = 4000, [int]$stepMs = 50) {
  $sw = [Diagnostics.Stopwatch]::StartNew()
  while ($sw.ElapsedMilliseconds -lt $timeoutMs) { if (& $cond) { return $sw.ElapsedMilliseconds }; Start-Sleep -Milliseconds $stepMs }
  -1
}

function Report($name, $ok, $detail) {
  $line = ('{0,-62} {1}  {2}' -f $name, $(if ($ok) { 'PASS' } else { 'FAIL' }), $detail)
  Write-Host $line -ForegroundColor $(if ($ok) { 'Green' } else { 'Red' })
  Add-Content -Path (Join-Path $Out 'results.log') -Value $line
}

# A separate-process WinForms window that logs every click (screen coords) it receives.
function Start-ClickWindow([int]$x, [int]$y, [int]$w, [int]$h, [string]$log) {
  Remove-Item $log -ErrorAction SilentlyContinue
  $src = @"
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type -Namespace Q -Name N -MemberDefinition '[System.Runtime.InteropServices.DllImport("user32.dll")] public static extern bool SetProcessDPIAware();'
[void][Q.N]::SetProcessDPIAware()
`$f = New-Object System.Windows.Forms.Form
`$f.Text = 'KroviQA-ClickWindow'; `$f.FormBorderStyle = 'None'; `$f.StartPosition = 'Manual'
`$f.Bounds = New-Object System.Drawing.Rectangle $x, $y, $w, $h; `$f.BackColor = [System.Drawing.Color]::FromArgb(255, 220, 60, 60)
`$f.ShowInTaskbar = `$false
`$f.Add_MouseDown({ param(`$s,`$e) `$p = [System.Windows.Forms.Cursor]::Position; Add-Content '$log' ('click ' + `$p.X + ',' + `$p.Y) })
[void]`$f.ShowDialog()
"@
  $f = Join-Path $Out 'clickwin.ps1'; Set-Content $f $src
  Start-Process powershell -ArgumentList '-NoProfile', '-STA', '-ExecutionPolicy', 'Bypass', '-File', $f -PassThru
}

# A borderless window covering the whole primary monitor (what a game / video player looks like).
function Start-FullscreenWindow {
  $src = @"
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type -Namespace Q -Name N -MemberDefinition '[System.Runtime.InteropServices.DllImport("user32.dll")] public static extern bool SetProcessDPIAware();'
[void][Q.N]::SetProcessDPIAware()
`$f = New-Object System.Windows.Forms.Form
`$f.Text = 'KroviQA-Fullscreen'; `$f.FormBorderStyle = 'None'; `$f.StartPosition = 'Manual'
`$f.Bounds = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds; `$f.BackColor = [System.Drawing.Color]::FromArgb(255, 20, 40, 90)
`$f.Add_Shown({ `$f.Activate() })
[void]`$f.ShowDialog()
"@
  $f = Join-Path $Out 'fswin.ps1'; Set-Content $f $src
  Start-Process powershell -ArgumentList '-NoProfile', '-STA', '-ExecutionPolicy', 'Bypass', '-File', $f -PassThru
}

# --- CDP: run JS inside the installed app's webview (needs the app started with
#     WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=$CdpPort) ---------------
$CdpPort = 9333
function Invoke-Cdp([string]$expr) {
  $targets = Invoke-RestMethod "http://127.0.0.1:$CdpPort/json"
  $t = $targets | Where-Object { $_.type -eq 'page' } | Select-Object -First 1
  $ws = New-Object System.Net.WebSockets.ClientWebSocket
  [void]$ws.ConnectAsync([uri]$t.webSocketDebuggerUrl, [Threading.CancellationToken]::None).GetAwaiter().GetResult()
  $msg = @{ id = 1; method = 'Runtime.evaluate'; params = @{ expression = $expr; awaitPromise = $true; returnByValue = $true } } | ConvertTo-Json -Depth 6 -Compress
  $bytes = [Text.Encoding]::UTF8.GetBytes($msg)
  [void]$ws.SendAsync([ArraySegment[byte]]$bytes, 'Text', $true, [Threading.CancellationToken]::None).GetAwaiter().GetResult()
  $buf = New-Object byte[] 65536; $sb = New-Object System.Text.StringBuilder
  do {
    $r = $ws.ReceiveAsync([ArraySegment[byte]]$buf, [Threading.CancellationToken]::None).GetAwaiter().GetResult()
    [void]$sb.Append([Text.Encoding]::UTF8.GetString($buf, 0, $r.Count))
  } until ($r.EndOfMessage)
  try { $ws.CloseAsync('NormalClosure', '', [Threading.CancellationToken]::None).GetAwaiter().GetResult() } catch {}
  $sb.ToString() | ConvertFrom-Json
}
function Tauri-Invoke([string]$cmd, [string]$argsJson = '{}') {
  $r = Invoke-Cdp "window.__TAURI_INTERNALS__.invoke('$cmd', $argsJson).then(v => JSON.stringify({ok:v}), e => JSON.stringify({err:String(e)}))"
  if ($r.result.result.value) { $r.result.result.value | ConvertFrom-Json } else { $r }
}

# --- UI Automation: find / click tray icon + its menu ---------------------------------------
$UIA = [System.Windows.Automation.AutomationElement]
function Find-Uia([string]$nameLike, $type = $null, $root = $null, [int]$depthLimit = 0) {
  if (-not $root) { $root = $UIA::RootElement }
  $cond = if ($type) { New-Object System.Windows.Automation.PropertyCondition ([System.Windows.Automation.AutomationElement]::ControlTypeProperty), $type } else { [System.Windows.Automation.Condition]::TrueCondition }
  $all = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants, $cond)
  foreach ($e in $all) { if ($e.Current.Name -like $nameLike) { return $e } }
  $null
}
function Click-Rect($rect, [switch]$right) {
  $x = [int]($rect.X + $rect.Width / 2); $y = [int]($rect.Y + $rect.Height / 2)
  [void][KQ.U32]::SetCursorPos($x, $y); Start-Sleep -Milliseconds 120
  if ($right) { [KQ.U32]::mouse_event(0x8, 0, 0, 0, [UIntPtr]::Zero); [KQ.U32]::mouse_event(0x10, 0, 0, 0, [UIntPtr]::Zero) }
  else { [KQ.U32]::mouse_event(2, 0, 0, 0, [UIntPtr]::Zero); [KQ.U32]::mouse_event(4, 0, 0, 0, [UIntPtr]::Zero) }
}

