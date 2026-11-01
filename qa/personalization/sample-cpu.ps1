# Sample the CPU time of one process (the Chrome renderer hosting the preview page) over N seconds.
#   pwsh -File qa/personalization/sample-cpu.ps1 -ProcId 27756 -Seconds 30 -Label on-curious
# CPU% is of ONE core: 100 = one core fully busy. Includes every thread of that renderer
# (main, compositor, raster, IO), which is the honest "cost of the page" number.
param(
  [Parameter(Mandatory)] [int]$ProcId,
  [int]$Seconds = 30,
  [string]$Label = ''
)
$p0 = Get-Process -Id $ProcId
$c0 = $p0.TotalProcessorTime.TotalMilliseconds
$t0 = Get-Date
Start-Sleep -Seconds $Seconds
$p1 = Get-Process -Id $ProcId
$c1 = $p1.TotalProcessorTime.TotalMilliseconds
$dt = ((Get-Date) - $t0).TotalMilliseconds
[pscustomobject]@{
  label       = $Label
  seconds     = [math]::Round($dt / 1000, 1)
  cpuMs       = [math]::Round($c1 - $c0, 1)
  cpuPctCore  = [math]::Round(($c1 - $c0) / $dt * 100, 3)
  workingSetMB = [math]::Round($p1.WorkingSet64 / 1MB, 1)
  privateMB   = [math]::Round($p1.PrivateMemorySize64 / 1MB, 1)
} | ConvertTo-Json -Compress
