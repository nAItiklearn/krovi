Add-Type -AssemblyName System.Windows.Forms
Add-Type -Namespace H -Name N -MemberDefinition '[System.Runtime.InteropServices.DllImport("user32.dll")] public static extern bool RegisterHotKey(System.IntPtr h, int id, uint mod, uint vk);'
$f = New-Object System.Windows.Forms.Form; $f.ShowInTaskbar = $false; $f.WindowState = 'Minimized'; $f.Opacity = 0
$f.Add_Shown({ $ok = [H.N]::RegisterHotKey($f.Handle, 1, 0x3, 0x78); Set-Content "$env:TEMP\hk-owner.txt" "registered=$ok" })   # Ctrl+Alt+F9
[void]$f.ShowDialog()
