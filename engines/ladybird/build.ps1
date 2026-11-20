# Build kakangkung-ladybird-host on Windows.
# Prefer Boat (https://boat.dev/dashboard) for full Ladybird builds; this script is for the stub
# host or for a machine that already has Ladybird built under WSL2 / clang-cl.
param(
    [switch]$Stub,
    [switch]$Ladybird,
    [string]$LadybirdSourceDir = $env:LADYBIRD_SOURCE_DIR,
    [string]$LadybirdBuildDir = $env:LADYBIRD_BUILD_DIR
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $MyInvocation.MyCommand.Path
$Out = Join-Path $Root "out"
New-Item -ItemType Directory -Force -Path $Out | Out-Null

if ($Ladybird) {
    if (-not $LadybirdSourceDir -or -not $LadybirdBuildDir) {
        throw "Pass -LadybirdSourceDir and -LadybirdBuildDir (or set LADYBIRD_* env vars). Full Ladybird builds are meant to run on Boat: https://boat.dev/dashboard"
    }
    cmake -S $Root -B $Out -DHAVE_LADYBIRD=ON "-DLADYBIRD_SOURCE_DIR=$LadybirdSourceDir" "-DLADYBIRD_BUILD_DIR=$LadybirdBuildDir"
} else {
    cmake -S $Root -B $Out -DHAVE_LADYBIRD=OFF
}

cmake --build $Out --config Release
$exe = Join-Path $Out "kakangkung-ladybird-host.exe"
if (-not (Test-Path $exe)) { $exe = Join-Path $Out "Release\kakangkung-ladybird-host.exe" }
Write-Host "Built: $exe"
& $exe --version
