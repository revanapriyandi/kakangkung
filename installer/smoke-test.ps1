# Installs a built Kakangkung setup silently for the current user, checks what it left, then
# uninstalls it and checks that everything is gone (the person's data folder is kept).
# Used by CI and the release workflow:  pwsh installer\smoke-test.ps1 dist\Kakangkung-Setup-….exe
param(
    [Parameter(Mandatory)] [string] $Setup,
    [string] $Language = 'indonesian',
    [string] $Expect = 'id'
)
$ErrorActionPreference = 'Stop'

$app = Join-Path $env:LOCALAPPDATA 'Programs\Kakangkung'
$exe = Join-Path $app 'kakangkung.exe'
$link = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Kakangkung.lnk'
$uninstallKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\{B62D7E15-F7C0-4FC1-9F78-A27C8FC6A39E}_is1'
$settingsKey = 'HKCU:\Software\Kakangkung'
$log = Join-Path $env:RUNNER_TEMP 'kakangkung-setup.log'

function Check([bool] $ok, [string] $what) {
    if (-not $ok) { throw "installer smoke test: $what" }
    "ok    $what"
}

$p = Start-Process -FilePath $Setup -PassThru -Wait -ArgumentList `
    '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/CURRENTUSER', "/LANG=$Language", "/LOG=`"$log`""
if ($p.ExitCode) { Get-Content $log -Tail 40; throw "setup exited with $($p.ExitCode)" }

Check (Test-Path $exe) "program installed in $app"
Check ((Get-Item $exe).Length -gt 1MB) 'program file is the real build'
Check (Test-Path $link) 'Start menu shortcut'
Check ((Get-ItemProperty $settingsKey -ErrorAction SilentlyContinue).Language -eq $Expect) "interface language '$Expect' from the installer"
$entry = Get-ItemProperty $uninstallKey -ErrorAction SilentlyContinue
Check ($null -ne $entry) 'listed in Installed apps'
Check ($entry.DisplayVersion -match '^\d+\.\d+\.\d+') "version $($entry.DisplayVersion) in Installed apps"

# The uninstaller copies itself to %TEMP% and returns at once: wait for the program to go.
$uninstaller = $entry.UninstallString.Trim('"')
Start-Process -FilePath $uninstaller -Wait -ArgumentList '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART'
$deadline = (Get-Date).AddSeconds(90)
while ((Test-Path $exe) -or (Test-Path $uninstallKey)) {
    if ((Get-Date) -gt $deadline) { break }
    Start-Sleep -Milliseconds 500
}
Check (-not (Test-Path $exe)) 'program removed'
Check (-not (Test-Path $link)) 'Start menu shortcut removed'
Check (-not (Test-Path $uninstallKey)) 'removed from Installed apps'
Check (-not (Test-Path $settingsKey)) 'installer language setting removed'
'installer smoke test passed'
