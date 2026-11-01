# Test 7: the "installed" toast identity. The plugin sets the AppUserModelID to the app identifier when the exe is NOT in a
# target\debug|release folder (as in an installed build). Run a copy of the QA exe from outside such a folder and see how
# Windows files the toast. (The real installer's Start-menu shortcut carries tech.krovi.app; this machine's copy is checked below.)
. D:\krovi\qa\reminders\qa.ps1
Add-Type -Namespace KQR -Name K2 -MemberDefinition '[DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);' -ErrorAction SilentlyContinue
function Chord2([byte[]]$vks) { foreach ($k in $vks) { [KQR.K2]::keybd_event($k,0,0,[UIntPtr]::Zero) }; Start-Sleep -Milliseconds 80; [array]::Reverse($vks); foreach ($k in $vks) { [KQR.K2]::keybd_event($k,0,2,[UIntPtr]::Zero) } }
Stop-RemQA | Out-Host
$dir = Join-Path $QR 'run-installed'; New-Item -ItemType Directory -Force $dir | Out-Null
Copy-Item $Exe (Join-Path $dir 'Krovi.exe') -Force
$mark = if (Test-Path $Log) { (Get-Content $Log | Measure-Object).Count } else { 0 }
Start-RemQA (Join-Path $dir 'Krovi.exe') | Out-Host
"--- environment as seen from a non-target folder:"
Cdp "window.__TAURI_INTERNALS__.invoke('reminders_status').then(s => s.environment)"
"--- test-fire reminder 6 ('Stretch your legs') under this identity"
Cdp "window.__TAURI_INTERNALS__.invoke('reminder_test_fire', {id: 6}).then(r => ({toast: r.toast, sound: r.sound, shownLateMs: r.shownLateMs}))"
Get-Content $Log | Select-Object -Skip $mark
Start-Sleep -Milliseconds 1200

$src = "$env:LOCALAPPDATA\Microsoft\Windows\Notifications"
$tmp = Join-Path $env:TEMP 'krovi-wpn-copy'; New-Item -ItemType Directory -Force $tmp | Out-Null
Copy-Item "$src\wpndatabase.db*" $tmp -Force -ErrorAction SilentlyContinue
$py = @'
import sqlite3, sys, datetime, re
c = sqlite3.connect(sys.argv[1])
def ft(t): return datetime.datetime(1601,1,1) + datetime.timedelta(microseconds=t/10) + datetime.timedelta(hours=5, minutes=30)
rows = c.execute("select n.ArrivalTime, h.PrimaryId, n.Payload from Notification n join NotificationHandler h on h.RecordId = n.HandlerId where h.PrimaryId like 'tech.krovi%' order by n.ArrivalTime desc limit 5").fetchall()
print("toasts filed under a tech.krovi.* AUMID:", len(rows))
for a, hid, p in rows:
    p = p.decode('utf-8','replace') if isinstance(p, bytes) else str(p)
    print(ft(a).strftime('%H:%M:%S.%f')[:-3], hid, [t for t in re.findall(r'<text[^>]*>([^<]*)</text>', p) if t])
'@
$py | python -I - (Join-Path $tmp 'wpndatabase.db')
$null = FullShot 'identity-installed-1'
Chord2 @(0x5B, 0x4E); Start-Sleep -Milliseconds 1600
"notification centre: " + (FullShot 'identity-installed-centre')
Chord2 @(0x5B, 0x4E)
