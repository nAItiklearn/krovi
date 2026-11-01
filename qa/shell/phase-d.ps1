# Phase D: shell-cursor event (idle companion eyes) captured from the real installed app
# through the QA UI's window.__cursor log (read over CDP).
param([Parameter(Mandatory)][string]$Setup)
. D:\krovi\qa\shell\lib.ps1
$ErrorActionPreference = 'Continue'
Add-Content (Join-Path $Out 'results.log') "`n===== PHASE D (shell-cursor) $(Get-Date -Format s) ====="
$install = Join-Path $env:LOCALAPPDATA 'Krovi'; $exe = Join-Path $install 'Krovi.exe'
$pill = Expected-Geom 'pill'; $panel = Expected-Geom 'panel'; $s = $pill.scale
function Mode { $k = Get-KroviMain; if (-not $k -or -not $k.visible) { 'hidden' } elseif (Same-Rect $k $pill) { 'pill' } elseif (Same-Rect $k $panel) { 'panel' } else { 'other' } }
function Ev { $r = Invoke-Cdp 'JSON.stringify(window.__cursor)'; @($r.result.result.value | ConvertFrom-Json) }
function Clear-Ev { [void](Invoke-Cdp 'window.__cursor.length = 0; 1') }
function MouseTo([int]$x, [int]$y) { [void][KQ.U32]::SetCursorPos($x, $y) }

$p = Start-Process $Setup -ArgumentList '/S' -PassThru -Wait
Report 'D0 install /S' ((Test-Path $exe) -and $p.ExitCode -eq 0) "exit=$($p.ExitCode)"
Remove-Item (Join-Path $env:TEMP 'krovi-shell.log') -ErrorAction SilentlyContinue
$env:KROVI_SHELL_LOG = '1'
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=$CdpPort"
MouseTo 100 700
$null = Start-Process $exe
[void](Wait-Until { (Mode) -eq 'pill' } 10000); Start-Sleep -Seconds 3

$cx = [int]($pill.x + $pill.w / 2); $cy = [int]($pill.y + (10 + 22) * $s)    # visible pill centre, physical px

# 1. out of range: nothing emitted
Clear-Ev; MouseTo 100 700; Start-Sleep -Milliseconds 1500
$e = Ev
Report 'D1 cursor far away: zero shell-cursor events' ($e.Count -eq 0) "events in 1.5s: $($e.Count)"

# 2. move into range: emits with correct vector
Clear-Ev
MouseTo ($cx + 125) $cy; Start-Sleep -Milliseconds 500        # 125 physical px = 100 logical px right of centre
$e = Ev
$last = $e | Select-Object -Last 1
Report 'D2 in range: dx~100 dy~0 dist~100 (logical px from pill centre)' ($e.Count -ge 1 -and [Math]::Abs($last.dx - 100) -le 2 -and [Math]::Abs($last.dy) -le 2 -and [Math]::Abs($last.dist - 100) -le 3) "last=$($last | ConvertTo-Json -Compress); n=$($e.Count)"

# 3. still cursor + 1px jitter: no emits
Clear-Ev; Start-Sleep -Milliseconds 1000
$still = (Ev).Count
MouseTo ($cx + 126) $cy; Start-Sleep -Milliseconds 300; MouseTo ($cx + 125) ($cy + 1); Start-Sleep -Milliseconds 700
$jit = (Ev).Count
Report 'D3 still / 1px jitter (<2 px): no emits' ($still -eq 0 -and $jit -eq 0) "still=$still jitter=$jit"

# 4. continuous sweep for 3 s: rate <= 10 Hz
Clear-Ev
$sw = [Diagnostics.Stopwatch]::StartNew(); $i = 0
while ($sw.ElapsedMilliseconds -lt 3000) { $i++; MouseTo ([int]($cx + 200 * [Math]::Cos($i / 8.0))) ([int]($cy + 150 * [Math]::Sin($i / 8.0))); Start-Sleep -Milliseconds 10 }
Start-Sleep -Milliseconds 300
$e = Ev; $inr = @($e | Where-Object { $_.dist -lt 9000 })
$gaps = for ($k = 1; $k -lt $inr.Count; $k++) { $inr[$k].t - $inr[$k - 1].t }
$minGap = if ($gaps) { ($gaps | Measure-Object -Minimum).Minimum } else { 0 }
$rate = [Math]::Round($inr.Count / 3.0, 1)
# authoritative emitter-side gaps from the shell log (JS receive times jitter by +-25 ms)
$ts = @(Get-Content (Join-Path $env:TEMP 'krovi-shell.log') | Where-Object { $_ -match '\] glance ' } | ForEach-Object { [int](($_ -replace '^\[\s*(\d+)\].*', '$1')) })
$emitGaps = for ($k = 1; $k -lt $ts.Count; $k++) { $ts[$k] - $ts[$k - 1] }
$minEmit = ($emitGaps | Where-Object { $_ -lt 2000 } | Measure-Object -Minimum).Minimum
Report 'D4 continuous motion: <=10 Hz, all dist<=400' ($rate -le 10.5 -and $minEmit -ge 99 -and -not ($inr | Where-Object { $_.dist -gt 400 })) "in-range events=$($inr.Count) in 3s = $rate Hz; emitter min gap=${minEmit}ms (webview-side min $([Math]::Round($minGap))ms = IPC jitter); max dist=$(($inr | Measure-Object dist -Maximum).Maximum)"

# 5. leave range: exactly one final {0,0,9999}, then silence
Clear-Ev; MouseTo 100 700; Start-Sleep -Milliseconds 1500
$e = Ev
Report 'D5 leaving range: one final {dx:0,dy:0,dist:9999}, then silence' ($e.Count -eq 1 -and $e[0].dx -eq 0 -and $e[0].dy -eq 0 -and $e[0].dist -eq 9999) "events=$($e | ConvertTo-Json -Compress)"

# 6. expand panel while cursor in range: final recentre, no pill-mode emits afterwards
MouseTo ($cx + 100) $cy; Start-Sleep -Milliseconds 400
Clear-Ev
Chord; [void](Wait-Until { (Mode) -eq 'panel' } 4000); Start-Sleep -Milliseconds 600
MouseTo ($cx + 130) ($cy + 20); Start-Sleep -Milliseconds 600; MouseTo ($cx + 60) ($cy + 40); Start-Sleep -Milliseconds 600
$e = Ev
Report 'D6 panel open: recentre once, no emits in panel mode' ($e.Count -eq 1 -and $e[0].dist -eq 9999) "events=$($e | ConvertTo-Json -Compress)"
Esc; [void](Wait-Until { (Mode) -eq 'pill' } 4000)

# 7. idle CPU, cursor parked in range vs out of range (Krovi.exe only)
function CpuPct { $k = KroviProc | Select-Object -First 1; $a = (Get-Process -Id $k.Id).TotalProcessorTime.TotalMilliseconds; Start-Sleep -Seconds 10; $b = (Get-Process -Id $k.Id).TotalProcessorTime.TotalMilliseconds; [Math]::Round(($b - $a) / 10000 * 100, 2) }
MouseTo 100 700; Start-Sleep -Seconds 2; $cOut = CpuPct
MouseTo ($cx + 50) $cy; Start-Sleep -Seconds 2; $cIn = CpuPct
Report 'D7 idle CPU unchanged (parked cursor out of range vs in range)' ($cOut -lt 3 -and $cIn -lt 3) "out-of-range=$cOut %  in-range(parked)=$cIn %  of one core, 10 s each"

$null = Start-Process $exe -ArgumentList '--quit'
$ms = Wait-Until { -not (KroviProc) } 8000
$u = Start-Process (Join-Path $install 'uninstall.exe') -ArgumentList '/S' -PassThru; $null = $u.WaitForExit(40000); Start-Sleep -Seconds 3
Report 'D8 cleanup: --quit + uninstall' ((-not (KroviProc)) -and -not (Test-Path $exe)) "quit in ${ms}ms; exe removed=$(-not (Test-Path $exe))"

