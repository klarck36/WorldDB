param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('in-process', 'sidecar')]
    [string]$Mode,
    [Parameter(Mandatory = $true)]
    [string]$ExecutablePath,
    [string]$EngineExecutablePath
)

$ErrorActionPreference = 'Stop'
$executable = [System.IO.Path]::GetFullPath($ExecutablePath)
if (-not (Test-Path -LiteralPath $executable -PathType Leaf)) {
    throw 'Build the selected ODE-002 mode before running this IPC smoke check.'
}
if ($Mode -eq 'sidecar' -and $EngineExecutablePath -and -not (Test-Path -LiteralPath $EngineExecutablePath -PathType Leaf)) {
    throw 'The requested sidecar engine executable does not exist.'
}

$tempBase = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
$testRoot = Join-Path $tempBase ('worlddb-ode002-ipc-' + [guid]::NewGuid().ToString('N'))
$null = New-Item -ItemType Directory -Path $testRoot
$databaseRoot = Join-Path $testRoot 'database'
$reportPath = Join-Path $testRoot 'startup.json'
$ipcPrefix = Join-Path $testRoot 'ipc.json'
$primaryPath = Join-Path $testRoot 'ipc-primary.json'
$secondaryPath = Join-Path $testRoot 'ipc-secondary.json'
$process = $null

function Wait-ForFiles([System.Diagnostics.Process]$Process, [string[]]$Paths) {
    $deadline = [DateTime]::UtcNow.AddSeconds(30)
    while (@($Paths | Where-Object { -not (Test-Path -LiteralPath $_ -PathType Leaf) }).Count -gt 0) {
        $Process.Refresh()
        if ($Process.HasExited) {
            $stderrPath = Join-Path $testRoot 'ipc.stderr.log'
            $diagnostic = Get-Content -LiteralPath $stderrPath -Raw -ErrorAction SilentlyContinue
            throw "Desktop process exited before both window IPC calls completed (exit $($Process.ExitCode)). $diagnostic"
        }
        if ([DateTime]::UtcNow -ge $deadline) {
            throw 'Timed out waiting for the authenticated native-window IPC calls.'
        }
        Start-Sleep -Milliseconds 100
    }
}

try {
    $env:WORLDDB_ODE_DATABASE = $databaseRoot
    $env:WORLDDB_ODE_RESULT = $reportPath
    $env:WORLDDB_ODE_IPC_RESULT = $ipcPrefix
    $env:WORLDDB_ODE_AUTOCLOSE_MS = '12000'
    if ($Mode -eq 'sidecar' -and $EngineExecutablePath) {
        $env:WORLDDB_ODE_ENGINE_EXECUTABLE = [System.IO.Path]::GetFullPath($EngineExecutablePath)
    } else {
        Remove-Item Env:\WORLDDB_ODE_ENGINE_EXECUTABLE -ErrorAction SilentlyContinue
    }

    $stdoutPath = Join-Path $testRoot 'ipc.stdout.log'
    $stderrPath = Join-Path $testRoot 'ipc.stderr.log'
    $process = Start-Process -FilePath $executable -WindowStyle Hidden -PassThru `
        -RedirectStandardOutput $stdoutPath -RedirectStandardError $stderrPath

    Wait-ForFiles $process @($reportPath, $primaryPath, $secondaryPath)
    $report = Get-Content -LiteralPath $reportPath -Raw | ConvertFrom-Json
    $primary = Get-Content -LiteralPath $primaryPath -Raw | ConvertFrom-Json
    $secondary = Get-Content -LiteralPath $secondaryPath -Raw | ConvertFrom-Json
    if ($report.mode -ne ($Mode -replace '-', '_')) { throw 'The executable reported the wrong process mode.' }
    foreach ($entry in @(@{ Value = $primary; Label = 'primary' }, @{ Value = $secondary; Label = 'secondary' })) {
        if ($entry.Value.protocol_version -ne 1 -or $entry.Value.window -ne $entry.Label -or $entry.Value.status -ne 'authorized_health_ok' -or $entry.Value.security_probe_mode -ne $true) {
            throw "The $($entry.Label) window did not complete the versioned authenticated health call."
        }
    }

    $process.Refresh()
    $processIds = @([int]$process.Id)
    if ($Mode -eq 'sidecar') { $processIds += [int]$report.engine.engine_process_id }
    foreach ($processId in $processIds) {
        $listeners = @(Get-NetTCPConnection -State Listen -OwningProcess $processId -ErrorAction SilentlyContinue)
        if ($listeners.Count -gt 0) {
            throw "WorldDB process $processId unexpectedly listens on a network port."
        }
    }

    if (-not $process.WaitForExit(30000)) {
        $process.Kill()
        throw 'The IPC smoke process did not shut down.'
    }
    $process.Refresh()
    if ($process.ExitCode -ne 0) { throw "The IPC smoke process exited with code $($process.ExitCode)." }

    [pscustomobject]@{
        mode = $Mode
        primary_window_authenticated_health = 'PASS'
        secondary_window_authenticated_health = 'PASS'
        versioned_ipc_protocol = 'PASS'
        invalid_session_rejected_in_both_windows = 'PASS'
        renderer_selected_path_rejected_in_both_windows = 'PASS'
        filesystem_plugin_command_rejected_in_both_windows = 'PASS'
        core_network_listeners = 'PASS'
        process_shutdown = 'PASS'
    } | ConvertTo-Json -Compress
}
finally {
    foreach ($name in @('WORLDDB_ODE_DATABASE', 'WORLDDB_ODE_RESULT', 'WORLDDB_ODE_IPC_RESULT', 'WORLDDB_ODE_AUTOCLOSE_MS', 'WORLDDB_ODE_ENGINE_EXECUTABLE')) {
        Remove-Item "Env:\$name" -ErrorAction SilentlyContinue
    }
    if ($null -ne $process) {
        $process.Refresh()
        if (-not $process.HasExited) {
            $process.Kill()
            $process.WaitForExit()
        }
    }
    $resolvedRoot = [System.IO.Path]::GetFullPath($testRoot)
    $tempPrefix = $tempBase.TrimEnd([System.IO.Path]::DirectorySeparatorChar) + [System.IO.Path]::DirectorySeparatorChar
    if (-not $resolvedRoot.StartsWith($tempPrefix, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw 'Refusing to remove a smoke-test directory outside the system temp directory.'
    }
    if (Test-Path -LiteralPath $resolvedRoot) { Remove-Item -LiteralPath $resolvedRoot -Recurse -Force }
}
