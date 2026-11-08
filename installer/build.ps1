# Builds the Kakangkung installer with Inno Setup 6.5+ (installed with Chocolatey when missing).
# Used by CI and the release workflow:
#   pwsh installer\build.ps1 -Version 0.1.0 -Arch x64 -BinDir target\release
#
# Code signing: when SIGN_PFX_BASE64 (a base64 .pfx) and SIGN_PFX_PASSWORD are set, the program
# file, the setup and its uninstaller are signed (SHA-256, timestamped). Without them the files are
# unsigned and Windows SmartScreen warns on first download.
param(
    [Parameter(Mandatory)] [string] $Version,
    [Parameter(Mandatory)] [ValidateSet('x64', 'arm64')] [string] $Arch,
    [Parameter(Mandatory)] [string] $BinDir,
    [string] $OutDir = 'dist'
)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot

function Find-Iscc {
    Get-ChildItem "${env:ProgramFiles(x86)}\Inno Setup*\ISCC.exe", "$env:ProgramFiles\Inno Setup*\ISCC.exe" -ErrorAction SilentlyContinue |
        Sort-Object FullName | Select-Object -Last 1
}
# ISCC.exe carries no file version; kakangkung.iss itself refuses Inno Setup older than 6.5.
$iscc = Find-Iscc
if (-not $iscc) {
    choco install innosetup -y --no-progress | Out-Null
    $iscc = Find-Iscc
}
if (-not $iscc) { throw 'Inno Setup was not found' }

$bin = Resolve-Path (Join-Path $root $BinDir)
$out = Join-Path $root $OutDir
New-Item -ItemType Directory -Force $out | Out-Null
$isccArgs = @("/DAppVersion=$Version", "/DArch=$Arch", "/DBinDir=$bin", "/DOutDir=$out")

if ($env:SIGN_PFX_BASE64 -and $env:SIGN_PFX_PASSWORD) {
    $signtool = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\signtool.exe" |
        Sort-Object FullName | Select-Object -Last 1
    if (-not $signtool) { throw 'signtool.exe was not found' }
    $pfx = Join-Path ([IO.Path]::GetTempPath()) 'kakangkung-sign.pfx'
    [IO.File]::WriteAllBytes($pfx, [Convert]::FromBase64String($env:SIGN_PFX_BASE64))
    $sign = "`"$($signtool.FullName)`" sign /fd sha256 /tr http://timestamp.digicert.com /td sha256 /f `"$pfx`" /p `"$env:SIGN_PFX_PASSWORD`""
    & cmd /c "$sign `"$bin\kakangkung.exe`""
    if ($LASTEXITCODE) { throw "signing kakangkung.exe failed ($LASTEXITCODE)" }
    $isccArgs += '/DSign', "/Skksign=$sign `$f"
    'Signing: on'
} else {
    'Signing: off (no SIGN_PFX_BASE64 / SIGN_PFX_PASSWORD)'
}

& $iscc.FullName @isccArgs (Join-Path $PSScriptRoot 'kakangkung.iss')
if ($LASTEXITCODE) { throw "Inno Setup failed ($LASTEXITCODE)" }
if ($pfx) { Remove-Item $pfx -Force }
Get-ChildItem $out -Filter "Kakangkung-Setup-$Version-$Arch.exe" | ForEach-Object { "$($_.Name)  $([math]::Round($_.Length / 1MB, 1)) MB" }
