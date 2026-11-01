# Krovi clip encoder (ui-polish, internal tooling).
#
# raw/<name>.mp4 (chrome-devtools screencast, AV1, VFR, includes dead time before/after the scripted
# choreography) -> out/<name>.mp4 (H.264) + out/<name>.webm (VP9) + out/<name>-poster.webp
#
# 1. finds the two magenta marker flashes written by record-lib.js (U-plane average > 165)
# 2. trims to the content between them (start-marker end -> end-marker start)
# 3. re-encodes at constant fps, downscaled, muted, with a size budget (default 1.5 MB)
#
# usage:  pwsh -File encode-clips.ps1 -Name 01-pill-expand [-Width 960] [-Fps 30] [-BudgetKB 1500]
param(
  [Parameter(Mandatory)][string]$Name,
  [int]$Width = 1280,
  [int]$Fps = 30,
  [int]$BudgetKB = 1500,
  [string]$Crop = ''   # optional ffmpeg crop "w:h:x:y" applied before scaling (e.g. the pill strip)
)
$ErrorActionPreference = 'Stop'
$root = $PSScriptRoot
$raw = Join-Path $root "raw\$Name.mp4"
$out = Join-Path $root 'out'
New-Item -ItemType Directory -Force $out | Out-Null
$tmp = Join-Path $env:TEMP "krovi-clip-$Name"
New-Item -ItemType Directory -Force $tmp | Out-Null
Push-Location $tmp
try {
  if (Test-Path sig.txt) { Remove-Item sig.txt }
  & ffmpeg -v quiet -i $raw -vf 'scale=32:18:flags=area,signalstats,metadata=print:key=lavfi.signalstats.UAVG:file=sig.txt' -an -f null - 2>$null

  # parse (pts_time, UAVG) pairs and group magenta frames into runs
  $runs = @(); $cur = $null; $t = 0.0
  foreach ($line in Get-Content sig.txt) {
    if ($line -match 'pts_time:([0-9.]+)') { $t = [double]$Matches[1] }
    elseif ($line -match 'UAVG=([0-9.]+)') {
      $mag = [double]$Matches[1] -gt 165
      if ($mag -and -not $cur) { $cur = @{ start = $t; end = $t } }
      elseif ($mag -and $cur) { $cur.end = $t }
      elseif (-not $mag -and $cur) { $runs += , $cur; $cur = $null }
    }
  }
  if ($cur) { $runs += , $cur }
  if ($runs.Count -lt 2) { throw "expected 2 marker flashes in $raw, found $($runs.Count)" }
  $a = $runs[0].end + 0.04
  $b = $runs[$runs.Count - 1].start - 0.02
  $dur = [math]::Round($b - $a, 2)
  Write-Host ("{0}: markers at {1:n2}s .. {2:n2}s -> content {3}s" -f $Name, $a, $b, $dur)

  $vf = "fps=$Fps,scale=${Width}:-2:flags=lanczos"
  if ($Crop) { $vf = "crop=$Crop,$vf" }
  $mp4 = Join-Path $out "$Name.mp4"
  $webm = Join-Path $out "$Name.webm"
  $poster = Join-Path $out "$Name-poster.webp"

  # CRF ladder: step up until under budget
  foreach ($crf in 22, 25, 28, 31, 34, 37) {
    & ffmpeg -v error -y -ss $a -t $dur -i $raw -vf $vf -c:v libx264 -preset slow -crf $crf -pix_fmt yuv420p -profile:v high -movflags +faststart -an $mp4 2>$null
    if ((Get-Item $mp4).Length / 1KB -le $BudgetKB) { Write-Host "  mp4 crf=$crf"; break }
  }
  foreach ($crf in 30, 34, 37, 40, 43) {
    & ffmpeg -v error -y -ss $a -t $dur -i $raw -vf $vf -c:v libvpx-vp9 -b:v 0 -crf $crf -row-mt 1 -deadline good -cpu-used 2 -pix_fmt yuv420p -an $webm 2>$null
    if ((Get-Item $webm).Length / 1KB -le $BudgetKB) { Write-Host "  webm crf=$crf"; break }
  }
  # poster = a representative frame (45% in), webp
  $pt = [math]::Round($a + $dur * 0.45, 2)
  & ffmpeg -v error -y -ss $pt -i $raw -frames:v 1 -vf "scale=${Width}:-2:flags=lanczos" -c:v libwebp -quality 78 $poster 2>$null

  foreach ($f in $mp4, $webm, $poster) {
    $kb = [math]::Round((Get-Item $f).Length / 1KB)
    Write-Host ("  {0,-34} {1,6} KB" -f (Split-Path $f -Leaf), $kb)
  }
}
finally { Pop-Location }
