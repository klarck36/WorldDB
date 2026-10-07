param(
    [Parameter(Mandatory = $true)]
    [string] $TargetId,

    [Parameter(Mandatory = $true)]
    [ValidatePattern('^0x[0-9a-f]{16}$')]
    [string] $Seed,

    [switch] $PlanOnly,

    [string] $ResultsRoot = 'target/fuzz-results'
)

$ErrorActionPreference = 'Stop'
$workspaceRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$targetTable = Import-Csv -LiteralPath (Join-Path $workspaceRoot 'policy/fuzz-targets.tsv') -Delimiter "`t"
$decoderTable = Import-Csv -LiteralPath (Join-Path $workspaceRoot 'policy/decoder-inventory.tsv') -Delimiter "`t"
$runnerTable = Import-Csv -LiteralPath (Join-Path $workspaceRoot 'policy/fuzz-runners.tsv') -Delimiter "`t"
$resourceTable = Import-Csv -LiteralPath (Join-Path $workspaceRoot 'policy/fuzz-resource-profiles.tsv') -Delimiter "`t"

$target = $targetTable | Where-Object { $_.target_id -ceq $TargetId } | Select-Object -First 1
$decoder = $null
if ($null -eq $target) {
    $decoder = $decoderTable | Where-Object { $_.decoder_id -ceq $TargetId } | Select-Object -First 1
    if ($null -eq $decoder) {
        throw "Unregistered fuzz target: $TargetId"
    }
    $runnerId = if ($decoder.family -eq 'typescript') { 'typescript_transport' } else { 'rust_core_decoder' }
    $runner = $runnerTable | Where-Object { $_.runner_id -ceq $runnerId } | Select-Object -First 1
    $resourceId = $runner.resource_profile
    $corpusName = ($decoder.seed_id -split '/', 2)[0]
    $corpusPath = switch ($corpusName) {
        { $_ -in @('value', 'frame') } { 'crates/worlddb-core/tests/data/wire-v1.0-golden.tsv'; break }
        'record' { 'crates/worlddb-core/tests/data/record-v1.0-golden.tsv'; break }
        'record_ref' { 'crates/worlddb-core/tests/data/record-ref-v1.0-golden.tsv'; break }
        'audit' { 'crates/worlddb-core/tests/data/audit-v1.0-golden.tsv'; break }
        'text' { 'crates/worlddb-core/tests/data/text-parser-v1.0-golden.tsv'; break }
        'core_bytes' { 'crates/worlddb-core/tests/data/core-bytes-v1.0-golden.tsv'; break }
        'generated' { 'policy/fuzz-seeds/core/migration-run-journal.hex'; break }
        'typescript' { 'bindings/typescript/test/data/transport-v1.0-golden.tsv'; break }
        default { throw "No seed corpus route for $($decoder.seed_id)" }
    }
    $seedPaths = @($corpusPath, 'policy/decoder-seeds.tsv')
} else {
    $runner = $runnerTable | Where-Object { $_.runner_id -ceq $target.runner_id } | Select-Object -First 1
    $resourceId = $target.resource_profile
    $seedPaths = @($target.seed_corpus -split ';')
}

if ($null -eq $runner) { throw "No runner is registered for $TargetId" }
$profile = $resourceTable | Where-Object { $_.profile_id -ceq $resourceId } | Select-Object -First 1
if ($null -eq $profile) { throw "No resource profile is registered for $TargetId" }

function Get-FileSha256([string] $Path) {
    $stream = [System.IO.File]::OpenRead($Path)
    try {
        return [Convert]::ToHexString([System.Security.Cryptography.SHA256]::HashData($stream)).ToLowerInvariant()
    } finally {
        $stream.Dispose()
    }
}

function Get-InputCorpusManifest([string[]] $Paths, [long] $MaxInputBytes) {
    $items = [System.Collections.Generic.List[object]]::new()
    $seen = [System.Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    foreach ($relativePath in $Paths) {
        $absolutePath = Join-Path $workspaceRoot $relativePath
        if (Test-Path -LiteralPath $absolutePath -PathType Container) {
            $files = Get-ChildItem -LiteralPath $absolutePath -File -Recurse | Sort-Object FullName
            foreach ($file in $files) {
                $relative = [System.IO.Path]::GetRelativePath($workspaceRoot, $file.FullName).Replace('\', '/')
                if ($file.Length -gt $MaxInputBytes) {
                    throw "Seed file exceeds max_input_bytes ($MaxInputBytes): $relative ($($file.Length) bytes)"
                }
                if ($seen.Add($relative)) {
                    $items.Add([ordered]@{ path = $relative; sha256 = Get-FileSha256 $file.FullName; bytes = $file.Length })
                }
            }
        } elseif (Test-Path -LiteralPath $absolutePath -PathType Leaf) {
            $relative = $relativePath.Replace('\', '/')
            if ($seen.Add($relative)) {
                $file = Get-Item -LiteralPath $absolutePath
                if ($file.Length -gt $MaxInputBytes) {
                    throw "Seed file exceeds max_input_bytes ($MaxInputBytes): $relative ($($file.Length) bytes)"
                }
                $items.Add([ordered]@{ path = $relative; sha256 = Get-FileSha256 $absolutePath; bytes = $file.Length })
            }
        } else {
            throw "Seed corpus path does not exist: $relativePath"
        }
    }
    if ($items.Count -eq 0) { throw "Seed corpus is empty for $TargetId" }
    return ,$items.ToArray()
}

function Get-WorkspaceTreeSha256 {
    $entries = [System.Collections.Generic.List[string]]::new()
    $files = & git -C $workspaceRoot ls-files
    if ($LASTEXITCODE -ne 0) { throw 'Could not enumerate the Git source tree' }
    foreach ($relative in $files) {
        $absolute = Join-Path $workspaceRoot $relative
        if (Test-Path -LiteralPath $absolute -PathType Leaf) {
            $entries.Add("$relative`t$(Get-FileSha256 $absolute)")
        }
    }
    $bytes = [System.Text.Encoding]::UTF8.GetBytes(($entries -join "`n"))
    return [Convert]::ToHexString([System.Security.Cryptography.SHA256]::HashData($bytes)).ToLowerInvariant()
}

function Write-Manifest([object] $Value, [string] $Path) {
    $temporaryPath = "$Path.tmp"
    $json = $Value | ConvertTo-Json -Depth 12
    [System.IO.File]::WriteAllText($temporaryPath, "$json`n", [System.Text.UTF8Encoding]::new($false))
    Move-Item -LiteralPath $temporaryPath -Destination $Path -Force
}

function Resolve-Program([string] $Name) {
    $command = Get-Command $Name -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($null -eq $command -and $Name -eq 'cargo') {
        $fallback = Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe'
        if (Test-Path -LiteralPath $fallback -PathType Leaf) { return $fallback }
    }
    if ($null -eq $command -and $Name -eq 'pnpm') {
        $command = Get-Command 'pnpm.cmd' -ErrorAction SilentlyContinue | Select-Object -First 1
    }
    if ($null -eq $command) { throw "Required program is unavailable: $Name" }
    return $command.Source
}

function Start-CapturedProcess([string] $CommandText, [string] $WorkingDirectory, [string] $StdoutPath, [string] $StderrPath) {
    $parts = @($CommandText -split '\s+' | Where-Object { $_ -ne '' })
    if ($parts.Count -lt 2) { throw "Invalid command definition: $CommandText" }
    $program = Resolve-Program $parts[0]
    $info = [System.Diagnostics.ProcessStartInfo]::new()
    $info.FileName = $program
    $info.WorkingDirectory = $WorkingDirectory
    $info.UseShellExecute = $false
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    foreach ($argument in $parts[1..($parts.Count - 1)]) { [void]$info.ArgumentList.Add($argument) }
    $process = [System.Diagnostics.Process]::new()
    $process.StartInfo = $info
    if (-not $process.Start()) { throw "Could not start $program" }
    $stdoutStream = [System.IO.File]::Create($StdoutPath)
    $stderrStream = [System.IO.File]::Create($StderrPath)
    $stdoutTask = $process.StandardOutput.BaseStream.CopyToAsync($stdoutStream)
    $stderrTask = $process.StandardError.BaseStream.CopyToAsync($stderrStream)
    return @{ process = $process; stdoutTask = $stdoutTask; stderrTask = $stderrTask; stdoutStream = $stdoutStream; stderrStream = $stderrStream }
}

function Complete-CapturedProcess([object] $Captured) {
    $Captured.stdoutTask.GetAwaiter().GetResult()
    $Captured.stderrTask.GetAwaiter().GetResult()
    $Captured.stdoutStream.Dispose()
    $Captured.stderrStream.Dispose()
}

function Get-ProcessTreeStats([int] $RootProcessId) {
    $processes = Get-CimInstance Win32_Process -Property ProcessId, ParentProcessId
    $ids = [System.Collections.Generic.HashSet[int]]::new()
    [void]$ids.Add($RootProcessId)
    do {
        $changed = $false
        foreach ($entry in $processes) {
            if ($ids.Contains([int]$entry.ParentProcessId) -and $ids.Add([int]$entry.ProcessId)) {
                $changed = $true
            }
        }
    } while ($changed)
    $live = @(Get-Process -Id @($ids) -ErrorAction SilentlyContinue)
    $rss = [long](($live | Measure-Object -Property WorkingSet64 -Sum).Sum)
    $cpu = [double](($live | ForEach-Object { $_.TotalProcessorTime.TotalSeconds } | Measure-Object -Sum).Sum)
    return @{ processIds = @($ids); rssBytes = $rss; cpuSeconds = $cpu }
}

function Get-DirectoryBytes([string] $Path) {
    if (-not (Test-Path -LiteralPath $Path)) { return [long]0 }
    return [long]((Get-ChildItem -LiteralPath $Path -File -Recurse | Measure-Object -Property Length -Sum).Sum)
}

$revision = (& git -C $workspaceRoot rev-parse HEAD).Trim()
if ($LASTEXITCODE -ne 0 -or $revision -notmatch '^[0-9a-f]{40}$') { throw 'A full Git source revision is required' }
$dirty = @(& git -C $workspaceRoot status --porcelain --untracked-files=all)
if ($LASTEXITCODE -ne 0) { throw 'Could not inspect the Git working tree' }
$treeSha256 = $null
$corpusManifest = Get-InputCorpusManifest $seedPaths ([long]$profile.max_input_bytes)

$safeTarget = $TargetId -replace '[^a-zA-Z0-9_.-]', '_'
$startedAt = [DateTimeOffset]::UtcNow
$runId = "M9-04-$safeTarget-$($Seed.Substring(2))-$($startedAt.ToString('yyyyMMddTHHmmssZ'))"
$resultsAbsolute = Join-Path $workspaceRoot $ResultsRoot
$runDirectory = Join-Path $resultsAbsolute $runId
$relativeRunDirectory = [System.IO.Path]::GetRelativePath($workspaceRoot, $runDirectory).Replace('\', '/')
$fuzzerReportRelative = "$relativeRunDirectory/fuzzer-report.json"
$resourceSamplesRelative = "$relativeRunDirectory/resource-samples.tsv"
$manifestPath = Join-Path $resultsAbsolute "$runId.manifest.json"
$buildParts = @($runner.build_command -split '\s+' | Where-Object { $_ -ne '' })
$runParts = @($runner.run_command -split '\s+' | Where-Object { $_ -ne '' })
$buildCommand = @($buildParts)
$runCommand = @($runParts)
$targetEnv = $runner.target_selector_env
$durationEnv = $runner.duration_env
$seedEnv = $runner.seed_env
$corpusEnv = $runner.seed_corpus_env
$reportEnv = $runner.report_env
$maxInputEnv = $runner.max_input_env
$inputTimeoutEnv = $runner.input_timeout_env
$manifest = [ordered]@{
    schema_version = 1
    run_id = $runId
    target_id = $TargetId
    runner_id = $runner.runner_id
    source_revision = $revision
    source_tree_sha256 = $treeSha256
    working_tree_clean = ($dirty.Count -eq 0)
    seed = $Seed
    started_at_utc = $startedAt.ToString('yyyy-MM-ddTHH:mm:ss.fffZ')
    finished_at_utc = $null
    duration_requested_seconds = [int]$profile.campaign_duration_seconds
    wall_elapsed_seconds = 0
    cpu_seconds = 0
    resource_profile_id = $profile.profile_id
    peak_process_rss_bytes = 0
    peak_temp_disk_bytes = 0
    input_corpus = $corpusManifest
    build_command = $buildCommand
    run_command = $runCommand
    exit_code = $null
    crash_count = 0
    rounds = 0
    fuzzer_report_path = $null
    resource_samples_path = $null
    crash_corpus_path = $null
    coverage_artifacts_path = $null
    result = 'RUNNING'
}

if ($PlanOnly) {
    [pscustomobject]@{
        TargetId = $TargetId
        Runner = $runner.runner_id
        Build = $runner.build_command
        Run = $runner.run_command
        SeedCorpus = ($seedPaths -join ';')
        DurationSeconds = $profile.campaign_duration_seconds
        WallLimitSeconds = $profile.job_wall_seconds
        MaxInputBytes = $profile.max_input_bytes
        MaxProcessRssBytes = $profile.max_process_rss_bytes
        MaxTempDiskBytes = $profile.max_temp_disk_bytes
        Seed = $Seed
    } | Format-List
    exit 0
}
if ($dirty.Count -gt 0) { throw 'Long fuzz campaigns require a clean Git checkout; commit the target inventory first' }
$treeSha256 = Get-WorkspaceTreeSha256
$manifest.source_tree_sha256 = $treeSha256

New-Item -ItemType Directory -Force -Path $runDirectory | Out-Null
New-Item -ItemType Directory -Force -Path $resultsAbsolute | Out-Null
$manifestPath = Join-Path $resultsAbsolute "$runId.manifest.json"
$buildOutput = Join-Path $runDirectory 'build.stdout.log'
$buildError = Join-Path $runDirectory 'build.stderr.log'
$runOutput = Join-Path $runDirectory 'run.stdout.log'
$runError = Join-Path $runDirectory 'run.stderr.log'
$samplesPath = Join-Path $runDirectory 'resource-samples.tsv'
[System.IO.File]::WriteAllText($samplesPath, "elapsed_seconds`tcpu_seconds`ttree_rss_bytes`trun_directory_bytes`n", [System.Text.UTF8Encoding]::new($false))

$result = 'BUILD_FAILED'
$exitCode = $null
$oldEnvironment = @{}
$oldEnvironment['CARGO_TARGET_DIR'] = [Environment]::GetEnvironmentVariable('CARGO_TARGET_DIR', 'Process')
foreach ($name in @($targetEnv, $durationEnv, $seedEnv, $corpusEnv, $reportEnv, $maxInputEnv, $inputTimeoutEnv, 'WORLDDB_SOURCE_REVISION')) {
    if ($name -and $name -ne '-') { $oldEnvironment[$name] = [Environment]::GetEnvironmentVariable($name, 'Process') }
}
try {
    if ($targetEnv -and $targetEnv -ne '-') { [Environment]::SetEnvironmentVariable($targetEnv, $TargetId, 'Process') }
    [Environment]::SetEnvironmentVariable($durationEnv, [string]$profile.campaign_duration_seconds, 'Process')
    [Environment]::SetEnvironmentVariable($seedEnv, $Seed, 'Process')
    if ($corpusEnv -and $corpusEnv -ne '-') {
        [Environment]::SetEnvironmentVariable($corpusEnv, ($seedPaths -join ';'), 'Process')
    }
    [Environment]::SetEnvironmentVariable($reportEnv, (Join-Path $runDirectory 'fuzzer-report.json'), 'Process')
    [Environment]::SetEnvironmentVariable('WORLDDB_SOURCE_REVISION', $revision, 'Process')
    if ($runner.build_profile -eq 'windows_msvc_locked') {
        [Environment]::SetEnvironmentVariable('CARGO_TARGET_DIR', (Join-Path $runDirectory 'cargo-target'), 'Process')
    }
    Write-Manifest $manifest $manifestPath

    $build = Start-CapturedProcess $runner.build_command $workspaceRoot $buildOutput $buildError
    $build.process.WaitForExit()
    $buildExit = $build.process.ExitCode
    Complete-CapturedProcess $build
    if ($buildExit -ne 0) {
        $exitCode = $buildExit
        $result = 'BUILD_FAILED'
    } elseif ((Get-DirectoryBytes $runDirectory) -gt [long]$profile.max_temp_disk_bytes) {
        $exitCode = $buildExit
        $result = 'RESOURCE_LIMIT'
    } else {
        $result = 'INTERRUPTED'
        if ($maxInputEnv -and $maxInputEnv -ne '-') {
            [Environment]::SetEnvironmentVariable($maxInputEnv, [string]$profile.max_input_bytes, 'Process')
        }
        if ($inputTimeoutEnv -and $inputTimeoutEnv -ne '-') {
            [Environment]::SetEnvironmentVariable($inputTimeoutEnv, [string]$profile.per_input_timeout_seconds, 'Process')
        }
        $run = Start-CapturedProcess $runner.run_command $workspaceRoot $runOutput $runError
        $runStart = [DateTimeOffset]::UtcNow
        $lastSample = [DateTimeOffset]::MinValue
        $limitExceeded = $false
        while (-not $run.process.HasExited) {
            Start-Sleep -Seconds 5
            $stats = Get-ProcessTreeStats $run.process.Id
            $elapsed = ([DateTimeOffset]::UtcNow - $runStart).TotalSeconds
            $directoryBytes = Get-DirectoryBytes $runDirectory
            $manifest.peak_process_rss_bytes = [Math]::Max([long]$manifest.peak_process_rss_bytes, [long]$stats.rssBytes)
            $manifest.peak_temp_disk_bytes = [Math]::Max([long]$manifest.peak_temp_disk_bytes, $directoryBytes)
            $manifest.wall_elapsed_seconds = [Math]::Round($elapsed, 3)
            $manifest.cpu_seconds = [Math]::Round($stats.cpuSeconds, 3)
            if (([DateTimeOffset]::UtcNow - $lastSample).TotalSeconds -ge 30) {
                [System.IO.File]::AppendAllText($samplesPath, "$([Math]::Round($elapsed,3))`t$([Math]::Round($stats.cpuSeconds,3))`t$($stats.rssBytes)`t$directoryBytes`n", [System.Text.UTF8Encoding]::new($false))
                $lastSample = [DateTimeOffset]::UtcNow
            }
            if ($stats.rssBytes -gt [long]$profile.max_process_rss_bytes -or $directoryBytes -gt [long]$profile.max_temp_disk_bytes) {
                $result = 'RESOURCE_LIMIT'
                $limitExceeded = $true
                & taskkill.exe /PID $run.process.Id /T /F | Out-Null
                break
            }
            if ($elapsed -gt [double]$profile.job_wall_seconds) {
                $result = 'TIMEOUT'
                $limitExceeded = $true
                & taskkill.exe /PID $run.process.Id /T /F | Out-Null
                break
            }
        }
        $run.process.WaitForExit()
        $exitCode = $run.process.ExitCode
        Complete-CapturedProcess $run
        if (-not $limitExceeded) {
            $reportFile = Join-Path $runDirectory 'fuzzer-report.json'
            if ($exitCode -eq 0 -and (Test-Path -LiteralPath $reportFile -PathType Leaf)) {
                $fuzzerReport = Get-Content -LiteralPath $reportFile -Raw | ConvertFrom-Json
                $manifest.crash_count = @($fuzzerReport.crashes).Count
                $manifest.rounds = [long]$fuzzerReport.rounds
                $measuredDuration = if ($null -ne $fuzzerReport.cpu_seconds) { [double]$fuzzerReport.cpu_seconds } else { [double]$fuzzerReport.elapsed_seconds }
                $manifest.fuzzer_report_path = "$relativeRunDirectory/fuzzer-report.json"
                if ($manifest.crash_count -gt 0) { $result = 'FAIL' }
                elseif ($measuredDuration -ge [double]$profile.campaign_duration_seconds) { $result = 'PASS_LOCAL' }
                else { $result = 'INTERRUPTED' }
            } else {
                $result = 'FAIL'
            }
        }
    }
} catch {
    if ($result -eq 'RUNNING') { $result = 'FAIL' }
    throw
} finally {
    foreach ($name in $oldEnvironment.Keys) {
        [Environment]::SetEnvironmentVariable($name, $oldEnvironment[$name], 'Process')
    }
    $manifest.finished_at_utc = [DateTimeOffset]::UtcNow.ToString('yyyy-MM-ddTHH:mm:ss.fffZ')
    if (Test-Path -LiteralPath $samplesPath -PathType Leaf) {
        $manifest.resource_samples_path = "$relativeRunDirectory/resource-samples.tsv"
        $manifest.peak_temp_disk_bytes = [Math]::Max([long]$manifest.peak_temp_disk_bytes, (Get-DirectoryBytes $runDirectory))
    }
    if ($null -ne $exitCode) { $manifest.exit_code = $exitCode }
    $manifest.result = $result
    if (Test-Path -LiteralPath $manifestPath) { Write-Manifest $manifest $manifestPath }
}

Write-Output "FUZZ RUN $result : $runId"
Write-Output "Manifest: $([System.IO.Path]::GetRelativePath($workspaceRoot, $manifestPath).Replace('\', '/'))"
if ($result -ne 'PASS_LOCAL') { exit 1 }
