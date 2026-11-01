<#
.SYNOPSIS
  Builds the Krovi website: syncs the shared header/footer, builds the app demo into web/demo/,
  and verifies that web/ is deployable (marketing site at the root, demo under /demo/).

.DESCRIPTION
  The website is the static site in web/. The app demo is a SEPARATE route: the Vite app is built with
  --base=/demo/ into web/demo/ and is served at /demo/ (and embedded on the home page as /demo/?embed=1).
  The Vite "dist" must never be deployed as the site root, so this script refuses to continue unless
  web/index.html carries the marketing marker comment and the verification step passes.

  This script does NOT deploy unless you pass -Deploy (and confirm). Nothing here pushes or tags.

.PARAMETER SkipDemo   Skip the Vite build (fast iteration on the site only; web/demo/ must already exist).
.PARAMETER Serve      After building, serve web/ locally with the headers from web/vercel.json.
.PARAMETER Port       Port for -Serve (default 5500).
.PARAMETER External   Also request every external link to prove it resolves (needs network).
.PARAMETER Deploy     After a clean verification, run `vercel deploy --prod` from web/ (asks first).
.PARAMETER Yes        With -Deploy: skip the confirmation prompt.

.EXAMPLE
  powershell -File scripts\build-web.ps1                 # sync + build demo + verify
  powershell -File scripts\build-web.ps1 -SkipDemo -Serve
  powershell -File scripts\build-web.ps1 -External       # before a release: check outbound links too
  powershell -File scripts\build-web.ps1 -Deploy         # guarded production deploy
#>
[CmdletBinding()]
param(
  [switch]$SkipDemo,
  [switch]$Serve,
  [int]$Port = 5500,
  [switch]$External,
  [switch]$Deploy,
  [switch]$Yes
)

$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$web = Join-Path $root 'web'
$marker = '<!-- krovi-marketing-site -->'

function Step([string]$m) { Write-Host "==> $m" -ForegroundColor Cyan }
function Fail([string]$m) { Write-Host "FAIL: $m" -ForegroundColor Red; exit 1 }
function Need-Marker {
  $index = Join-Path $web 'index.html'
  if (-not (Test-Path $index)) { Fail 'web/index.html is missing.' }
  if (-not (Select-String -Path $index -SimpleMatch -Quiet -Pattern $marker)) {
    Fail "web/index.html does not contain the marker $marker. It may be the Vite app bundle; refusing to continue."
  }
}

Set-Location $root

Step 'Guard: web/index.html must be the marketing page'
Need-Marker

Step 'Sync the shared header/footer into every page'
& node (Join-Path $root 'scripts/build-web.mjs') sync
if ($LASTEXITCODE -ne 0) { Fail 'partial sync failed.' }

if ($SkipDemo) {
  Write-Host '    -SkipDemo: keeping the existing web/demo/ build.' -ForegroundColor Yellow
} else {
  Step 'Build the app demo -> web/demo (base /demo/)'
  # vite.config.ts sets base './' and outDir 'dist'; these CLI flags override both, so the config is untouched.
  & npx vite build --base=/demo/ --outDir web/demo --emptyOutDir
  if ($LASTEXITCODE -ne 0) { Fail 'vite build failed.' }
}

Step 'Verify: markers, links, anchors, CSP hygiene, zero third-party requests'
$checkArgs = @('check')
if ($External) { $checkArgs += '--external' }
& node (Join-Path $root 'scripts/build-web.mjs') @checkArgs
if ($LASTEXITCODE -ne 0) { Fail 'verification failed; fix the problems above.' }
Need-Marker

$demoFiles = (Get-ChildItem (Join-Path $web 'demo') -Recurse -File | Measure-Object -Property Length -Sum)
Write-Host ("    web/demo: {0} files, {1:N0} KB" -f $demoFiles.Count, ($demoFiles.Sum / 1KB))

if ($Deploy) {
  Step 'Deploy (production) from web/'
  if (-not $Yes) {
    $answer = Read-Host 'Deploy web/ to Vercel production? Type "deploy" to continue'
    if ($answer -ne 'deploy') { Fail 'Aborted by user.' }
  }
  Set-Location $web
  & vercel deploy --prod
  exit $LASTEXITCODE
}

if ($Serve) {
  Step "Serving web/ at http://localhost:$Port  (Ctrl+C to stop)"
  & node (Join-Path $root 'scripts/build-web.mjs') serve $Port
  exit $LASTEXITCODE
}

Write-Host ''
Write-Host 'OK. web/ is ready.' -ForegroundColor Green
Write-Host '  Preview : powershell -File scripts\build-web.ps1 -SkipDemo -Serve   (http://localhost:5500)'
Write-Host '  Deploy  : powershell -File scripts\build-web.ps1 -Deploy            (guarded)  or  cd web; vercel deploy --prod'
