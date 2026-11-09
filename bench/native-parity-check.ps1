param([string]$OutputDirectory = (Join-Path $PSScriptRoot '..\..\checks\chrome-parity'), [string]$ExtensionFolder, [string]$Executable)
$ErrorActionPreference = 'Stop'
$project = Split-Path $PSScriptRoot -Parent
$out = [IO.Path]::GetFullPath($OutputDirectory)
$run = Join-Path $out ([DateTime]::UtcNow.ToString('yyyyMMdd-HHmmss-fff'))
New-Item -ItemType Directory -Path $run -Force | Out-Null
$local = Join-Path $run 'synthetic-localappdata'
python (Join-Path $PSScriptRoot 'seed-parity-fixtures.py') $local
if ($LASTEXITCODE -ne 0) { throw 'Could not seed the synthetic browser profiles.' }
$result = Join-Path $run 'selftest.json'
$info = [Diagnostics.ProcessStartInfo]::new()
$info.FileName = if ($Executable) { [IO.Path]::GetFullPath($Executable) } else { Join-Path $project 'target\release\kakangkung.exe' }
$info.ArgumentList.Add('--self-test')
$info.ArgumentList.Add($result)
$info.UseShellExecute = $false
$info.CreateNoWindow = $true
$info.WindowStyle = [Diagnostics.ProcessWindowStyle]::Hidden
$info.RedirectStandardError = $true
$info.RedirectStandardOutput = $true
$info.Environment['LOCALAPPDATA'] = $local
$info.Environment['KAKANGKUNG_TEST_DIR'] = Join-Path $run 'kakangkung-profile'
$info.Environment['KAKANGKUNG_USER_DATA_DIR'] = Join-Path $run 'kakangkung-profile'
$info.Environment['KAKANGKUNG_TEST_AVATAR'] = Join-Path $local 'avatar.png'
$shots = Join-Path $run 'shots'
New-Item -ItemType Directory -Path $shots -Force | Out-Null
$info.Environment['KAKANGKUNG_SHOT_DIR'] = $shots
if ($ExtensionFolder) { $info.Environment['KAKANGKUNG_TEST_EXTENSION'] = [IO.Path]::GetFullPath($ExtensionFolder) }
$process = [Diagnostics.Process]::Start($info)
$errors = $process.StandardError.ReadToEndAsync()
$output = $process.StandardOutput.ReadToEndAsync()
$process.WaitForExit()
[IO.File]::WriteAllText((Join-Path $run 'stderr.log'), $errors.GetAwaiter().GetResult())
[IO.File]::WriteAllText((Join-Path $run 'stdout.log'), $output.GetAwaiter().GetResult())
if (!(Test-Path -LiteralPath $result)) { throw "Self-test exited without a result (exit $($process.ExitCode)); logs: $run" }
$report = Get-Content -LiteralPath $result -Raw | ConvertFrom-Json
$report.steps | ForEach-Object { '{0}: {1}' -f $(if ($_.passed) { 'PASS' } else { 'FAIL' }), $_.step }
Write-Output "Result: $result"
if (@($report.steps | Where-Object { !$_.passed }).Count) { throw 'Native parity check failed.' }
