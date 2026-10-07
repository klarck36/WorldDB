param(
    [Parameter(Mandatory = $true)]
    [string]$InProcessExecutable,
    [Parameter(Mandatory = $true)]
    [string]$UpdatedInProcessExecutable,
    [Parameter(Mandatory = $true)]
    [string]$SidecarExecutable,
    [Parameter(Mandatory = $true)]
    [string]$EngineExecutable,
    [Parameter(Mandatory = $true)]
    [string]$UpdatedEngineExecutable,
    [switch]$KeepArtifacts
)

$ErrorActionPreference = 'Stop'
$tempBase = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
$testRoot = Join-Path $tempBase ('worlddb-ode002-matrix-' + [guid]::NewGuid().ToString('N'))
$null = New-Item -ItemType Directory -Path $testRoot
$inProcessCopy = Join-Path $testRoot 'in-process-app.exe'
$updatedInProcessCopy = Join-Path $testRoot 'in-process-app-updated.exe'
$sidecarCopy = Join-Path $testRoot 'sidecar-app.exe'
$engineV1 = Join-Path $testRoot 'engine-v1.exe'
$engineV2 = Join-Path $testRoot 'engine-v2.exe'

function Assert-Condition([bool]$Condition, [string]$Message) {
    if (-not $Condition) {
        throw $Message
    }
}

function Wait-ForResult([System.Diagnostics.Process]$Process, [string]$Path) {
    $deadline = [DateTime]::UtcNow.AddSeconds(90)
    while (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        $Process.Refresh()
        if ($Process.HasExited) {
            $stderrPath = Join-Path $testRoot (([System.IO.Path]::GetFileNameWithoutExtension($Path)) + '.stderr.log')
            $stdoutPath = Join-Path $testRoot (([System.IO.Path]::GetFileNameWithoutExtension($Path)) + '.stdout.log')
            $diagnostic = Get-Content -LiteralPath $stderrPath -Raw -ErrorAction SilentlyContinue
            $output = Get-Content -LiteralPath $stdoutPath -Raw -ErrorAction SilentlyContinue
            throw "Desktop process exited before writing its report (exit $($Process.ExitCode)). stderr: $diagnostic stdout: $output"
        }
        if ([DateTime]::UtcNow -ge $deadline) {
            throw 'Timed out waiting for the ODE-002 matrix report.'
        }
        Start-Sleep -Milliseconds 100
    }
    return Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json
}

function Start-HiddenApp([string]$Executable, [string]$Name, [string]$Database, [string]$Report, [string]$HoldMs, [bool]$RunStream, [bool]$RunPanic, [bool]$RunUpdate, [string]$EnginePath, [string]$UpdatePath) {
    $env:WORLDDB_ODE_DATABASE = $Database
    $env:WORLDDB_ODE_RESULT = $Report
    $env:WORLDDB_ODE_AUTOCLOSE_MS = $HoldMs
    if ($RunStream) { $env:WORLDDB_ODE_STREAM_TEST = '1' } else { Remove-Item Env:\WORLDDB_ODE_STREAM_TEST -ErrorAction SilentlyContinue }
    if ($RunPanic) { $env:WORLDDB_ODE_PANIC_TEST = '1' } else { Remove-Item Env:\WORLDDB_ODE_PANIC_TEST -ErrorAction SilentlyContinue }
    if ($RunUpdate) { $env:WORLDDB_ODE_UPDATE_TEST = '1' } else { Remove-Item Env:\WORLDDB_ODE_UPDATE_TEST -ErrorAction SilentlyContinue }
    if ($EnginePath) { $env:WORLDDB_ODE_ENGINE_EXECUTABLE = $EnginePath } else { Remove-Item Env:\WORLDDB_ODE_ENGINE_EXECUTABLE -ErrorAction SilentlyContinue }
    if ($UpdatePath) { $env:WORLDDB_ODE_ENGINE_UPDATE = $UpdatePath } else { Remove-Item Env:\WORLDDB_ODE_ENGINE_UPDATE -ErrorAction SilentlyContinue }

    $stdout = Join-Path $testRoot ($Name + '.stdout.log')
    $stderr = Join-Path $testRoot ($Name + '.stderr.log')
    return Start-Process -FilePath $Executable -WindowStyle Hidden -PassThru `
        -RedirectStandardOutput $stdout -RedirectStandardError $stderr
}

function Assert-Stream([object]$Report, [string]$Mode) {
    Assert-Condition ($Report.mode -eq $Mode) "$Mode executable reported the wrong process mode."
    $fullRuns = @($Report.stream_full_runs)
    $cancelRuns = @($Report.stream_cancelled_runs)
    Assert-Condition ($fullRuns.Count -eq 5 -and $cancelRuns.Count -eq 5) "$Mode did not complete all five measurements."
    foreach ($run in $fullRuns) {
        Assert-Condition ([uint64]$run.bytes_read -eq 104857600 -and $run.cancelled -eq $false) "$Mode full-stream run did not consume exactly 100 MiB."
        Assert-Condition ($run.digest -eq $Report.stream_full.digest) "$Mode full-stream digest changed between runs."
    }
    foreach ($run in $cancelRuns) {
        Assert-Condition ([uint64]$run.bytes_read -eq 8388608 -and $run.cancelled -eq $true) "$Mode cancellation did not stop exactly at 8 MiB."
        Assert-Condition ($run.digest -eq $Report.stream_cancelled.digest) "$Mode cancelled-prefix digest changed between runs."
    }
    Assert-Condition ($Report.stream_full.digest.Length -eq 64) "$Mode did not return a BLAKE3 digest."
    Assert-Condition ($Report.stream_cancelled.digest.Length -eq 64) "$Mode did not digest the cancelled prefix."
}

function Get-Median([object[]]$Values) {
    $sorted = @($Values | ForEach-Object { [double]$_ } | Sort-Object)
    $middle = [int][Math]::Floor($sorted.Count / 2)
    if (($sorted.Count % 2) -eq 1) { return $sorted[$middle] }
    return ($sorted[$middle - 1] + $sorted[$middle]) / 2
}

function Invoke-InProcess([string]$Executable) {
    $database = Join-Path $testRoot 'in-process-database'
    $reportPath = Join-Path $testRoot 'in-process.json'
    $process = Start-HiddenApp $Executable 'in-process' $database $reportPath '30000' $true $true $true '' ''
    try {
        $report = Wait-ForResult $process $reportPath
        Assert-Stream $report 'in_process'
        Assert-Condition ($report.engine_panic.poison_detected -eq $true) 'In-process engine panic did not poison engine state.'
        Assert-Condition ($report.engine_panic.application_restart_required -eq $true) 'In-process panic did not require an application restart.'
        Assert-Condition ($report.engine_update.application_restart_required -eq $true) 'In-process update did not require an application restart.'
        if (-not $process.WaitForExit(30000)) { $process.Kill(); throw 'In-process application did not exit for its required restart.' }
        $process.WaitForExit()
        $process.Refresh()
        if ($null -ne $process.ExitCode -and $process.ExitCode -ne 0) {
            $panicLog = Get-Content -LiteralPath (Join-Path $testRoot 'in-process.stderr.log') -Raw -ErrorAction SilentlyContinue
            throw "In-process application exited with $($process.ExitCode) after the injected panic. $panicLog"
        }

        $reopenPath = Join-Path $testRoot 'in-process-reopen.json'
        $reopen = Start-HiddenApp $updatedInProcessCopy 'in-process-reopen' $database $reopenPath '500' $false $false $false '' ''
        try {
            $reopenReport = Wait-ForResult $reopen $reopenPath
            if (-not $reopen.WaitForExit(30000)) { $reopen.Kill(); throw 'In-process application did not complete its restart check.' }
            $reopen.WaitForExit()
            $reopen.Refresh()
            if ($null -ne $reopen.ExitCode -and $reopen.ExitCode -ne 0) { throw "In-process restart exited with code $($reopen.ExitCode)." }
            Assert-Condition ($reopenReport.engine_at_start.writer_owned -eq $true) 'In-process application restart did not reacquire the writer lock.'
            Assert-Condition ($reopenReport.application_process_id -ne $report.application_process_id) 'In-process restart reused the previous application process.'
            Assert-Condition ($reopenReport.application_build_id -ne $report.application_build_id) 'In-process update did not launch a different application build.'
        }
        finally {
            $reopen.Refresh()
            if (-not $reopen.HasExited) { $reopen.Kill(); $reopen.WaitForExit() }
        }
        return $report
    }
    finally {
        $process.Refresh()
        if (-not $process.HasExited) { $process.Kill(); $process.WaitForExit() }
    }
}

function Invoke-Sidecar([string]$Executable, [string]$InitialEngine, [string]$UpdatedEngine) {
    $database = Join-Path $testRoot 'sidecar-database'
    $reportPath = Join-Path $testRoot 'sidecar.json'
    $process = Start-HiddenApp $Executable 'sidecar' $database $reportPath '8000' $true $true $true $InitialEngine $UpdatedEngine
    try {
        $report = Wait-ForResult $process $reportPath
        Assert-Stream $report 'sidecar'
        Assert-Condition ($report.engine_panic.application_process_survived -eq $true) 'Desktop app did not survive the sidecar engine panic.'
        Assert-Condition ($report.engine_panic.writer_lock_reacquired -eq $true) 'Restarted sidecar did not reacquire the writer lock.'
        Assert-Condition ($report.engine_panic.previous_engine_process_id -ne $report.engine_panic.replacement_engine_process_id) 'Sidecar process did not restart after its panic.'
        Assert-Condition ($report.engine_update.previous_engine_build_id -ne $report.engine_update.updated_engine_build_id) 'Sidecar update did not switch to a different engine build.'
        Assert-Condition ($report.engine_update.writer_lock_reacquired -eq $true) 'Updated sidecar did not reacquire the writer lock.'
        Assert-Condition ($report.engine_update.application_process_id -eq $report.application_process_id) 'Sidecar update unexpectedly restarted the desktop application.'
        Assert-Condition ($report.engine_after_tests.writer_owned -eq $true) 'Sidecar did not retain the writer lock after panic and update.'
        if (-not $process.WaitForExit(30000)) {
            $diagnostic = Get-Content -LiteralPath (Join-Path $testRoot 'sidecar.stderr.log') -Raw -ErrorAction SilentlyContinue
            $process.Kill()
            throw "Sidecar desktop application did not shut down (pid $($process.Id)). $diagnostic"
        }
        $process.WaitForExit()
        $process.Refresh()
        if ($null -ne $process.ExitCode -and $process.ExitCode -ne 0) { throw "Sidecar desktop application exited with code $($process.ExitCode)." }
        foreach ($engineId in @($report.engine_panic.previous_engine_process_id, $report.engine_panic.replacement_engine_process_id, $report.engine_update.updated_engine_process_id)) {
            $live = Get-Process -Id ([int]$engineId) -ErrorAction SilentlyContinue
            Assert-Condition ($null -eq $live) "Sidecar process $engineId remained alive after application exit."
        }
        return $report
    }
    finally {
        $process.Refresh()
        if (-not $process.HasExited) { $process.Kill(); $process.WaitForExit() }
    }
}

try {
    foreach ($file in @($InProcessExecutable, $UpdatedInProcessExecutable, $SidecarExecutable, $EngineExecutable, $UpdatedEngineExecutable)) {
        if (-not (Test-Path -LiteralPath $file -PathType Leaf)) { throw "Required ODE-002 build artifact not found: $file" }
    }
    Copy-Item -LiteralPath $InProcessExecutable -Destination $inProcessCopy
    Copy-Item -LiteralPath $UpdatedInProcessExecutable -Destination $updatedInProcessCopy
    Copy-Item -LiteralPath $SidecarExecutable -Destination $sidecarCopy
    Copy-Item -LiteralPath $EngineExecutable -Destination $engineV1
    Copy-Item -LiteralPath $UpdatedEngineExecutable -Destination $engineV2

    $inProcessReport = Invoke-InProcess $inProcessCopy
    $sidecarReport = Invoke-Sidecar $sidecarCopy $engineV1 $engineV2
    Assert-Condition ($inProcessReport.stream_full.digest -eq $sidecarReport.stream_full.digest) 'In-process and sidecar 100 MiB digests differ.'
    Assert-Condition ($inProcessReport.stream_cancelled.digest -eq $sidecarReport.stream_cancelled.digest) 'In-process and sidecar cancelled-stream digests differ.'
    $inProcessFullTimes = @($inProcessReport.stream_full_runs | ForEach-Object { $_.elapsed_microseconds })
    $sidecarFullTimes = @($sidecarReport.stream_full_runs | ForEach-Object { $_.elapsed_microseconds })
    $inProcessCancelTimes = @($inProcessReport.stream_cancelled_runs | ForEach-Object { $_.elapsed_microseconds })
    $sidecarCancelTimes = @($sidecarReport.stream_cancelled_runs | ForEach-Object { $_.elapsed_microseconds })

    [pscustomobject]@{
        windows = 'PASS'
        in_process_full_100_mib = 'PASS'
        sidecar_full_100_mib = 'PASS'
        in_process_cancel_8_mib = 'PASS'
        sidecar_cancel_8_mib = 'PASS'
        identical_stream_digests = 'PASS'
        in_process_panic_requires_app_restart = 'PASS'
        sidecar_panic_isolated_and_restarted = 'PASS'
        in_process_update_requires_app_restart = 'PASS'
        in_process_update_swaps_app_build_and_restarts = 'PASS'
        sidecar_update_keeps_app_running = 'PASS'
        writer_lock_reacquired_after_all_restarts = 'PASS'
        sidecar_processes_reaped = 'PASS'
        measurement_runs_per_case = 5
        in_process_full_stream_median_microseconds = Get-Median $inProcessFullTimes
        sidecar_full_stream_median_microseconds = Get-Median $sidecarFullTimes
        in_process_cancel_median_microseconds = Get-Median $inProcessCancelTimes
        sidecar_cancel_median_microseconds = Get-Median $sidecarCancelTimes
    } | ConvertTo-Json -Compress
}
finally {
    foreach ($name in @('WORLDDB_ODE_DATABASE', 'WORLDDB_ODE_RESULT', 'WORLDDB_ODE_AUTOCLOSE_MS', 'WORLDDB_ODE_STREAM_TEST', 'WORLDDB_ODE_PANIC_TEST', 'WORLDDB_ODE_UPDATE_TEST', 'WORLDDB_ODE_ENGINE_EXECUTABLE', 'WORLDDB_ODE_ENGINE_UPDATE')) {
        Remove-Item "Env:\$name" -ErrorAction SilentlyContinue
    }
    $resolvedRoot = [System.IO.Path]::GetFullPath($testRoot)
    $tempPrefix = $tempBase.TrimEnd([System.IO.Path]::DirectorySeparatorChar) + [System.IO.Path]::DirectorySeparatorChar
    if (-not $resolvedRoot.StartsWith($tempPrefix, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw 'Refusing to remove a test directory outside the system temp directory.'
    }
    if (-not $KeepArtifacts -and (Test-Path -LiteralPath $resolvedRoot)) { Remove-Item -LiteralPath $resolvedRoot -Recurse -Force }
    if ($KeepArtifacts) { Write-Output "Matrix test artifacts: $resolvedRoot" }
}
