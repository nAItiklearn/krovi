# Krovi real-install QA harness (orchestrator-owned). Dot-source:  . D:\krovi\qa\harness.ps1
# Drives the INSTALLED app the way a user would: global shortcut, clicks, clipboard,
# a real Windows media session, and screenshots as evidence.

Add-Type -AssemblyName System.Drawing, System.Windows.Forms
if (-not ('KQ.U32' -as [type])) {
Add-Type -Namespace KQ -Name U32 -MemberDefinition @'
[DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
[DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
[DllImport("user32.dll")] public static extern void mouse_event(uint f, uint dx, uint dy, uint d, UIntPtr e);
[DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
[DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
[DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
[DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
[DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
[DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
public delegate bool EnumProc(IntPtr h, IntPtr l);
public struct RECT { public int Left, Top, Right, Bottom; }
'@
}
[void][KQ.U32]::SetProcessDPIAware()

$QA = 'D:\krovi\qa'
$Shots = Join-Path $QA 'shots'
New-Item -ItemType Directory -Force $Shots | Out-Null

function Shot([string]$name, [int]$x = -1, [int]$y = 0, [int]$w = 0, [int]$h = 0) {
  $b = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
  if ($x -lt 0) { $x = $b.X; $y = $b.Y; $w = $b.Width; $h = $b.Height }
  $bmp = New-Object System.Drawing.Bitmap $w, $h
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.CopyFromScreen($x, $y, 0, 0, $bmp.Size)
  $p = Join-Path $Shots "$name.png"; $bmp.Save($p); $g.Dispose(); $bmp.Dispose(); $p
}
# Top-centre crop (where the Island lives), physical px.
function ShotIsland([string]$name, [int]$w = 1400, [int]$h = 900) {
  $b = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
  Shot $name ([int]($b.X + ($b.Width - $w) / 2)) $b.Y $w $h
}

function Key([byte]$vk, [switch]$up) { [KQ.U32]::keybd_event($vk, 0, $(if ($up) { 2 } else { 0 }), [UIntPtr]::Zero) }
# Ctrl+Alt+Space by default
function Chord([byte[]]$vks = @(0x11, 0x12, 0x20)) {
  foreach ($k in $vks) { Key $k }; Start-Sleep -Milliseconds 60
  [array]::Reverse($vks); foreach ($k in $vks) { Key $k -up }
}
function Esc { Key 0x1B; Key 0x1B -up }
function Click([int]$x, [int]$y) {
  [void][KQ.U32]::SetCursorPos($x, $y); Start-Sleep -Milliseconds 40
  [KQ.U32]::mouse_event(2, 0, 0, 0, [UIntPtr]::Zero); [KQ.U32]::mouse_event(4, 0, 0, 0, [UIntPtr]::Zero)
}

# Visible top-level windows of the krovi-app process with their physical rects.
function KroviWindows {
  $pids = @(Get-Process krovi-app, Krovi -ErrorAction SilentlyContinue | ForEach-Object Id)
  $out = New-Object System.Collections.ArrayList
  $cb = [KQ.U32+EnumProc]{ param($h, $l)
    $p = 0; [void][KQ.U32]::GetWindowThreadProcessId($h, [ref]$p)
    if ($pids -contains $p) {
      $r = New-Object KQ.U32+RECT; [void][KQ.U32]::GetWindowRect($h, [ref]$r)
      [void]$out.Add([pscustomobject]@{ hwnd = $h; pid = $p; visible = [KQ.U32]::IsWindowVisible($h)
        x = $r.Left; y = $r.Top; w = $r.Right - $r.Left; h = $r.Bottom - $r.Top })
    }; $true }
  [void][KQ.U32]::EnumWindows($cb, [IntPtr]::Zero)
  $out | Where-Object { $_.visible -and $_.w -gt 50 }
}

function KroviProc { Get-Process krovi-app, Krovi -ErrorAction SilentlyContinue }

# A real Windows media session: Edge plays a local looping tone and publishes
# MediaSession metadata, which Windows exposes through GSMTC.
function Start-TestMedia([string]$title = 'Krovi QA A') {
  $page = Join-Path $QA 'media\player.html'
  $u = 'file:///' + ($page -replace '\\', '/') + '?title=' + [uri]::EscapeDataString($title)
  Start-Process msedge -ArgumentList '--new-window', '--autoplay-policy=no-user-gesture-required', $u
}
