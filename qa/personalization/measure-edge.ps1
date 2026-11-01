# Cost of the preview page in a REAL Chromium (Edge 156 = the engine inside WebView2), with NO
# DevTools attached, summed over every process of a throwaway profile (browser + GPU + renderer
# + utility). That is the honest "what does this page cost the machine" number.
#
#   powershell -NoProfile -File qa/personalization/measure-edge.ps1 -Url "http://localhost:5182/src/components/idle/preview.html?perf=on&delay=1500" -Label on-curious -Seconds 30
#
# Only processes whose command line carries this run's private --user-data-dir are touched.
param(
  [Parameter(Mandatory)] [string]$Url,
  [string]$Label = '',
  [int]$Seconds = 30,
  [int]$WarmupSeconds = 15
)
$edge = 'C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe'
$profileDir = Join-Path $env:TEMP ("krovi-idle-measure-" + [guid]::NewGuid().ToString('N').Substring(0, 8))
$args = @(
  "--user-data-dir=$profileDir", '--no-first-run', '--no-default-browser-check',
  '--disable-features=CalculateNativeWinOcclusion,msEdgeWelcome', '--disable-backgrounding-occluded-windows',
  '--disable-extensions', '--window-position=40,40', '--window-size=700,260', "--app=$Url"
)
$proc = Start-Process -FilePath $edge -ArgumentList $args -PassThru
try {
  Start-Sleep -Seconds $WarmupSeconds
  # the page's cost = its renderer(s) + the GPU process (the browser/utility processes carry
  # Edge's own first-run housekeeping, which is not ours)
  $mine = { Get-CimInstance Win32_Process -Filter "Name='msedge.exe'" | Where-Object { $_.CommandLine -like "*$profileDir*" -and $_.CommandLine -match '--type=(renderer|gpu-process)' } }
  # per-pid snapshot: a process that exits between the two snapshots must not skew the delta
  $snap = {
    $h = @{}
    foreach ($p in (& $mine)) {
      $g = Get-Process -Id $p.ProcessId -ErrorAction SilentlyContinue
      if ($g) { $h[[int]$p.ProcessId] = [pscustomobject]@{ cpu = $g.TotalProcessorTime.TotalMilliseconds; ws = $g.WorkingSet64; type = [regex]::Match($p.CommandLine, '--type=([a-z-]+)').Groups[1].Value } }
    }
    $h
  }
  $a = & $snap
  $t0 = Get-Date
  Start-Sleep -Seconds $Seconds
  $b = & $snap
  $dt = ((Get-Date) - $t0).TotalMilliseconds
  $cpu = 0.0; $ws = 0.0; $kept = @()
  foreach ($id in $b.Keys) {
    if ($a.ContainsKey($id)) {
      $d = $b[$id].cpu - $a[$id].cpu
      $cpu += $d; $ws += $b[$id].ws
      $kept += ('{0}:{1}={2}ms' -f $b[$id].type, $id, [math]::Round($d))
    }
  }
  [pscustomobject]@{
    label        = $Label
    seconds      = [math]::Round($dt / 1000, 1)
    processes    = $kept.Count
    perProcess   = ($kept -join ' ')
    cpuMs        = [math]::Round($cpu, 1)
    cpuPctCore   = [math]::Round($cpu / $dt * 100, 3)
    workingSetMB = [math]::Round($ws / 1MB, 1)
  } | ConvertTo-Json -Compress
}
finally {
  # close only what this run started
  Get-CimInstance Win32_Process -Filter "Name='msedge.exe'" | Where-Object { $_.CommandLine -like "*$profileDir*" } |
    ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
  Start-Sleep -Milliseconds 800
  Remove-Item -Recurse -Force $profileDir -ErrorAction SilentlyContinue
}
