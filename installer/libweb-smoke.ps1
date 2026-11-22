# Runs a Kakangkung build with its LibWeb runtime through the self-test and fails when:
#   - any console window opens while it runs (the app must never show a terminal),
#   - a self-test step fails,
#   - the stub host was used instead of the LibWeb host (ladybird-runtime\bin).
# Used by the Ladybird runtime workflow (fresh build) and the release workflow (installed copy):
#   pwsh installer\libweb-smoke.ps1 -Exe "$env:LOCALAPPDATA\Programs\Kakangkung\kakangkung.exe"
param(
    [Parameter(Mandatory)] [string] $Exe,
    [string] $ShotDir = (Join-Path $env:RUNNER_TEMP 'shots'),
    [int] $TimeoutSeconds = 240
)
$ErrorActionPreference = 'Stop'

$runtimeHost = Join-Path (Split-Path $Exe) 'ladybird-runtime\bin\kakangkung-ladybird-host.exe'
if (-not (Test-Path $runtimeHost)) { throw "LibWeb host missing: $runtimeHost" }
"LibWeb host: $runtimeHost"

Add-Type @'
using System; using System.Text; using System.Collections.Generic; using System.Runtime.InteropServices;
public static class KkWindows {
    delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc f, IntPtr l);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassName(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    // Visible console hosts: classic conhost, Windows Terminal, pseudo-console.
    public static List<string> Consoles() {
        var found = new List<string>();
        EnumWindows((h, l) => {
            if (!IsWindowVisible(h)) return true;
            var c = new StringBuilder(256); GetClassName(h, c, 256);
            var cls = c.ToString();
            if (cls == "ConsoleWindowClass" || cls == "CASCADIA_HOSTING_WINDOW_CLASS" || cls == "PseudoConsoleWindow") {
                var t = new StringBuilder(512); GetWindowText(h, t, 512); uint pid; GetWindowThreadProcessId(h, out pid);
                found.Add(cls + " pid=" + pid + " '" + t + "'");
            }
            return true;
        }, IntPtr.Zero);
        return found;
    }
}
'@

$before = [KkWindows]::Consoles()
"console windows before start: $($before.Count)"
$tmp = if ($env:RUNNER_TEMP) { $env:RUNNER_TEMP } else { $env:TEMP }
$out = Join-Path $tmp 'kakangkung-selftest.json'
$stderr = Join-Path $tmp 'kakangkung-stderr.log'
Remove-Item $out, $stderr -ErrorAction SilentlyContinue
$env:KAKANGKUNG_SHOT_DIR = $ShotDir
New-Item -ItemType Directory -Force $ShotDir | Out-Null

$p = Start-Process -FilePath $Exe -ArgumentList "--self-test `"$out`"" -PassThru -RedirectStandardError $stderr
$seen = @{}
$deadline = (Get-Date).AddSeconds($TimeoutSeconds)
while (-not $p.HasExited -and (Get-Date) -lt $deadline) {
    foreach ($w in [KkWindows]::Consoles()) { if ($before -notcontains $w) { $seen[$w] = $true } }
    Start-Sleep -Milliseconds 250
}
$timedOut = -not $p.HasExited
if ($timedOut) {
    Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
    Get-Process | Where-Object Name -match '^(kakangkung|WebContent|RequestServer|ImageDecoder|WebWorker)' |
        Stop-Process -Force -ErrorAction SilentlyContinue
}
'--- kakangkung stderr ---'
if (Test-Path $stderr) { Get-Content $stderr }
'-------------------------'
if ($timedOut) { throw "self-test exceeded $TimeoutSeconds s" }
if ($seen.Count) {
    $seen.Keys | ForEach-Object { "CONSOLE WINDOW: $_" }
    throw "$($seen.Count) console window(s) opened while Kakangkung ran"
}
'ok    no console window opened'
if (-not (Test-Path $out)) { throw "self-test wrote no result (exit $($p.ExitCode))" }
$r = Get-Content $out -Raw | ConvertFrom-Json
$r.steps | ForEach-Object { '{0,-5} {1}: {2}' -f ($(if ($_.passed) { 'PASS' } else { 'FAIL' })), $_.step, $_.detail }
$engine = $r.steps | Where-Object step -eq 'ladybird' | ForEach-Object detail
if ($engine -notmatch 'engine=ladybird') { throw "the LibWeb host was not used: $engine" }
'ok    LibWeb host used (engine=ladybird)'
$failed = @($r.steps | Where-Object { -not $_.passed })
if ($failed.Count) { throw "$($failed.Count) self-test step(s) failed" }
"ok    all $(@($r.steps).Count) self-test steps passed"
