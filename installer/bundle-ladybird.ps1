# Copy kakangkung-ladybird-host (+ optional Lagom runtime) next to kakangkung.exe for packaging.
# Used by CI / release.yml before installer\build.ps1 and the portable zip.
#
#   pwsh installer\bundle-ladybird.ps1 -BinDir target\x86_64-pc-windows-msvc\release
#
# Optional real LibWeb/Lagom (Windows DLLs + share\Lagom) via:
#   -LadybirdRuntimeDir path\to\runtime
#   or env LADYBIRD_RUNTIME_DIR / extract of LADYBIRD_RUNTIME_URL (set by the workflow)
param(
    [Parameter(Mandatory)] [string] $BinDir,
    [string] $LadybirdRuntimeDir = $env:LADYBIRD_RUNTIME_DIR
)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot
$bin = Resolve-Path (Join-Path $root $BinDir)

$hostNames = @(
    'kakangkung-ladybird-host.exe',
    'kakangkung-ladybird-host'
)
$hostSrc = $null
foreach ($n in $hostNames) {
    $c = Join-Path $bin $n
    if (Test-Path $c) { $hostSrc = $c; break }
}
if (-not $hostSrc) {
    throw "kakangkung-ladybird-host missing under $bin — build -p ladybird-host before packaging (KB-38)."
}

$hostDst = Join-Path $bin 'kakangkung-ladybird-host.exe'
if ($hostSrc -ne $hostDst) {
    Copy-Item -Force $hostSrc $hostDst
}

$runtimeBytes = 0L
if ($LadybirdRuntimeDir -and (Test-Path $LadybirdRuntimeDir)) {
    $rt = Resolve-Path $LadybirdRuntimeDir
    $dstRt = Join-Path $bin 'ladybird-runtime'
    if (Test-Path $dstRt) { Remove-Item -Recurse -Force $dstRt }
    Copy-Item -Recurse -Force $rt $dstRt
    $runtimeBytes = (Get-ChildItem -Recurse -File $dstRt | Measure-Object -Property Length -Sum).Sum
    "Ladybird runtime: $dstRt ($([math]::Round($runtimeBytes / 1MB, 1)) MB)"
} else {
    'Ladybird runtime: not supplied (host only). Set LADYBIRD_RUNTIME_DIR or LADYBIRD_RUNTIME_URL for full Lagom.'
}

$hostBytes = (Get-Item $hostDst).Length
"Ladybird host: $hostDst ($([math]::Round($hostBytes / 1KB, 1)) KB)"
if ($hostBytes -lt 50KB) {
    throw "Ladybird host looks empty ($hostBytes bytes)"
}
