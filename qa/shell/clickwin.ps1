Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type -Namespace Q -Name N -MemberDefinition '[System.Runtime.InteropServices.DllImport("user32.dll")] public static extern bool SetProcessDPIAware();'
[void][Q.N]::SetProcessDPIAware()
$f = New-Object System.Windows.Forms.Form
$f.Text = 'KroviQA-ClickWindow'; $f.FormBorderStyle = 'None'; $f.StartPosition = 'Manual'
$f.Bounds = New-Object System.Drawing.Rectangle 300, 0, 1320, 400; $f.BackColor = [System.Drawing.Color]::FromArgb(255, 220, 60, 60)
$f.ShowInTaskbar = $false
$f.Add_MouseDown({ param($s,$e) $p = [System.Windows.Forms.Cursor]::Position; Add-Content 'D:\krovi\qa\shell\clicks.log' ('click ' + $p.X + ',' + $p.Y) })
[void]$f.ShowDialog()
