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
$primaryProjectPath = Join-Path $testRoot 'ipc-project-primary.json'
$secondaryProjectPath = Join-Path $testRoot 'ipc-project-secondary.json'
$primarySchemaPath = Join-Path $testRoot 'ipc-schema-primary.jsonl'
$secondarySchemaPath = Join-Path $testRoot 'ipc-schema-secondary.jsonl'
$primaryEntityPath = Join-Path $testRoot 'ipc-entity-primary.jsonl'
$secondaryEntityPath = Join-Path $testRoot 'ipc-entity-secondary.jsonl'
$primaryBranchLayerPath = Join-Path $testRoot 'ipc-branch-layer-primary.jsonl'
$secondaryBranchLayerPath = Join-Path $testRoot 'ipc-branch-layer-secondary.jsonl'
$primaryTransferPath = Join-Path $testRoot 'ipc-transfer-primary.jsonl'
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
            $missing = @($Paths | Where-Object { -not (Test-Path -LiteralPath $_ -PathType Leaf) })
            $stdoutPath = Join-Path $testRoot 'ipc.stdout.log'
            $stderrPath = Join-Path $testRoot 'ipc.stderr.log'
            $stdout = Get-Content -LiteralPath $stdoutPath -Raw -ErrorAction SilentlyContinue
            $stderr = Get-Content -LiteralPath $stderrPath -Raw -ErrorAction SilentlyContinue
            throw "Timed out waiting for the authenticated native-window IPC calls. Missing: $($missing -join ', '). stdout: $stdout stderr: $stderr"
        }
        Start-Sleep -Milliseconds 100
    }
}

function Wait-ForSchemaOperations([System.Diagnostics.Process]$Process, [string]$PrimaryPath, [string]$SecondaryPath) {
    $deadline = [DateTime]::UtcNow.AddSeconds(20)
    while ([DateTime]::UtcNow -lt $deadline) {
        if ((Test-Path -LiteralPath $PrimaryPath -PathType Leaf) -and (Test-Path -LiteralPath $SecondaryPath -PathType Leaf)) {
            $primary = @(Get-Content -LiteralPath $PrimaryPath | ForEach-Object { $_ | ConvertFrom-Json })
            $secondary = @(Get-Content -LiteralPath $SecondaryPath | ForEach-Object { $_ | ConvertFrom-Json })
            $batchCount = @($primary | Where-Object { $_.operation -eq 'set_lifecycle_batch' }).Count
            $currentReadCount = @($secondary | Where-Object { $_.operation -eq 'snapshot_current' }).Count
            if ($batchCount -ge 2 -and $currentReadCount -ge 1) { return }
        }
        $Process.Refresh()
        if ($Process.HasExited) { break }
        Start-Sleep -Milliseconds 100
    }
    $primaryEvents = if (Test-Path -LiteralPath $PrimaryPath -PathType Leaf) { Get-Content -LiteralPath $PrimaryPath -Raw } else { '<missing>' }
    $secondaryEvents = if (Test-Path -LiteralPath $SecondaryPath -PathType Leaf) { Get-Content -LiteralPath $SecondaryPath -Raw } else { '<missing>' }
    throw "Timed out waiting for complete schema IPC workflows. Primary: $primaryEvents Secondary: $secondaryEvents"
}

function Wait-ForEntityOperations([System.Diagnostics.Process]$Process, [string]$PrimaryPath, [string]$SecondaryPath) {
    $deadline = [DateTime]::UtcNow.AddSeconds(20)
    while ([DateTime]::UtcNow -lt $deadline) {
        if ((Test-Path -LiteralPath $PrimaryPath -PathType Leaf) -and (Test-Path -LiteralPath $SecondaryPath -PathType Leaf)) {
            $primary = @(Get-Content -LiteralPath $PrimaryPath | ForEach-Object { $_ | ConvertFrom-Json })
            $secondary = @(Get-Content -LiteralPath $SecondaryPath | ForEach-Object { $_ | ConvertFrom-Json })
            $creates = @($primary | Where-Object { $_.operation -eq 'create' }).Count
            $historicalReads = @($primary | Where-Object { $_.operation -eq 'snapshot_historical' }).Count
            $explicitReads = @($primary | Where-Object { $_.operation -eq 'snapshot_explicit' }).Count
            $retirements = @($primary | Where-Object { $_.operation -eq 'retire' }).Count
            $secondaryReads = @($secondary | Where-Object { $_.operation -eq 'snapshot_current' }).Count
            if ($creates -ge 2 -and $historicalReads -ge 2 -and $explicitReads -ge 2 -and $retirements -ge 1 -and $secondaryReads -ge 1) { return }
        }
        $Process.Refresh()
        if ($Process.HasExited) { break }
        Start-Sleep -Milliseconds 100
    }
    $primaryEvents = if (Test-Path -LiteralPath $PrimaryPath -PathType Leaf) { Get-Content -LiteralPath $PrimaryPath -Raw } else { '<missing>' }
    $secondaryEvents = if (Test-Path -LiteralPath $SecondaryPath -PathType Leaf) { Get-Content -LiteralPath $SecondaryPath -Raw } else { '<missing>' }
    throw "Timed out waiting for complete Entity IPC workflows. Primary: $primaryEvents Secondary: $secondaryEvents"
}

function Wait-ForBranchLayerOperations([System.Diagnostics.Process]$Process, [string]$PrimaryPath, [string]$SecondaryPath) {
    $deadline = [DateTime]::UtcNow.AddSeconds(20)
    while ([DateTime]::UtcNow -lt $deadline) {
        if ((Test-Path -LiteralPath $PrimaryPath -PathType Leaf) -and (Test-Path -LiteralPath $SecondaryPath -PathType Leaf)) {
            $primary = @(Get-Content -LiteralPath $PrimaryPath | ForEach-Object { $_ | ConvertFrom-Json })
            $secondary = @(Get-Content -LiteralPath $SecondaryPath | ForEach-Object { $_ | ConvertFrom-Json })
            $childCreates = @($primary | Where-Object { $_.operation -eq 'create_child' -and $_.succeeded -and $_.branch_created }).Count
            $layerCreates = @($primary | Where-Object { $_.operation -eq 'create_layer' -and $_.succeeded -and $_.layer_changed }).Count
            $layerRevisions = @($primary | Where-Object { $_.operation -eq 'revise_layer' -and $_.succeeded -and $_.layer_changed }).Count
            $explicitReads = @($primary | Where-Object { $_.operation -eq 'snapshot_explicit' -and $_.succeeded }).Count
            $rejectedStaleWrites = @($primary | Where-Object { $_.operation -eq 'create_child' -and -not $_.succeeded }).Count
            $secondaryReads = @($secondary | Where-Object { $_.operation -eq 'snapshot_current' -and $_.succeeded }).Count
            if ($childCreates -ge 1 -and $layerCreates -ge 1 -and $layerRevisions -ge 1 -and $explicitReads -ge 2 -and $rejectedStaleWrites -ge 1 -and $secondaryReads -ge 1) { return }
        }
        $Process.Refresh()
        if ($Process.HasExited) { break }
        Start-Sleep -Milliseconds 100
    }
    $primaryEvents = if (Test-Path -LiteralPath $PrimaryPath -PathType Leaf) { Get-Content -LiteralPath $PrimaryPath -Raw } else { '<missing>' }
    $secondaryEvents = if (Test-Path -LiteralPath $SecondaryPath -PathType Leaf) { Get-Content -LiteralPath $SecondaryPath -Raw } else { '<missing>' }
    throw "Timed out waiting for complete branch/layer IPC workflows. Primary: $primaryEvents Secondary: $secondaryEvents"
}

function Wait-ForTransferOperations([System.Diagnostics.Process]$Process, [string]$PrimaryPath) {
    $deadline = [DateTime]::UtcNow.AddSeconds(20)
    while ([DateTime]::UtcNow -lt $deadline) {
        if (Test-Path -LiteralPath $PrimaryPath -PathType Leaf) {
            $primary = @(Get-Content -LiteralPath $PrimaryPath | ForEach-Object { $_ | ConvertFrom-Json })
            if (@($primary | Where-Object { $_.operation -eq 'list' -and $_.succeeded }).Count -ge 1) { return }
        }
        $Process.Refresh()
        if ($Process.HasExited) { break }
        Start-Sleep -Milliseconds 100
    }
    $primaryEvents = if (Test-Path -LiteralPath $PrimaryPath -PathType Leaf) { Get-Content -LiteralPath $PrimaryPath -Raw } else { '<missing>' }
    throw "Timed out waiting for the authenticated HistorySpace transfer catalog call. Primary: $primaryEvents"
}

try {
    $env:WORLDDB_ODE_RESULT = $reportPath
    $env:WORLDDB_ODE_IPC_RESULT = $ipcPrefix
    $env:WORLDDB_ODE_SCHEMA_SMOKE_RESULT = $ipcPrefix
    $env:WORLDDB_ODE_ENTITY_SMOKE_RESULT = $ipcPrefix
    $env:WORLDDB_ODE_BRANCH_LAYER_SMOKE_RESULT = $ipcPrefix
    $env:WORLDDB_ODE_TRANSFER_SMOKE_RESULT = $ipcPrefix
    $env:WORLDDB_ODE_PROJECT_SMOKE_ROOT = $databaseRoot
    $env:WORLDDB_ODE_AUTOCLOSE_MS = '30000'
    $env:WORLDDB_ODE_ENGINE_PRINCIPAL_ID = '00000000-0000-7000-8000-000000000099'
    if ($Mode -eq 'sidecar' -and $EngineExecutablePath) {
        $env:WORLDDB_ODE_ENGINE_EXECUTABLE = [System.IO.Path]::GetFullPath($EngineExecutablePath)
    } else {
        Remove-Item Env:\WORLDDB_ODE_ENGINE_EXECUTABLE -ErrorAction SilentlyContinue
    }

    $stdoutPath = Join-Path $testRoot 'ipc.stdout.log'
    $stderrPath = Join-Path $testRoot 'ipc.stderr.log'
    $process = Start-Process -FilePath $executable -WindowStyle Hidden -PassThru `
        -RedirectStandardOutput $stdoutPath -RedirectStandardError $stderrPath

    Wait-ForFiles $process @($reportPath, $primaryPath, $secondaryPath, $primaryProjectPath, $secondaryProjectPath, $primarySchemaPath, $secondarySchemaPath)
    Wait-ForSchemaOperations $process $primarySchemaPath $secondarySchemaPath
    Wait-ForEntityOperations $process $primaryEntityPath $secondaryEntityPath
    Wait-ForBranchLayerOperations $process $primaryBranchLayerPath $secondaryBranchLayerPath
    Wait-ForTransferOperations $process $primaryTransferPath
    $report = Get-Content -LiteralPath $reportPath -Raw | ConvertFrom-Json
    $primary = Get-Content -LiteralPath $primaryPath -Raw | ConvertFrom-Json
    $secondary = Get-Content -LiteralPath $secondaryPath -Raw | ConvertFrom-Json
    $primaryProject = Get-Content -LiteralPath $primaryProjectPath -Raw | ConvertFrom-Json
    $secondaryProject = Get-Content -LiteralPath $secondaryProjectPath -Raw | ConvertFrom-Json
    $primarySchema = @(Get-Content -LiteralPath $primarySchemaPath | ForEach-Object { $_ | ConvertFrom-Json })
    $secondarySchema = @(Get-Content -LiteralPath $secondarySchemaPath | ForEach-Object { $_ | ConvertFrom-Json })
    $primaryEntity = @(Get-Content -LiteralPath $primaryEntityPath | ForEach-Object { $_ | ConvertFrom-Json })
    $secondaryEntity = @(Get-Content -LiteralPath $secondaryEntityPath | ForEach-Object { $_ | ConvertFrom-Json })
    $primaryBranchLayers = @(Get-Content -LiteralPath $primaryBranchLayerPath | ForEach-Object { $_ | ConvertFrom-Json })
    $secondaryBranchLayers = @(Get-Content -LiteralPath $secondaryBranchLayerPath | ForEach-Object { $_ | ConvertFrom-Json })
    $primaryTransfer = @(Get-Content -LiteralPath $primaryTransferPath | ForEach-Object { $_ | ConvertFrom-Json })
    if ($report.mode -ne ($Mode -replace '-', '_')) { throw 'The executable reported the wrong process mode.' }
    foreach ($entry in @(@{ Value = $primary; Label = 'primary' }, @{ Value = $secondary; Label = 'secondary' })) {
        if ($entry.Value.protocol_version -ne 1 -or $entry.Value.window -ne $entry.Label -or $entry.Value.status -ne 'authorized_health_ok' -or $entry.Value.security_probe_mode -ne $true) {
            throw "The $($entry.Label) window did not complete the versioned authenticated health call."
        }
    }
    foreach ($entry in @(@{ Value = $primaryProject; Label = 'primary' }, @{ Value = $secondaryProject; Label = 'secondary' })) {
        if ($entry.Value.protocol_version -ne 1 -or $entry.Value.window -ne $entry.Label -or -not $entry.Value.project_open -or $entry.Value.revision -lt 1 -or $entry.Value.role -ne 'gm' -or [string]::IsNullOrWhiteSpace($entry.Value.snapshot_id) -or $null -eq $entry.Value.engine.engine_process_id) {
            throw "The $($entry.Label) window did not complete the authenticated project open/create flow."
        }
    }
    if ($primaryProject.database_id -ne $secondaryProject.database_id) { throw 'Both windows did not resolve the same WorldDB project.' }
    if ($primaryProject.snapshot_id -eq $secondaryProject.snapshot_id) { throw 'The native windows received the same project snapshot identity.' }
    if (@($primarySchema | Where-Object { -not $_.succeeded }).Count -gt 0) { throw 'The primary window had a rejected schema IPC operation.' }
    if (@($secondarySchema | Where-Object { -not $_.succeeded }).Count -gt 0) { throw 'The secondary window had a rejected schema read.' }
    $requiredSchemaOperations = @('snapshot_current', 'create', 'snapshot_historical', 'snapshot_explicit')
    foreach ($operation in $requiredSchemaOperations) {
        if (@($primarySchema | Where-Object { $_.operation -eq $operation }).Count -eq 0) {
            throw "The primary window did not complete schema IPC operation '$operation'."
        }
    }
    if (@($primarySchema | Where-Object { $_.operation -eq 'set_lifecycle_batch' }).Count -lt 2) {
        $schemaOperations = $primarySchema | ConvertTo-Json -Compress -Depth 5
        throw "The primary window did not complete both schema lifecycle transitions. Recorded: $schemaOperations"
    }
    if (@($secondarySchema | Where-Object { $_.operation -eq 'snapshot_current' }).Count -eq 0) {
        throw 'The secondary window did not read the shared current schema.'
    }
    if (@($primaryEntity | Where-Object { -not $_.succeeded }).Count -gt 0) { throw 'The primary window had a rejected Entity IPC operation.' }
    if (@($secondaryEntity | Where-Object { -not $_.succeeded }).Count -gt 0) { throw 'The secondary window had a rejected Entity read.' }
    $requiredEntityOperations = @('snapshot_current', 'create', 'snapshot_historical', 'snapshot_explicit', 'retire')
    foreach ($operation in $requiredEntityOperations) {
        if (@($primaryEntity | Where-Object { $_.operation -eq $operation }).Count -eq 0) {
            throw "The primary window did not complete Entity IPC operation '$operation'."
        }
    }
    if (@($primaryEntity | Where-Object { $_.operation -eq 'create' }).Count -lt 2) {
        throw 'The primary window did not create both the Active and Deprecated EntityType entities.'
    }
    if (@($primaryEntity | Where-Object { $_.warning.code -eq 'deprecated_entity_type' }).Count -ne 1) {
        throw 'The Deprecated EntityType creation did not return exactly one typed warning.'
    }
    if (@($secondaryEntity | Where-Object { $_.operation -eq 'snapshot_current' }).Count -eq 0) {
        throw 'The secondary window did not read the shared current Entity catalog.'
    }
    if (@($primaryBranchLayers | Where-Object { -not $_.succeeded -and $_.operation -ne 'create_child' }).Count -gt 0) {
        throw 'The primary window had an unexpected rejected branch/layer IPC operation.'
    }
    if (@($primaryBranchLayers | Where-Object { $_.operation -eq 'create_child' -and $_.succeeded -and $_.branch_created }).Count -lt 1) {
        throw 'The primary window did not create a child branch.'
    }
    if (@($primaryBranchLayers | Where-Object { $_.operation -eq 'create_layer' -and $_.succeeded -and $_.layer_changed }).Count -lt 1) {
        throw 'The primary window did not create an overlay Layer.'
    }
    if (@($primaryBranchLayers | Where-Object { $_.operation -eq 'revise_layer' -and $_.succeeded -and $_.layer_changed }).Count -lt 1) {
        throw 'The primary window did not switch the base Layer.'
    }
    if (@($primaryBranchLayers | Where-Object { $_.operation -eq 'snapshot_explicit' -and $_.succeeded }).Count -lt 2) {
        throw 'The primary window did not read both historical branch/layer snapshots.'
    }
    if (@($primaryBranchLayers | Where-Object { $_.operation -eq 'create_child' -and -not $_.succeeded }).Count -lt 1) {
        throw 'The stale child-creation conflict was not rejected.'
    }
    if (@($secondaryBranchLayers | Where-Object { $_.operation -eq 'snapshot_current' -and $_.succeeded }).Count -eq 0) {
        throw 'The secondary window did not read the shared branch/layer catalog.'
    }
    $transferCatalogReads = @($primaryTransfer | Where-Object {
        $_.operation -eq 'list' -and $_.succeeded -and $null -ne $_.record_count -and $null -ne $_.relation_count
    })
    if ($transferCatalogReads.Count -eq 0) {
        throw 'The primary window did not read a typed HistorySpace transfer catalog.'
    }

    $process.Refresh()
    $processIds = @([int]$process.Id)
    $processIds += [int]$primaryProject.engine.engine_process_id
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
        authenticated_project_bootstrap = 'PASS'
        transactional_schema_create_and_lifecycle = 'PASS'
        current_historical_and_explicit_schema_reads = 'PASS'
        secondary_window_schema_read = 'PASS'
        transactional_entity_create_and_retirement = 'PASS'
        current_historical_and_explicit_entity_reads = 'PASS'
        deprecated_entity_type_opt_in_warning = 'PASS'
        secondary_window_entity_read = 'PASS'
        transactional_branch_layer_creation_and_base_switch = 'PASS'
        current_and_historical_branch_layer_reads = 'PASS'
        stale_branch_write_rejected_without_publication = 'PASS'
        secondary_window_branch_layer_read = 'PASS'
        authenticated_history_space_transfer_catalog = 'PASS'
        shared_project_with_distinct_window_snapshots = 'PASS'
        versioned_ipc_protocol = 'PASS'
        invalid_session_rejected_in_both_windows = 'PASS'
        renderer_selected_path_rejected_in_both_windows = 'PASS'
        filesystem_plugin_command_rejected_in_both_windows = 'PASS'
        host_principal_not_selected_by_environment = 'PASS'
        core_network_listeners = 'PASS'
        process_shutdown = 'PASS'
    } | ConvertTo-Json -Compress
}
finally {
    foreach ($name in @('WORLDDB_ODE_DATABASE', 'WORLDDB_ODE_RESULT', 'WORLDDB_ODE_IPC_RESULT', 'WORLDDB_ODE_SCHEMA_SMOKE_RESULT', 'WORLDDB_ODE_ENTITY_SMOKE_RESULT', 'WORLDDB_ODE_BRANCH_LAYER_SMOKE_RESULT', 'WORLDDB_ODE_TRANSFER_SMOKE_RESULT', 'WORLDDB_ODE_PROJECT_SMOKE_ROOT', 'WORLDDB_ODE_AUTOCLOSE_MS', 'WORLDDB_ODE_ENGINE_EXECUTABLE', 'WORLDDB_ODE_ENGINE_PRINCIPAL_ID')) {
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
