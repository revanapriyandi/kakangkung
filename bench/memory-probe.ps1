param([string[]]$Only = @(), [string]$Exe = (Join-Path $PSScriptRoot '../target/release/kakangkung.exe'))
$ErrorActionPreference = 'Stop'
[cultureinfo]::CurrentCulture = 'en-US'
$Only = @($Only -split ',') | Where-Object { $_ }
$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$output = Join-Path $PSScriptRoot "results/memory-probe-$stamp"
New-Item -ItemType Directory -Path $output -Force | Out-Null
$resolvedExe = (Resolve-Path -LiteralPath $Exe).Path
$binaryHash = (Get-FileHash $resolvedExe).Hash
function Read-Tree([int]$RootId) {
    $all = @(Get-CimInstance Win32_Process)
    $tree = [System.Collections.Generic.List[object]]::new()
    $rootProcess = $all | Where-Object ProcessId -eq $RootId
    if (!$rootProcess) { throw 'Benchmark application exited before sampling' }
    $tree.Add($rootProcess)
    for ($i = 0; $i -lt $tree.Count; $i++) {
        $parent = $tree[$i]
        foreach ($child in $all | Where-Object { $_.ParentProcessId -eq $parent.ProcessId -and $_.CreationDate -ge $parent.CreationDate }) { $tree.Add($child) }
    }
    $privateWs = @{}
    Get-CimInstance Win32_PerfFormattedData_PerfProc_Process | ForEach-Object { $privateWs[[int]$_.IDProcess] = [long]$_.WorkingSetPrivate }
    foreach ($process in $tree) {
        [pscustomobject]@{ pid=$process.ProcessId; name=$process.Name; command=$process.CommandLine; private_ws_bytes=$privateWs[[int]$process.ProcessId]; commit_bytes=[long]$process.PrivatePageCount; cpu_total_seconds=[math]::Round(([double]$process.KernelModeTime+[double]$process.UserModeTime)/10000000,3) }
    }
}
$scenarios = @(
    @{name='native'; urls=@('kakangkung://newtab'); saver=$false; wait=20},
    @{name='one-web'; urls=@('https://example.com'); saver=$false; wait=20},
    @{name='four-web'; urls=@('https://example.com','https://example.org','https://example.net','https://www.iana.org/domains/reserved'); saver=$false; wait=20},
    @{name='four-web-balanced-45'; urls=@('https://example.com','https://example.org','https://example.net','https://www.iana.org/domains/reserved'); saver=$false; wait=45},
    @{name='four-real-balanced'; urls=@('https://www.youtube.com/watch?v=aqz-KE-bpKQ','https://github.com/revanapriyandi','https://www.instagram.com/','https://www.facebook.com/'); saver=$false; wait=45},
    @{name='four-real-active-youtube'; urls=@('https://github.com/revanapriyandi','https://www.instagram.com/','https://www.facebook.com/','https://www.youtube.com/watch?v=aqz-KE-bpKQ'); saver=$true; wait=45},
    @{name='four-real-ultralite'; urls=@('https://www.youtube.com/watch?v=aqz-KE-bpKQ','https://github.com/revanapriyandi','https://www.instagram.com/','https://www.facebook.com/'); saver=$true; wait=45},
    @{name='four-web-ultralite'; urls=@('https://example.com','https://example.org','https://example.net','https://www.iana.org/domains/reserved'); saver=$true; wait=45}
)
$summary = @()
foreach ($scenario in $scenarios) {
    if ($Only.Count -and $scenario.name -notin $Only) { continue }
    $profile = Join-Path ([IO.Path]::GetTempPath()) "kk-memory-probe-$stamp-$($scenario.name)"
    New-Item -ItemType Directory -Path $profile | Out-Null
    & {
        $mode = if ($scenario.saver) { 'ultralite' } else { 'balanced' }
        python -c "import sqlite3,sys;c=sqlite3.connect(sys.argv[1]);c.execute('CREATE TABLE settings(key TEXT PRIMARY KEY,value TEXT NOT NULL)');c.execute('INSERT INTO settings VALUES(?,?)',('memory.mode',sys.argv[2]));c.execute('CREATE TABLE history(url TEXT PRIMARY KEY,title TEXT NOT NULL DEFAULT '+chr(39)*2+',visit_count INTEGER NOT NULL DEFAULT 0,last_visit INTEGER NOT NULL DEFAULT 0)');c.execute('CREATE INDEX history_last ON history(last_visit DESC)');c.execute('PRAGMA user_version=1');c.commit()" (Join-Path $profile 'kakangkung.db') $mode
        if ($LASTEXITCODE) { throw 'Could not seed saver setting' }
    }
    $arguments = @("--user-data-dir=$profile",'--no-first-run') + $scenario.urls
    $process = Start-Process -FilePath $resolvedExe -ArgumentList $arguments -WindowStyle Hidden -PassThru -RedirectStandardError (Join-Path $output "$($scenario.name).log")
    try {
        Start-Sleep -Seconds $scenario.wait
        $rows = @(Read-Tree $process.Id)
        $rows | Export-Csv -NoTypeInformation (Join-Path $output "$($scenario.name)-processes.csv")
        $summary += [pscustomobject]@{ scenario=$scenario.name; tabs=$scenario.urls.Count; seconds=$scenario.wait; processes=$rows.Count; private_ws_mib=[math]::Round(($rows | Measure-Object private_ws_bytes -Sum).Sum / 1MB,2); commit_mib=[math]::Round(($rows | Measure-Object commit_bytes -Sum).Sum / 1MB,2); cpu_total_seconds=[math]::Round(($rows | Measure-Object cpu_total_seconds -Sum).Sum,3); profile=$profile }
        $summary | Export-Csv -NoTypeInformation (Join-Path $output 'summary.csv')
        $summary[-1] | ConvertTo-Json -Compress | Write-Output
    } finally {
        if (!$process.HasExited) { & taskkill.exe /PID $process.Id /T /F | Out-Null }
    }
}
$summary | Export-Csv -NoTypeInformation (Join-Path $output 'summary.csv')
@{ executable=$resolvedExe; sha256=$binaryHash; time=(Get-Date).ToString('o'); runs=1; urls=$scenarios; metric='whole process tree private working set and private commit; isolated fresh profiles; no working set trimming; one run per scenario' } | ConvertTo-Json -Depth 5 | Set-Content -Encoding utf8 (Join-Path $output 'method.json')
Write-Output "Evidence: $output"
