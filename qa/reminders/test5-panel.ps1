# Test 5: the reminder UI in the real (QA) panel: open it, go to Calendar, screenshot, put it away again.
# (Briefly shows the QA panel over the desktop; the installed Krovi is not touched.)
. D:\krovi\qa\reminders\qa.ps1
Add-Type -AssemblyName System.Drawing
$u = In-Minutes 15; $v = In-Minutes 20
$null = Inv 'reminder_add' "{date:'$($u.date)', title:'Standup (silent)', time:'$($u.time)', remind:true, sound:false}"
$null = Inv 'reminder_add' "{date:'$($v.date)', title:'Just a note, no alert', time:'$($v.time)', remind:false, sound:true}"
Inv 'shell_set_mode' "{mode:'panel'}"
Start-Sleep -Milliseconds 1500
Cdp "(() => { const b = document.querySelector('[aria-label=Calendar]'); if (b) b.click(); return !!b; })()"
Start-Sleep -Milliseconds 1500
$b = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
$w = 1380; $h = 900
$x = [int]($b.X + ($b.Width - $w) / 2)
$bmp = New-Object System.Drawing.Bitmap $w, $h
$g = [System.Drawing.Graphics]::FromImage($bmp); $g.CopyFromScreen($x, $b.Y, 0, 0, $bmp.Size)
$p = Join-Path $QR 'panel-calendar.png'; $bmp.Save($p); $g.Dispose(); $bmp.Dispose(); $p
Inv 'shell_set_mode' "{mode:'hidden'}"
