# Test 1: a reminder set ~2 minutes out fires a real Windows toast AT its time, with the panel hidden.
#   powershell -File qa\reminders\test1-fire.ps1 [-Title 'QA fire test'] [-Minutes 2]
param([string]$Title = 'QA fire test', [int]$Minutes = 2)
. D:\krovi\qa\reminders\qa.ps1
Add-Type -Namespace KQR -Name K -MemberDefinition '[DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra); [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y); [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint dx, uint dy, uint d, UIntPtr e);' -ErrorAction SilentlyContinue
function Chord2([byte[]]$vks) { foreach ($k in $vks) { [KQR.K]::keybd_event($k,0,0,[UIntPtr]::Zero) }; Start-Sleep -Milliseconds 80; [array]::Reverse($vks); foreach ($k in $vks) { [KQR.K]::keybd_event($k,0,2,[UIntPtr]::Zero) } }

Remove-Item $Log -ErrorAction SilentlyContinue
Start-RemQA | Out-Host

$t = In-Minutes $Minutes
"set at   : $((Get-Date).ToString('HH:mm:ss.fff'))   reminder for $($t.date) $($t.time) (window hidden: shell mode is 'hidden')"
$null = Inv 'reminder_add' "{date:'$($t.date)', title:'$Title', time:'$($t.time)', remind:true, sound:true}"
"--- list after add (remind/sound/firedAt):"
Cdp "window.__TAURI_INTERNALS__.invoke('load_calendar').then(l => l.filter(e => e.title === '$Title'))"
"--- pending per the scheduler + toast environment:"
Cdp "window.__TAURI_INTERNALS__.invoke('reminders_status').then(s => ({next: s.next, now: s.nowLocal, env: s.environment}))"
"--- shell mode:"
Inv 'shell_get_mode'

# watch the clock; screenshot the whole screen right after the due time (the banner is bottom-right)
$due = $t.at
while ((Get-Date) -lt $due.AddMilliseconds(-1500)) { Start-Sleep -Milliseconds 200 }
$shots = @()
foreach ($i in 1..3) {
  while ((Get-Date) -lt $due.AddMilliseconds(700 + 1500 * ($i - 1))) { Start-Sleep -Milliseconds 25 }
  $shots += (FullShot ("fire-$i")) + "  @ " + (Get-Date).ToString('HH:mm:ss.fff')
}
$shots
Start-Sleep -Milliseconds 800

"--- reminder log"
Get-Content $Log
"--- persisted row in the DB"
Db-Events

# Windows' own notification centre: shows the toast that was delivered (banners are held back while Do Not Disturb is on)
Chord2 @(0x5B, 0x4E)            # Win+N
Start-Sleep -Milliseconds 1500
$null = FullShot 'action-center-1'
[void][KQR.K]::SetCursorPos(1569, 407); Start-Sleep -Milliseconds 120   # the "+N notifications" expander of the PowerShell group
[KQR.K]::mouse_event(2,0,0,0,[UIntPtr]::Zero); [KQR.K]::mouse_event(4,0,0,0,[UIntPtr]::Zero)
Start-Sleep -Milliseconds 1200
"action centre: " + (FullShot 'action-center-2')
Chord2 @(0x5B, 0x4E)            # Win+N closes it
