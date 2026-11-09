param(
    [Parameter(Mandatory)] [string] $Baseline,
    [Parameter(Mandatory)] [string] $Candidate,
    [ValidateRange(1, 5)] [int] $Repeats = 2,
    [string] $Scenario = 'four-real-ultralite'
)
$ErrorActionPreference = 'Stop'
$executables = @{ baseline = (Resolve-Path -LiteralPath $Baseline).Path; candidate = (Resolve-Path -LiteralPath $Candidate).Path }
$output = Join-Path $PSScriptRoot ('results/memory-compare-' + (Get-Date -Format 'yyyyMMdd-HHmmss'))
New-Item -ItemType Directory -Path $output -Force | Out-Null
$previousArguments = $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS
$previousTrace = $env:KAKANGKUNG_MEMORY_TRACE
$rows = @()
try {
    # Alternate AB / BA; both binaries use ordinary engine arguments and fresh profiles.
    [Environment]::SetEnvironmentVariable('WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS', $null, 'Process')
    foreach ($iteration in 1..$Repeats) {
        $order = if ($iteration % 2) { @('baseline', 'candidate') } else { @('candidate', 'baseline') }
        foreach ($variant in $order) {
            $label = "$iteration-$variant"
            $env:KAKANGKUNG_MEMORY_TRACE = Join-Path $output "$label-native.jsonl"
            Write-Output "Measuring $label"
            $messages = @(& (Join-Path $PSScriptRoot 'memory-probe.ps1') -Only $Scenario -Exe $executables[$variant])
            $messages | Write-Output
            $evidence = @($messages | Where-Object { $_ -is [string] -and $_.StartsWith('Evidence: ') })
            if ($evidence.Count -ne 1) { throw "Missing evidence directory for $label" }
            $directory = $evidence[0].Substring('Evidence: '.Length)
            $method = Get-Content -LiteralPath (Join-Path $directory 'method.json') -Raw | ConvertFrom-Json
            $samples = @(Import-Csv -LiteralPath (Join-Path $directory 'summary.csv'))
            if ($samples.Count -ne 1) { throw 'Choose exactly one scenario per comparison.' }
            $sample = $samples[0]
            $rows += [pscustomobject]@{
                iteration = $iteration; variant = $variant; scenario = $sample.scenario
                private_ws_mib = [double]$sample.private_ws_mib; commit_mib = [double]$sample.commit_mib
                cpu_total_seconds = [double]$sample.cpu_total_seconds; processes = [int]$sample.processes
                sha256 = $method.sha256; evidence = $directory
            }
            $rows | Export-Csv -NoTypeInformation -LiteralPath (Join-Path $output 'comparison.csv')
        }
    }
    foreach ($variant in @('baseline', 'candidate')) {
        $group = @($rows | Where-Object variant -eq $variant)
        [pscustomobject]@{
            variant = $variant; runs = $group.Count
            private_ws_mib_mean = [math]::Round(($group | Measure-Object private_ws_mib -Average).Average, 2)
            commit_mib_mean = [math]::Round(($group | Measure-Object commit_mib -Average).Average, 2)
        } | ConvertTo-Json -Compress | Write-Output
    }
    Write-Output "Comparison: $output"
} finally {
    [Environment]::SetEnvironmentVariable('WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS', $previousArguments, 'Process')
    [Environment]::SetEnvironmentVariable('KAKANGKUNG_MEMORY_TRACE', $previousTrace, 'Process')
}
