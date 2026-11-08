# Memory benchmark: Kakangkung vs Chrome vs Edge (docs/BENCHMARKS.md, task section 7).
#
# For each browser and tab count: start with a fresh profile and the URLs below as tabs, wait
# $Settle seconds, sum the private working set ("Memory" in Task Manager) of the browser's whole
# process tree, close it. $Runs runs each; raw rows go to bench/results/<time>-processes.csv and the
# median/range table is written into docs/BENCHMARKS.md between the result markers.
#
# "kakangkung-saver" is Kakangkung with Memory Saver at Maximum (freeze after 30 s hidden),
# measured after $SaverSettle seconds: the "after suspension" scenario. Seeding that setting
# needs Python (sqlite3).
#
#   pwsh bench/run.ps1                       # everything (about 35 minutes)
#   pwsh bench/run.ps1 -Tabs 1 -Runs 1       # quick look
param(
    [string[]]$Tabs = @('1', '10', '25', '50', '100'),
    [int]$Runs = 3,
    # Seconds to wait, plus $PerTab for every tab (100 tabs take a while to load).
    [int]$Settle = 30,
    [int]$SaverSettle = 90,
    [double]$PerTab = 0.6,
    [string[]]$Browsers = @('kakangkung', 'kakangkung-saver', 'chrome', 'edge')
)
$ErrorActionPreference = 'Stop'
# `pwsh -File run.ps1 -Tabs 1,5` passes "1,5" as one string.
$Tabs = @($Tabs -split ',' | ForEach-Object { [int]$_ })
$Browsers = @($Browsers -split ',')
[cultureinfo]::CurrentCulture = 'en-US'   # 1,564 MB, not 1.564 MB
$root = Split-Path $PSScriptRoot -Parent

# Tab i opens $Urls[i % count]: what people keep open, all without signing in. Ad-heavy news,
# video, shopping, social, maps and web apps; 25+ tabs open some sites twice, as people do.
$Urls = @(
    'https://www.youtube.com/watch?v=jNQXAC9IVRw',        # video
    'https://www.kompas.com/',                            # news (ID), many ads
    'https://www.google.com/maps/@-6.2,106.82,12z',       # web app, WebGL map
    'https://www.tokopedia.com/',                         # shopping
    'https://www.reddit.com/r/indonesia/',                # social, infinite feed
    'https://github.com/rust-lang/rust',                  # developer site
    'https://www.detik.com/',                             # news (ID), many ads
    'https://stackoverflow.com/questions/tagged/rust',    # Q&A
    'https://www.amazon.com/',                            # shopping
    'https://www.bbc.com/news',                           # news
    'https://excalidraw.com/',                            # web app, canvas
    'https://translate.google.com/',                      # web app
    'https://www.cnn.com/',                               # news, video ads
    'https://en.wikipedia.org/wiki/Web_browser'           # article
)
$Exe = @{
    kakangkung = Join-Path $root 'target\release\kakangkung.exe'
    'kakangkung-saver' = Join-Path $root 'target\release\kakangkung.exe'
    chrome     = 'C:\Program Files\Google\Chrome\Application\chrome.exe'
    edge       = 'C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe'
}
# Memory-saving features are left at each browser's default for a fresh profile (stated in BENCHMARKS.md).
function Get-Args($browser, $profileDir, $urls) {
    $common = @("--user-data-dir=$profileDir", '--no-first-run')
    if ($browser -notlike 'kakangkung*') { $common += @('--no-default-browser-check', '--window-size=1280,800') }
    return $common + $urls
}

# Every process whose parent chain reaches $rootId, with its private working set. A child must
# be younger than its parent: a recycled parent PID would otherwise pull in unrelated processes.
function Get-Tree([int]$rootId) {
    $ws = @{}
    Get-CimInstance Win32_PerfFormattedData_PerfProc_Process | ForEach-Object { $ws[[int]$_.IDProcess] = [int64]$_.WorkingSetPrivate }
    $byPid = @{}; $byParent = @{}
    foreach ($p in Get-CimInstance Win32_Process) { $byPid[[int]$p.ProcessId] = $p; $byParent[[int]$p.ParentProcessId] += , $p }
    $tree = @(); $queue = [System.Collections.Generic.Queue[int]]::new()
    if ($byPid[$rootId]) { $tree += $byPid[$rootId]; $queue.Enqueue($rootId) }
    while ($queue.Count) {
        $parent = $byPid[$queue.Dequeue()]
        foreach ($c in $byParent[[int]$parent.ProcessId]) {
            if ($c.CreationDate -ge $parent.CreationDate) { $tree += $c; $queue.Enqueue([int]$c.ProcessId) }
        }
    }
    return $tree | ForEach-Object { [pscustomobject]@{ IDProcess = $_.ProcessId; Name = $_.Name; WorkingSetPrivate = $ws[[int]$_.ProcessId]; Commit = [int64]$_.PrivatePageCount } }
}

# Fresh Kakangkung profile with Memory Saver at Maximum: start once so the database exists, then set it.
function Initialize-Saver($profileDir) {
    $p = Start-Process $Exe.kakangkung -ArgumentList @("--user-data-dir=$profileDir", '--no-first-run', 'about:blank') -PassThru
    Start-Sleep -Seconds 8
    Get-Tree $p.Id | ForEach-Object { Stop-Process -Id $_.IDProcess -Force -ErrorAction SilentlyContinue }
    Start-Sleep -Seconds 3
    python -c "import sqlite3,sys; c=sqlite3.connect(sys.argv[1]); c.execute('INSERT INTO settings(key,value) VALUES(?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value', ('memory.mode','ultralite')); c.commit()" (Join-Path $profileDir 'kakangkung.db')
    if ($LASTEXITCODE) { throw 'seeding Memory Saver failed (is Python installed?)' }
}

function Median([double[]]$xs) {
    $s = $xs | Sort-Object; $n = $s.Count
    if ($n % 2) { return $s[($n - 1) / 2] } else { return ($s[$n / 2 - 1] + $s[$n / 2]) / 2 }
}

$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$resultsDir = Join-Path $root 'bench\results'
New-Item -ItemType Directory -Force $resultsDir | Out-Null
$rawFile = Join-Path $resultsDir "$stamp-processes.csv"
$raw = [System.Collections.Generic.List[object]]::new()

foreach ($browser in $Browsers) {
    if (-not (Test-Path $Exe[$browser])) { Write-Warning "$browser not found at $($Exe[$browser]), skipped"; continue }
    foreach ($n in $Tabs) {
        $urls = 0..($n - 1) | ForEach-Object { $Urls[$_ % $Urls.Count] }
        foreach ($run in 1..$Runs) {
            $profileDir = Join-Path ([IO.Path]::GetTempPath()) "kk-bench-$browser-$n-$run-$stamp"
            Write-Host "$browser, $n tab(s), run $run ..." -NoNewline
            if ($browser -eq 'kakangkung-saver') { Initialize-Saver $profileDir }
            $proc = Start-Process $Exe[$browser] -ArgumentList (Get-Args $browser $profileDir $urls) -PassThru
            Start-Sleep -Seconds ($(if ($browser -eq 'kakangkung-saver') { $SaverSettle } else { $Settle }) + [int]($PerTab * $n))
            $tree = Get-Tree $proc.Id
            foreach ($p in $tree) {
                $raw.Add([pscustomobject]@{
                        browser = $browser; tabs = $n; run = $run; pid = $p.IDProcess; name = $p.Name
                        private_ws_bytes = [int64]$p.WorkingSetPrivate; commit_bytes = $p.Commit
                    })
            }
            $mb = ($tree | Measure-Object WorkingSetPrivate -Sum).Sum / 1MB
            Write-Host (" {0} processes, {1:N0} MB" -f $tree.Count, $mb)
            $tree | ForEach-Object { Stop-Process -Id $_.IDProcess -Force -ErrorAction SilentlyContinue }
            Start-Sleep -Seconds 3
            Remove-Item -Recurse -Force $profileDir -ErrorAction SilentlyContinue
        }
    }
}
$raw | Export-Csv -NoTypeInformation $rawFile
Write-Host "raw: $rawFile"

# Summary: total per run, then median and range over runs.
$lines = @(
    "Run $stamp; $Runs run(s) each, measured $Settle s + $PerTab s per tab after start ($SaverSettle s + $PerTab s per tab for kakangkung-saver); raw data: ``bench/results/$stamp-processes.csv``.",
    '',
    '| Browser | Tabs | Processes (median) | Private working set, median (MB) | Range (MB) | Commit, median (MB) |',
    '|---|---:|---:|---:|---|---:|'
)
foreach ($g in ($raw | Group-Object browser, tabs)) {
    $perRun = $g.Group | Group-Object run
    $totals = $perRun | ForEach-Object { ($_.Group | Measure-Object private_ws_bytes -Sum).Sum / 1MB }
    $counts = $perRun | ForEach-Object { $_.Count }
    $commits = $perRun | ForEach-Object { ($_.Group | Measure-Object commit_bytes -Sum).Sum / 1MB }
    $min = ($totals | Measure-Object -Minimum).Minimum; $max = ($totals | Measure-Object -Maximum).Maximum
    $lines += '| {0} | {1} | {2} | {3:N0} | {4:N0} – {5:N0} | {6:N0} |' -f $g.Group[0].browser, $g.Group[0].tabs, (Median $counts), (Median $totals), $min, $max, (Median $commits)
}
$table = $lines -join "`n"
Write-Host "`n$table"

$doc = Join-Path $root 'docs\BENCHMARKS.md'
$text = Get-Content -Raw $doc
$start = '<!-- results -->'; $end = '<!-- /results -->'
$i = $text.IndexOf($start); $j = $text.IndexOf($end)
if ($i -lt 0 -or $j -lt $i) { throw "result markers not found in $doc" }
$text = $text.Substring(0, $i + $start.Length) + "`n$table`n" + $text.Substring($j)
Set-Content -NoNewline -Encoding utf8 $doc $text
Write-Host "updated $doc"
