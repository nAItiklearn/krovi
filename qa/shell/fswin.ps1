Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type -Namespace Q -Name N -MemberDefinition '[System.Runtime.InteropServices.DllImport("user32.dll")] public static extern bool SetProcessDPIAware();'
[void][Q.N]::SetProcessDPIAware()
$f = New-Object System.Windows.Forms.Form
$f.Text = 'KroviQA-Fullscreen'; $f.FormBorderStyle = 'None'; $f.StartPosition = 'Manual'
$f.Bounds = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds; $f.BackColor = [System.Drawing.Color]::FromArgb(255, 20, 40, 90)
$f.Add_Shown({ $f.Activate() })
[void]$f.ShowDialog()
