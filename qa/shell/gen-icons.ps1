# Regenerates src-tauri/icons/* for Krovi: minimal monochrome (black rounded square,
# white "island" pill with a status dot). Re-runnable; writes the filenames the bundler expects.
Add-Type -AssemblyName System.Drawing
$out = 'D:\krovi\src-tauri\icons'

function New-RoundRect([single]$x, [single]$y, [single]$w, [single]$h, [single]$r) {
  $p = New-Object System.Drawing.Drawing2D.GraphicsPath
  $d = [Math]::Min($r * 2, [Math]::Min($w, $h))
  $p.AddArc($x, $y, $d, $d, 180, 90)
  $p.AddArc($x + $w - $d, $y, $d, $d, 270, 90)
  $p.AddArc($x + $w - $d, $y + $h - $d, $d, $d, 0, 90)
  $p.AddArc($x, $y + $h - $d, $d, $d, 90, 90)
  $p.CloseFigure()
  $p
}

function Draw-Icon([int]$s) {
  $bmp = New-Object System.Drawing.Bitmap $s, $s, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.SmoothingMode = 'AntiAlias'; $g.PixelOffsetMode = 'HighQuality'; $g.Clear([System.Drawing.Color]::Transparent)
  $pad = [single]($s * 0.03)
  $sq = [single]($s - 2 * $pad)
  $bg = New-RoundRect $pad $pad $sq $sq ([single]($s * 0.22))
  $g.FillPath((New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 10, 10, 11))), $bg)
  # hairline ring so the icon reads on dark taskbars too
  $ring = New-Object System.Drawing.Pen ([System.Drawing.Color]::FromArgb(110, 255, 255, 255)), ([single]([Math]::Max(1.0, $s * 0.02)))
  $g.DrawPath($ring, $bg)
  # the island: white pill, dark dot on the left
  $pw = [single]($s * 0.62); $ph = [single]($s * 0.26)
  $px = [single](($s - $pw) / 2); $py = [single](($s - $ph) / 2)
  $pill = New-RoundRect $px $py $pw $ph ([single]($ph / 2))
  $g.FillPath([System.Drawing.Brushes]::White, $pill)
  $dd = [single]($ph * 0.46)
  $dx = [single]($px + $ph * 0.28); $dy = [single]($py + ($ph - $dd) / 2)
  $g.FillEllipse((New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 10, 10, 11))), $dx, $dy, $dd, $dd)
  $g.Dispose()
  $bmp
}

function Save-Png([int]$s, [string]$path) {
  $b = Draw-Icon $s; $b.Save($path, [System.Drawing.Imaging.ImageFormat]::Png); $b.Dispose()
}

Save-Png 32  (Join-Path $out '32x32.png')
Save-Png 128 (Join-Path $out '128x128.png')
Save-Png 512 (Join-Path $out 'icon.png')

# Multi-size .ico with PNG-compressed entries
$sizes = 16, 24, 32, 48, 64, 128, 256
$blobs = @()
foreach ($s in $sizes) {
  $b = Draw-Icon $s; $ms = New-Object System.IO.MemoryStream
  $b.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png); $b.Dispose()
  $blobs += , $ms.ToArray()
}
$fs = [System.IO.File]::Create((Join-Path $out 'icon.ico')); $bw = New-Object System.IO.BinaryWriter $fs
$bw.Write([uint16]0); $bw.Write([uint16]1); $bw.Write([uint16]$sizes.Count)
$offset = 6 + 16 * $sizes.Count
for ($i = 0; $i -lt $sizes.Count; $i++) {
  $s = $sizes[$i]; $dim = if ($s -ge 256) { 0 } else { $s }
  $bw.Write([byte]$dim); $bw.Write([byte]$dim); $bw.Write([byte]0); $bw.Write([byte]0)
  $bw.Write([uint16]1); $bw.Write([uint16]32); $bw.Write([uint32]$blobs[$i].Length); $bw.Write([uint32]$offset)
  $offset += $blobs[$i].Length
}
foreach ($bl in $blobs) { $bw.Write($bl) }
$bw.Close(); $fs.Close()
Get-ChildItem $out | Select-Object Name, Length
