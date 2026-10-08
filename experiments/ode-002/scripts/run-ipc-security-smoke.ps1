param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('in-process', 'sidecar')]
    [string]$Mode,
    [Parameter(Mandatory = $true)]
    [string]$ExecutablePath,
    [string]$EngineExecutablePath,
    [switch]$KeepArtifacts,
    [string]$ArtifactsRoot,
    [ValidateRange(30, 600)]
    [int]$FactsTimeoutSeconds = 600
)

$ErrorActionPreference = 'Stop'
$isWindowsPlatform = [System.Environment]::OSVersion.Platform -eq [System.PlatformID]::Win32NT
if ($ArtifactsRoot -and -not $KeepArtifacts) {
    throw 'ArtifactsRoot requires KeepArtifacts.'
}
if ($isWindowsPlatform -and -not ('WorldDbIpcSmokeNative' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class WorldDbIpcSmokeNative {
    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool GetExitCodeProcess(IntPtr processHandle, out uint exitCode);
}
'@
}
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
$primaryPerspectivePath = Join-Path $testRoot 'ipc-perspective-primary.jsonl'
$secondaryPerspectivePath = Join-Path $testRoot 'ipc-perspective-secondary.jsonl'
$primarySecurityPolicyPath = Join-Path $testRoot 'ipc-security-policy-primary.jsonl'
$secondarySecurityPolicyPath = Join-Path $testRoot 'ipc-security-policy-secondary.jsonl'
$primaryFactsPath = Join-Path $testRoot 'ipc-facts-primary.jsonl'
$process = $null
$processHandle = [IntPtr]::Zero
$smokePassed = $false

function Get-ListeningTcpConnections([int]$ProcessId) {
    if ($isWindowsPlatform) {
        return @(Get-NetTCPConnection -State Listen -OwningProcess $ProcessId -ErrorAction SilentlyContinue)
    }
    $lsofCommand = Get-Command lsof -ErrorAction SilentlyContinue
    if ($null -eq $lsofCommand) {
        throw 'lsof is required to verify that native smoke processes do not listen on TCP ports.'
    }
    $rows = @(& $lsofCommand.Source -n -P -a -p $ProcessId -iTCP -sTCP:LISTEN 2>$null)
    if ($LASTEXITCODE -eq 1) { return @() }
    if ($LASTEXITCODE -ne 0) { throw "Could not inspect TCP listeners for process $ProcessId (lsof exit $LASTEXITCODE)." }
    return @($rows | Select-Object -Skip 1 | Where-Object { $_.Trim() })
}

function Stop-SmokeSidecarChild {
    if ($Mode -ne 'sidecar' -or [string]::IsNullOrWhiteSpace($EngineExecutablePath)) { return }
    if (-not (Test-Path -LiteralPath $primaryProjectPath -PathType Leaf)) { return }
    try {
        $project = Get-Content -LiteralPath $primaryProjectPath -Raw | ConvertFrom-Json
        $engineProcessId = [int]$project.engine.engine_process_id
    } catch {
        return
    }
    if ($engineProcessId -le 0 -or ($null -ne $process -and $engineProcessId -eq $process.Id)) { return }
    try {
        $engine = [System.Diagnostics.Process]::GetProcessById($engineProcessId)
        if ($engine.HasExited) { return }
        $actualPath = $engine.MainModule.FileName
        $expectedPath = [System.IO.Path]::GetFullPath($EngineExecutablePath)
        if (-not [string]::Equals([System.IO.Path]::GetFullPath($actualPath), $expectedPath, [System.StringComparison]::OrdinalIgnoreCase)) {
            Write-Warning "Refusing to stop process $engineProcessId because it is not the configured smoke-test sidecar."
            return
        }
        $engine.Kill()
        if (-not $engine.WaitForExit(5000)) {
            Write-Warning "The smoke-test sidecar process $engineProcessId did not stop within five seconds."
        }
    } catch [System.ArgumentException] {
        # The sidecar already exited and released its database lock.
    } catch {
        Write-Warning "Could not confirm sidecar cleanup for process ${engineProcessId}: $_"
    }
}

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

function Wait-ForSchemaOperations([System.Diagnostics.Process]$Process, [string]$PrimaryPath, [string]$SecondaryPath, [string]$FactsPath) {
    $deadline = [DateTime]::UtcNow.AddSeconds(60)
    while ([DateTime]::UtcNow -lt $deadline) {
        $factsEvents = @(Get-Content -LiteralPath $FactsPath -ErrorAction SilentlyContinue | ForEach-Object { $_ | ConvertFrom-Json })
        $smokeError = @($factsEvents | Where-Object {
            $_.operation -eq 'diagnostic' -and $_.details -like 'schema-smoke:error:*'
        } | Select-Object -Last 1)
        $schemaSmokeComplete = @($factsEvents | Where-Object {
            $_.operation -eq 'diagnostic' -and $_.details -eq 'facts-smoke:schema-smoke:after-final-entity-type-deprecation'
        }).Count -ge 1
        if ($smokeError.Count -gt 0) {
            throw "Schema IPC smoke failed in the renderer: $($smokeError[0].details)"
        }
        if ((Test-Path -LiteralPath $PrimaryPath -PathType Leaf) -and (Test-Path -LiteralPath $SecondaryPath -PathType Leaf)) {
            $primary = @(Get-Content -LiteralPath $PrimaryPath | ForEach-Object { $_ | ConvertFrom-Json })
            $secondary = @(Get-Content -LiteralPath $SecondaryPath | ForEach-Object { $_ | ConvertFrom-Json })
            $creates = @($primary | Where-Object { $_.operation -eq 'create' -and $_.succeeded }).Count
            $batchCount = @($primary | Where-Object { $_.operation -eq 'set_lifecycle_batch' }).Count
            $currentReadCount = @($secondary | Where-Object { $_.operation -eq 'snapshot_current' -and $_.succeeded }).Count
            $timelineStates = @($primary | ForEach-Object { $_.definitions } | Where-Object { $_.family -eq 'timeline' -and $_.symbol -eq 'ipc_smoke_timeline' } | Select-Object -ExpandProperty lifecycle -Unique)
            $timeUnitStates = @($primary | ForEach-Object { $_.definitions } | Where-Object { $_.family -eq 'time_unit' -and $_.symbol -eq 'ipc_smoke_max_scale' } | Select-Object -ExpandProperty lifecycle -Unique)
            $timelineComplete = @('active', 'deprecated', 'retired' | Where-Object { $timelineStates -contains $_ }).Count -eq 3
            $timeUnitComplete = @('active', 'deprecated', 'retired' | Where-Object { $timeUnitStates -contains $_ }).Count -eq 3
            if ($creates -ge 3 -and $batchCount -ge 7 -and $timelineComplete -and $timeUnitComplete -and $currentReadCount -ge 1 -and $schemaSmokeComplete) { return }
        }
        $Process.Refresh()
        if ($Process.HasExited) { break }
        Start-Sleep -Milliseconds 100
    }
    $primaryEvents = if (Test-Path -LiteralPath $PrimaryPath -PathType Leaf) { Get-Content -LiteralPath $PrimaryPath -Raw } else { '<missing>' }
    $secondaryEvents = if (Test-Path -LiteralPath $SecondaryPath -PathType Leaf) { Get-Content -LiteralPath $SecondaryPath -Raw } else { '<missing>' }
    throw "Timed out waiting for complete schema IPC workflows. Primary: $primaryEvents Secondary: $secondaryEvents"
}

function Wait-ForEntityOperations([System.Diagnostics.Process]$Process, [string]$PrimaryPath, [string]$SecondaryPath, [string]$FactsPath) {
    $deadline = [DateTime]::UtcNow.AddSeconds(60)
    while ([DateTime]::UtcNow -lt $deadline) {
        if ((Test-Path -LiteralPath $PrimaryPath -PathType Leaf) -and (Test-Path -LiteralPath $SecondaryPath -PathType Leaf)) {
            $primary = @(Get-Content -LiteralPath $PrimaryPath | ForEach-Object { $_ | ConvertFrom-Json })
            $secondary = @(Get-Content -LiteralPath $SecondaryPath | ForEach-Object { $_ | ConvertFrom-Json })
            $factsEvents = @(Get-Content -LiteralPath $FactsPath -ErrorAction SilentlyContinue | ForEach-Object { $_ | ConvertFrom-Json })
            $smokeError = @($factsEvents | Where-Object {
                $_.operation -eq 'diagnostic' -and $_.details -like 'entity-smoke:error:*'
            } | Select-Object -Last 1)
            $entitySmokeComplete = @($factsEvents | Where-Object {
                $_.operation -eq 'diagnostic' -and $_.details -eq 'facts-smoke:entity-smoke:complete'
            }).Count -ge 1
            if ($smokeError.Count -gt 0) {
                throw "Entity IPC smoke failed in the renderer: $($smokeError[0].details)"
            }
            $creates = @($primary | Where-Object { $_.operation -eq 'create' }).Count
            $historicalReads = @($primary | Where-Object { $_.operation -eq 'snapshot_historical' }).Count
            $explicitReads = @($primary | Where-Object { $_.operation -eq 'snapshot_explicit' }).Count
            $retirements = @($primary | Where-Object { $_.operation -eq 'retire' }).Count
            $secondaryReads = @($secondary | Where-Object { $_.operation -eq 'snapshot_current' }).Count
            if ($creates -ge 2 -and $historicalReads -ge 2 -and $explicitReads -ge 1 -and $retirements -ge 1 -and $secondaryReads -ge 1 -and $entitySmokeComplete) { return }
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
    $deadline = [DateTime]::UtcNow.AddSeconds(60)
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

function Wait-ForTransferOperations([System.Diagnostics.Process]$Process, [string]$PrimaryPath, [string]$FactsPath) {
    $deadline = [DateTime]::UtcNow.AddSeconds(60)
    while ([DateTime]::UtcNow -lt $deadline) {
        $factsEvents = @(Get-Content -LiteralPath $FactsPath -ErrorAction SilentlyContinue | ForEach-Object { $_ | ConvertFrom-Json })
        $smokeError = @($factsEvents | Where-Object {
            $_.operation -eq 'diagnostic' -and $_.details -like 'branch-layer-smoke:error:*'
        } | Select-Object -Last 1)
        $branchLayerSmokeComplete = @($factsEvents | Where-Object {
            $_.operation -eq 'diagnostic' -and $_.details -eq 'facts-smoke:branch-layer-smoke:complete'
        }).Count -ge 1
        if ($smokeError.Count -gt 0) {
            throw "Branch/layer IPC smoke failed in the renderer: $($smokeError[0].details)"
        }
        if (Test-Path -LiteralPath $PrimaryPath -PathType Leaf) {
            $primary = @(Get-Content -LiteralPath $PrimaryPath | ForEach-Object { $_ | ConvertFrom-Json })
            if (@($primary | Where-Object { $_.operation -eq 'list' -and $_.succeeded }).Count -ge 1 -and $branchLayerSmokeComplete) { return }
        }
        $Process.Refresh()
        if ($Process.HasExited) { break }
        Start-Sleep -Milliseconds 100
    }
    $primaryEvents = if (Test-Path -LiteralPath $PrimaryPath -PathType Leaf) { Get-Content -LiteralPath $PrimaryPath -Raw } else { '<missing>' }
    $lastStages = @($factsEvents | Where-Object { $_.operation -eq 'diagnostic' } | Select-Object -Last 20 | ForEach-Object { $_.details }) -join ' | '
    throw "Timed out waiting for the complete branch/layer and HistorySpace transfer workflow. Primary: $primaryEvents Stages: $lastStages"
}

function Wait-ForPerspectiveOperations([System.Diagnostics.Process]$Process, [string]$PrimaryPath, [string]$SecondaryPath, [string]$FactsPath) {
    $deadline = [DateTime]::UtcNow.AddSeconds(60)
    while ([DateTime]::UtcNow -lt $deadline) {
        $factsEvents = @(Get-Content -LiteralPath $FactsPath -ErrorAction SilentlyContinue | ForEach-Object { $_ | ConvertFrom-Json })
        $smokeError = @($factsEvents | Where-Object {
            $_.operation -eq 'diagnostic' -and $_.details -like 'perspective-smoke:error:*'
        } | Select-Object -Last 1)
        $perspectiveSmokeComplete = @($factsEvents | Where-Object {
            $_.operation -eq 'diagnostic' -and $_.details -eq 'facts-smoke:perspective-smoke:complete'
        }).Count -ge 1
        if ($smokeError.Count -gt 0) {
            throw "Perspective IPC smoke failed in the renderer: $($smokeError[0].details)"
        }
        if ((Test-Path -LiteralPath $PrimaryPath -PathType Leaf) -and (Test-Path -LiteralPath $SecondaryPath -PathType Leaf)) {
            $primary = @(Get-Content -LiteralPath $PrimaryPath | ForEach-Object { $_ | ConvertFrom-Json })
            $secondary = @(Get-Content -LiteralPath $SecondaryPath | ForEach-Object { $_ | ConvertFrom-Json })
            $creates = @($primary | Where-Object { $_.operation -eq 'create' -and $_.succeeded }).Count
            $updates = @($primary | Where-Object { $_.operation -eq 'update' -and $_.succeeded }).Count
            $retirements = @($primary | Where-Object { $_.operation -eq 'retire' -and $_.succeeded }).Count
            $validContexts = @($primary | Where-Object { $_.operation -eq 'validate_context' -and $_.succeeded }).Count
            $rejectedContexts = @($primary | Where-Object { $_.operation -eq 'validate_context' -and -not $_.succeeded }).Count
            $secondaryReads = @($secondary | Where-Object { $_.operation -eq 'snapshot_current' -and $_.succeeded }).Count
            if ($creates -ge 1 -and $updates -ge 1 -and $retirements -ge 1 -and $validContexts -ge 2 -and $rejectedContexts -ge 3 -and $secondaryReads -ge 1 -and $perspectiveSmokeComplete) { return }
        }
        $Process.Refresh()
        if ($Process.HasExited) { break }
        Start-Sleep -Milliseconds 100
    }
    $primaryEvents = if (Test-Path -LiteralPath $PrimaryPath -PathType Leaf) { Get-Content -LiteralPath $PrimaryPath -Raw } else { '<missing>' }
    $secondaryEvents = if (Test-Path -LiteralPath $SecondaryPath -PathType Leaf) { Get-Content -LiteralPath $SecondaryPath -Raw } else { '<missing>' }
    throw "Timed out waiting for complete Perspective IPC workflows. Primary: $primaryEvents Secondary: $secondaryEvents"
}

function Wait-ForSecurityPolicyOperations([System.Diagnostics.Process]$Process, [string]$PrimaryPath) {
    $deadline = [DateTime]::UtcNow.AddSeconds(20)
    while ([DateTime]::UtcNow -lt $deadline) {
        if (Test-Path -LiteralPath $PrimaryPath -PathType Leaf) {
            $primary = @(Get-Content -LiteralPath $PrimaryPath | ForEach-Object { $_ | ConvertFrom-Json })
            $assignments = @($primary | Where-Object { $_.operation -eq 'assign_role' -and $_.succeeded }).Count
            $revocations = @($primary | Where-Object { $_.operation -eq 'revoke_role_assignment' -and $_.succeeded }).Count
            $rules = @($primary | Where-Object { $_.operation -eq 'add_capability_rule' -and $_.succeeded }).Count
            $ruleRevocations = @($primary | Where-Object { $_.operation -eq 'revoke_capability_rule' -and $_.succeeded }).Count
            if ($assignments -ge 1 -and $revocations -ge 1 -and $rules -ge 1 -and $ruleRevocations -ge 1) { return }
        }
        $Process.Refresh()
        if ($Process.HasExited) { break }
        Start-Sleep -Milliseconds 100
    }
    $events = if (Test-Path -LiteralPath $PrimaryPath -PathType Leaf) { Get-Content -LiteralPath $PrimaryPath -Raw } else { '<missing>' }
    throw "Timed out waiting for policy IPC workflows. Primary: $events"
}

function Wait-ForFactsOperations([System.Diagnostics.Process]$Process, [string]$PrimaryPath) {
    $deadline = [DateTime]::UtcNow.AddSeconds($FactsTimeoutSeconds)
    while ([DateTime]::UtcNow -lt $deadline) {
        if (Test-Path -LiteralPath $PrimaryPath -PathType Leaf) {
            $operations = @(Get-Content -LiteralPath $PrimaryPath | ForEach-Object { $_ | ConvertFrom-Json })
            $smokeError = @($operations | Where-Object {
                $_.operation -eq 'diagnostic' -and $_.details -like 'facts-smoke:error:*'
            } | Select-Object -Last 1)
            if ($smokeError.Count -gt 0) {
                throw "Facts IPC smoke failed in the renderer: $($smokeError[0].details)"
            }
            $rejected = @($operations | Where-Object { -not $_.succeeded })
            $failedAssertions = @($rejected | Where-Object { $_.operation -eq 'create_assertion' })
            $nonAssertionRejections = @($rejected | Where-Object { $_.operation -ne 'create_assertion' })
            $expectedCommitConflict = @($operations | Where-Object {
                $_.operation -eq 'diagnostic' -and $_.details -eq 'facts-smoke:commit-conflict:confirmed'
            }).Count -gt 0
            $unknownCommitResolved = @($operations | Where-Object {
                $_.operation -eq 'diagnostic' -and $_.details -eq 'facts-smoke:unknown-commit:resolved'
            }).Count -gt 0
            if ($nonAssertionRejections.Count -gt 0 -or $failedAssertions.Count -gt 1) {
                throw "An unexpected factual-record or resolution-preview IPC call was rejected: $($rejected | ConvertTo-Json -Compress -Depth 5)"
            }
            $assertions = @($operations | Where-Object { $_.operation -eq 'create_assertion' -and $_.succeeded }).Count
            $masks = @($operations | Where-Object { $_.operation -eq 'create_mask' -and $_.succeeded })
            $boundaries = @($operations | Where-Object { $_.operation -eq 'create_replacement_boundary' -and $_.succeeded }).Count
            $corrections = @($operations | Where-Object { $_.operation -eq 'correct_assertion' -and $_.succeeded }).Count
            $events = @($operations | Where-Object { $_.operation -eq 'create_event' -and $_.succeeded }).Count
            $eventMasks = @($operations | Where-Object { $_.operation -eq 'create_event_mask' -and $_.succeeded }).Count
            $eventRelations = @($operations | Where-Object { $_.operation -eq 'create_event_relation' -and $_.succeeded }).Count
            $sources = @($operations | Where-Object { $_.operation -eq 'create_source' -and $_.succeeded }).Count
            $sourceSupersessions = @($operations | Where-Object { $_.operation -eq 'supersede_source' -and $_.succeeded }).Count
            $evidence = @($operations | Where-Object { $_.operation -eq 'create_evidence' -and $_.succeeded }).Count
            $provenance = @($operations | Where-Object { $_.operation -eq 'create_provenance' -and $_.succeeded }).Count
            $evidenceRetractions = @($operations | Where-Object { $_.operation -eq 'retract_evidence' -and $_.succeeded }).Count
            $provenanceRetractions = @($operations | Where-Object { $_.operation -eq 'retract_provenance' -and $_.succeeded }).Count
            $metaHistoryComplete = @($operations | Where-Object {
                $_.operation -eq 'diagnostic' -and $_.details -eq 'facts-smoke:meta-history:complete'
            }).Count -gt 0
            $projectSmokeComplete = @($operations | Where-Object {
                $_.operation -eq 'diagnostic' -and $_.details -eq 'facts-smoke:project-complete'
            }).Count -gt 0
            $recoverySmokeComplete = @($operations | Where-Object {
                $_.operation -eq 'diagnostic' -and $_.details -eq 'facts-smoke:recovery-smoke:complete'
            }).Count -gt 0
            $backupRendererPathsRejected = @($operations | Where-Object {
                $_.operation -eq 'diagnostic' -and $_.details -eq 'facts-smoke:backup-renderer-paths:rejected'
            }).Count -gt 0
            $exportImportRendererPathsRejected = @($operations | Where-Object {
                $_.operation -eq 'diagnostic' -and $_.details -eq 'facts-smoke:export-import-renderer-paths:rejected'
            }).Count -gt 0
            $purgeRendererPathsRejected = @($operations | Where-Object {
                $_.operation -eq 'diagnostic' -and $_.details -eq 'facts-smoke:purge-renderer-paths:rejected'
            }).Count -gt 0
            $diagnosticCanaryRejected = @($operations | Where-Object {
                $_.operation -eq 'diagnostic' -and $_.details -eq 'facts-smoke:diagnostic-canary:rejected'
            }).Count -gt 0
            $diagnosticRendererPathsRejected = @($operations | Where-Object {
                $_.operation -eq 'diagnostic' -and $_.details -eq 'facts-smoke:diagnostic-renderer-paths:rejected'
            }).Count -gt 0
            $spanClosures = @($operations | Where-Object { $_.operation -eq 'close_event_span' -and $_.succeeded }).Count
            $catalogs = @($operations | Where-Object { $_.operation -eq 'snapshot' -and $_.succeeded }).Count
            $lifecycle = @($operations | Where-Object { $_.operation -eq 'lifecycle' -and $_.succeeded })
            $lifecycleEffects = @($lifecycle | Select-Object -ExpandProperty result_kind -Unique)
            $eventRetractions = @($lifecycle | Where-Object { $_.family -eq 'event' -and $_.result_kind -eq 'retracted' }).Count
            $eventMaskRetractions = @($lifecycle | Where-Object { $_.family -eq 'event_mask' -and $_.result_kind -eq 'retracted' }).Count
            $graphConflicts = @($operations | Where-Object { $_.kind -eq 'event_graph_conflict' })
            $safeGraphConflicts = @($graphConflicts | Where-Object { $_.result_kind -eq 'not_saved' -and $_.outcome_kind -eq 'no_automatic_inference' }).Count
            $allTimes = @($operations | Where-Object {
                $_.succeeded -and (($_.operation -eq 'preview' -and $_.result_kind -eq 'all_times') -or
                    ($_.operation -eq 'query' -and $_.result_kind -eq 'resolved_all_times'))
            }).Count
            $points = @($operations | Where-Object {
                $_.succeeded -and (($_.operation -eq 'preview' -and $_.result_kind -eq 'point') -or
                    ($_.operation -eq 'query' -and $_.result_kind -eq 'resolved_point'))
            }).Count
            $historyQueries = @($operations | Where-Object { $_.operation -eq 'query' -and $_.query_mode -eq 'history' -and $_.result_kind -eq 'history' }).Count
            $explainQueries = @($operations | Where-Object { $_.operation -eq 'query' -and $_.query_mode -eq 'explain' -and $_.result_kind -eq 'explain' }).Count
            $tokenSearchPages = @($operations | Where-Object { $_.operation -eq 'query' -and $_.query_mode -eq 'token_search' -and $_.result_kind -eq 'token_search_page' }).Count
            $tokenSearchCompletePages = @($operations | Where-Object { $_.operation -eq 'query' -and $_.query_mode -eq 'token_search' -and $_.result_kind -eq 'token_search_complete' }).Count
            $graphQueries = @($operations | Where-Object { $_.operation -eq 'query' -and $_.query_mode -eq 'graph' -and $_.result_kind -eq 'graph' }).Count
            $countQueries = @($operations | Where-Object { $_.operation -eq 'query' -and $_.query_mode -eq 'count' -and $_.result_kind -eq 'aggregate_count' }).Count
            $existsQueries = @($operations | Where-Object { $_.operation -eq 'query' -and $_.query_mode -eq 'exists' -and $_.result_kind -eq 'aggregate_exists' }).Count
            $groupedCountQueries = @($operations | Where-Object { $_.operation -eq 'query' -and $_.query_mode -eq 'grouped_count' -and $_.result_kind -eq 'aggregate_grouped_count' }).Count
            $historicalSchemaQueries = @($operations | Where-Object { $_.operation -eq 'query' -and $_.schema_mode -eq 'historical' }).Count
            $currentSchemaQueries = @($operations | Where-Object { $_.operation -eq 'query' -and $_.schema_mode -eq 'current' }).Count
            $explicitSchemaQueries = @($operations | Where-Object { $_.operation -eq 'query' -and $_.schema_mode -eq 'explicit' }).Count
            $olderRecordedAsOfQueries = @($operations | Where-Object {
                $_.operation -eq 'query' -and $null -ne $_.recorded_as_of -and
                    [decimal]::Parse([string]$_.recorded_as_of) -lt [decimal]::Parse([string]$_.snapshot_revision_exact)
            }).Count
            $selectors = @($masks | Select-Object -ExpandProperty selector_kind -Unique)
            $hasExact = $selectors -contains 'exact_assertion'
            $hasProposition = $selectors -contains 'proposition'
            $hasSlot = $selectors -contains 'slot'
            if (
                $assertions -ge 3 -and $masks.Count -ge 3 -and $boundaries -ge 1 -and
                $corrections -ge 1 -and $catalogs -ge 1 -and $lifecycle.Count -ge 3 -and
                ($lifecycleEffects -contains 'retracted') -and ($lifecycleEffects -contains 'archived') -and
                ($lifecycleEffects -contains 'unarchived') -and
                $events -ge 3 -and $eventMasks -ge 1 -and $eventRelations -ge 5 -and
                $spanClosures -ge 1 -and $eventRetractions -ge 1 -and $eventMaskRetractions -ge 1 -and
                $safeGraphConflicts -ge 3 -and
                $sources -ge 1 -and $sourceSupersessions -ge 1 -and $evidence -ge 1 -and
                $provenance -ge 1 -and $evidenceRetractions -ge 1 -and $provenanceRetractions -ge 1 -and
                $metaHistoryComplete -and $projectSmokeComplete -and $recoverySmokeComplete -and $backupRendererPathsRejected -and $exportImportRendererPathsRejected -and $purgeRendererPathsRejected -and
                $diagnosticCanaryRejected -and $diagnosticRendererPathsRejected -and
                $allTimes -ge 7 -and $points -ge 1 -and
                $historyQueries -ge 1 -and $explainQueries -ge 1 -and
                $tokenSearchPages -ge 1 -and $tokenSearchCompletePages -ge 1 -and
                $graphQueries -ge 1 -and $countQueries -ge 1 -and
                $existsQueries -ge 1 -and $groupedCountQueries -ge 1 -and
                $historicalSchemaQueries -ge 1 -and $currentSchemaQueries -ge 1 -and
                $explicitSchemaQueries -ge 1 -and $olderRecordedAsOfQueries -ge 1 -and
                $hasExact -and $hasProposition -and $hasSlot -and
                $failedAssertions.Count -eq 1 -and $expectedCommitConflict -and $unknownCommitResolved
            ) {
                return
            }
        }
        $Process.Refresh()
        if ($Process.HasExited) { break }
        Start-Sleep -Milliseconds 100
    }
    $events = if (Test-Path -LiteralPath $PrimaryPath -PathType Leaf) { Get-Content -LiteralPath $PrimaryPath -Raw } else { '<missing>' }
    throw "Timed out waiting for factual-record, Event, EventMask, graph-conflict, History, Resolved, Explain, paged TokenSearch, Graph, and aggregate IPC workflows. Recorded: $events"
}

try {
    $env:WORLDDB_ODE_RESULT = $reportPath
    $env:WORLDDB_ODE_IPC_RESULT = $ipcPrefix
    $env:WORLDDB_ODE_SCHEMA_SMOKE_RESULT = $ipcPrefix
    $env:WORLDDB_ODE_ENTITY_SMOKE_RESULT = $ipcPrefix
    $env:WORLDDB_ODE_BRANCH_LAYER_SMOKE_RESULT = $ipcPrefix
    $env:WORLDDB_ODE_TRANSFER_SMOKE_RESULT = $ipcPrefix
    $env:WORLDDB_ODE_PERSPECTIVE_SMOKE_RESULT = $ipcPrefix
    $env:WORLDDB_ODE_SECURITY_POLICY_SMOKE_RESULT = $ipcPrefix
    $env:WORLDDB_ODE_FACTS_SMOKE_RESULT = $ipcPrefix
    $env:WORLDDB_ODE_UNKNOWN_COMMIT_OPERATION_ID = '00000000-0000-7000-8000-000000000041'
    $env:WORLDDB_ODE_PROJECT_SMOKE_ROOT = $databaseRoot
    $env:WORLDDB_ODE_AUTOCLOSE_MS = [string][Math]::Max(300000, ($FactsTimeoutSeconds + 120) * 1000)
    $env:WORLDDB_ODE_ENGINE_PRINCIPAL_ID = '00000000-0000-7000-8000-000000000099'
    if ($Mode -eq 'sidecar' -and $EngineExecutablePath) {
        $env:WORLDDB_ODE_ENGINE_EXECUTABLE = [System.IO.Path]::GetFullPath($EngineExecutablePath)
    } else {
        Remove-Item Env:\WORLDDB_ODE_ENGINE_EXECUTABLE -ErrorAction SilentlyContinue
    }

    $stdoutPath = Join-Path $testRoot 'ipc.stdout.log'
    $stderrPath = Join-Path $testRoot 'ipc.stderr.log'
    $startParameters = @{
        FilePath = $executable
        PassThru = $true
        RedirectStandardOutput = $stdoutPath
        RedirectStandardError = $stderrPath
    }
    if ($isWindowsPlatform) { $startParameters.WindowStyle = 'Hidden' }
    $process = Start-Process @startParameters
    if ($isWindowsPlatform) { $processHandle = $process.Handle }

    Wait-ForFiles $process @($reportPath, $primaryPath, $secondaryPath, $primaryProjectPath, $secondaryProjectPath, $primarySchemaPath, $secondarySchemaPath)
    Wait-ForSchemaOperations $process $primarySchemaPath $secondarySchemaPath $primaryFactsPath
    Wait-ForEntityOperations $process $primaryEntityPath $secondaryEntityPath $primaryFactsPath
    Wait-ForBranchLayerOperations $process $primaryBranchLayerPath $secondaryBranchLayerPath
    Wait-ForTransferOperations $process $primaryTransferPath $primaryFactsPath
    Wait-ForPerspectiveOperations $process $primaryPerspectivePath $secondaryPerspectivePath $primaryFactsPath
    Wait-ForSecurityPolicyOperations $process $primarySecurityPolicyPath
    Wait-ForFactsOperations $process $primaryFactsPath
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
    $primaryPerspectives = @(Get-Content -LiteralPath $primaryPerspectivePath | ForEach-Object { $_ | ConvertFrom-Json })
    $secondaryPerspectives = @(Get-Content -LiteralPath $secondaryPerspectivePath | ForEach-Object { $_ | ConvertFrom-Json })
    $primarySecurityPolicy = @(Get-Content -LiteralPath $primarySecurityPolicyPath | ForEach-Object { $_ | ConvertFrom-Json })
    $secondarySecurityPolicy = if (Test-Path -LiteralPath $secondarySecurityPolicyPath -PathType Leaf) {
        @(Get-Content -LiteralPath $secondarySecurityPolicyPath | ForEach-Object { $_ | ConvertFrom-Json })
    } else { @() }
    $primaryFacts = @(Get-Content -LiteralPath $primaryFactsPath | ForEach-Object { $_ | ConvertFrom-Json })
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
        if ($entry.Value.compatibility.storage_format -ne 'current_v1' -or $entry.Value.compatibility.format_upgrade_policy -ne 'explicit_only' -or $entry.Value.compatibility.schema_migration_policy -ne 'explicit_only' -or $entry.Value.compatibility.migration_applied_during_open -ne $false) {
            throw "The $($entry.Label) window did not report the read-only format and migration compatibility result."
        }
    }
    if ($primaryProject.database_id -ne $secondaryProject.database_id) { throw 'Both windows did not resolve the same WorldDB project.' }
    if ($primaryProject.snapshot_id -eq $secondaryProject.snapshot_id) { throw 'The native windows received the same project snapshot identity.' }
    if (@($primarySchema | Where-Object { -not $_.succeeded }).Count -gt 0) { throw 'The primary window had a rejected schema IPC operation.' }
    if (@($secondarySchema | Where-Object { -not $_.succeeded -and $_.project_open -ne $false }).Count -gt 0) { throw 'The secondary window had a rejected schema read while a project was open.' }
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
    $schemaDefinitions = @($primarySchema | ForEach-Object { $_.definitions })
    foreach ($definitionCheck in @(
        @{ Family = 'timeline'; Symbol = 'ipc_smoke_timeline' },
        @{ Family = 'time_unit'; Symbol = 'ipc_smoke_max_scale' }
    )) {
        $publishedStates = @($schemaDefinitions | Where-Object {
            $_.family -eq $definitionCheck.Family -and $_.symbol -eq $definitionCheck.Symbol
        } | Select-Object -ExpandProperty lifecycle -Unique)
        foreach ($lifecycle in @('active', 'deprecated', 'retired')) {
            if ($publishedStates -notcontains $lifecycle) {
                throw "The $($definitionCheck.Family) '$($definitionCheck.Symbol)' did not publish lifecycle '$lifecycle'."
            }
        }
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
    if (@($primaryPerspectives | Where-Object { -not $_.succeeded -and $_.operation -ne 'validate_context' }).Count -gt 0) {
        throw 'The primary window had an unexpected rejected Perspective IPC operation.'
    }
    $requiredPerspectiveOperations = @('snapshot_current', 'snapshot_historical', 'snapshot_explicit', 'create', 'update', 'retire', 'validate_context')
    foreach ($operation in $requiredPerspectiveOperations) {
        if (@($primaryPerspectives | Where-Object { $_.operation -eq $operation }).Count -eq 0) {
            throw "The primary window did not complete Perspective IPC operation '$operation'."
        }
    }
    $acceptedContexts = @($primaryPerspectives | Where-Object { $_.operation -eq 'validate_context' -and $_.succeeded }).Count
    $rejectedContexts = @($primaryPerspectives | Where-Object { $_.operation -eq 'validate_context' -and -not $_.succeeded }).Count
    if ($acceptedContexts -lt 2 -or $rejectedContexts -lt 3) {
        throw 'Valid and invalid Perspective context bindings were not distinguished.'
    }
    if (@($secondaryPerspectives | Where-Object { $_.operation -eq 'snapshot_current' -and $_.succeeded }).Count -eq 0) {
        throw 'The secondary window did not read the shared Perspective catalog.'
    }
    if (@($primarySecurityPolicy | Where-Object { -not $_.succeeded }).Count -gt 0) {
        throw 'The primary window had a rejected security policy IPC operation.'
    }
    $policyMutations = @($primarySecurityPolicy | Where-Object {
        $_.operation -in @('assign_role', 'revoke_role_assignment', 'add_capability_rule', 'revoke_capability_rule')
    })
    if ($policyMutations.Count -ne 4) { throw 'The security policy smoke did not complete all four role/capability changes.' }
    for ($index = 1; $index -lt $policyMutations.Count; $index++) {
        if ($policyMutations[$index].security_epoch -ne $policyMutations[$index - 1].security_epoch + 1) {
            throw 'A security policy change did not advance SecurityEpoch exactly once.'
        }
    }
    $policySnapshots = @($primarySecurityPolicy | Where-Object { $_.operation -eq 'snapshot' -and $_.succeeded })
    if ($policySnapshots.Count -lt 3) { throw 'Policy snapshots did not show state before, during, and after the changes.' }
    $baselineEpoch = $policyMutations[0].security_epoch - 1
    $baselineSnapshots = @($policySnapshots | Where-Object { $_.security_epoch -eq $baselineEpoch })
    $assignmentSnapshots = @($policySnapshots | Where-Object { $_.security_epoch -eq $policyMutations[0].security_epoch })
    $ruleSnapshots = @($policySnapshots | Where-Object { $_.security_epoch -eq $policyMutations[2].security_epoch })
    $finalSnapshots = @($policySnapshots | Where-Object { $_.security_epoch -eq $policyMutations[3].security_epoch })
    if ($baselineSnapshots.Count -eq 0 -or @($baselineSnapshots | Where-Object { $_.gm_admin_raw_allow -ne $false -or $_.gm_raw_history_allow -ne $true }).Count -gt 0) {
        throw 'The initial GM bundle does not distinguish RawHistoryRead from AdminRawRead as specified.'
    }
    $baselineAssignmentCount = $baselineSnapshots[0].assignment_count
    $baselineRuleCount = $baselineSnapshots[0].explicit_rule_count
    if (@($assignmentSnapshots | Where-Object { $_.assignment_count -eq $baselineAssignmentCount + 1 }).Count -eq 0) {
        throw 'The role assignment was not reflected in a policy snapshot at its committed epoch.'
    }
    if (@($ruleSnapshots | Where-Object { $_.assignment_count -eq $baselineAssignmentCount }).Count -eq 0) {
        throw 'Role assignment and revocation were not reflected in policy snapshots.'
    }
    if (@($ruleSnapshots | Where-Object { $_.gm_admin_raw_deny -eq $true -and $_.explicit_rule_count -gt $baselineRuleCount }).Count -eq 0) {
        throw 'The explicit GM AdminRawRead deny was not kept separate from the role bundle.'
    }
    if ($finalSnapshots.Count -gt 0 -and @($finalSnapshots | Where-Object { $_.explicit_rule_count -eq $baselineRuleCount }).Count -eq 0) {
        throw 'Revoking the temporary capability rule did not restore the original explicit-rule inventory.'
    }
    if (($secondarySecurityPolicy.Count -eq 0) -or
        (@($secondarySecurityPolicy | Where-Object { $_.operation -eq 'snapshot' -and $_.succeeded }).Count -eq 0)) {
        throw 'The secondary window did not read shared security policy.'
    }

    $process.Refresh()
    $processIds = @([int]$process.Id)
    $processIds += [int]$primaryProject.engine.engine_process_id
    foreach ($processId in $processIds) {
        $listeners = @(Get-ListeningTcpConnections $processId)
        if ($listeners.Count -gt 0) {
            throw "WorldDB process $processId unexpectedly listens on a network port."
        }
    }

    $processWaitMilliseconds = [int][Math]::Max(240000, ($FactsTimeoutSeconds + 60) * 1000)
    if (-not $process.WaitForExit($processWaitMilliseconds)) {
        $process.Kill()
        throw 'The IPC smoke process did not shut down.'
    }
    $process.Refresh()
    [uint32]$processExitCode = 0
    if ($isWindowsPlatform) {
        if (-not [WorldDbIpcSmokeNative]::GetExitCodeProcess($processHandle, [ref]$processExitCode)) {
            $nativeError = [Runtime.InteropServices.Marshal]::GetLastWin32Error()
            throw "Could not read the exited IPC smoke process code (Windows error $nativeError)."
        }
        if ($processExitCode -eq 259) { throw 'The IPC smoke process still reports itself as active after WaitForExit.' }
    } else {
        $processExitCode = [uint32]$process.ExitCode
    }
    if ($processExitCode -ne 0) { throw "The IPC smoke process exited with code $processExitCode." }
    $smokePassed = $true

    [pscustomobject]@{
        mode = $Mode
        primary_window_authenticated_health = 'PASS'
        secondary_window_authenticated_health = 'PASS'
        authenticated_project_bootstrap = 'PASS'
        transactional_schema_create_and_lifecycle = 'PASS'
        exact_timeline_calendar_epoch_publication = 'PASS'
        exact_positive_time_unit_scale_publication = 'PASS'
        timeline_and_time_unit_lifecycle_states_visible = 'PASS'
        out_of_range_epoch_and_zero_scale_rejected = 'PASS'
        current_historical_and_explicit_schema_reads = 'PASS'
        secondary_window_schema_read = 'PASS'
        transactional_entity_create_and_retirement = 'PASS'
        current_historical_and_explicit_entity_reads = 'PASS'
        deprecated_entity_type_opt_in_warning = 'PASS'
        secondary_window_entity_read = 'PASS'
        transactional_branch_layer_creation_and_base_switch = 'PASS'
        current_and_historical_branch_layer_reads = 'PASS'
        stale_branch_write_rejected_without_publication = 'PASS'
        stale_commit_conflict_rejected_without_publication = 'PASS'
        unknown_commit_outcome_reconciled_by_operation_id = 'PASS'
        secondary_window_branch_layer_read = 'PASS'
        authenticated_history_space_transfer_catalog = 'PASS'
        authenticated_perspective_catalog_and_contexts = 'PASS'
        current_historical_and_explicit_perspective_reads = 'PASS'
        assertion_mask_and_replacement_boundary_forms = 'PASS'
        exact_proposition_and_slot_mask_selectors = 'PASS'
        point_and_all_times_resolution_previews = 'PASS'
        raw_history_resolved_explain_queries = 'PASS'
        recorded_as_of_and_explicit_schema_modes = 'PASS'
        paged_token_search_cursor_and_expiry_message = 'PASS'
        authorized_graph_traversal_and_limits = 'PASS'
        complete_count_exists_and_grouped_count = 'PASS'
        event_roles_attributes_and_instant_span_creation = 'PASS'
        explicit_event_span_closure_and_event_retractions = 'PASS'
        event_mask_priority_and_separate_retraction = 'PASS'
        canonical_event_relations_and_safe_graph_conflicts = 'PASS'
        source_creation_and_replacement_lineage = 'PASS'
        evidence_and_provenance_creation = 'PASS'
        separate_evidence_and_provenance_retractions = 'PASS'
        world_state_and_epistemic_contexts_separate = 'PASS'
        invalid_and_retired_perspectives_rejected = 'PASS'
        secondary_window_perspective_read = 'PASS'
        shared_project_with_distinct_window_snapshots = 'PASS'
        versioned_ipc_protocol = 'PASS'
        invalid_session_rejected_in_both_windows = 'PASS'
        renderer_selected_path_rejected_in_both_windows = 'PASS'
        backup_renderer_paths_rejected_before_host_dialogs = 'PASS'
        export_import_renderer_paths_rejected_before_host_dialogs = 'PASS'
        purge_renderer_paths_rejected_before_host_dialogs = 'PASS'
        diagnostic_public_error_canary_rejected = 'PASS'
        diagnostic_renderer_paths_rejected_before_host_dialogs = 'PASS'
        filesystem_plugin_command_rejected_in_both_windows = 'PASS'
        host_principal_not_selected_by_environment = 'PASS'
        core_network_listeners = 'PASS'
        background_job_journal_and_renderer = 'PASS'
        background_job_shutdown_drain = 'PASS'
        process_shutdown = 'PASS'
    } | ConvertTo-Json -Compress
}
finally {
    foreach ($name in @('WORLDDB_ODE_DATABASE', 'WORLDDB_ODE_RESULT', 'WORLDDB_ODE_IPC_RESULT', 'WORLDDB_ODE_SCHEMA_SMOKE_RESULT', 'WORLDDB_ODE_ENTITY_SMOKE_RESULT', 'WORLDDB_ODE_BRANCH_LAYER_SMOKE_RESULT', 'WORLDDB_ODE_TRANSFER_SMOKE_RESULT', 'WORLDDB_ODE_PERSPECTIVE_SMOKE_RESULT', 'WORLDDB_ODE_SECURITY_POLICY_SMOKE_RESULT', 'WORLDDB_ODE_FACTS_SMOKE_RESULT', 'WORLDDB_ODE_PROJECT_SMOKE_ROOT', 'WORLDDB_ODE_AUTOCLOSE_MS', 'WORLDDB_ODE_ENGINE_EXECUTABLE', 'WORLDDB_ODE_ENGINE_PRINCIPAL_ID', 'WORLDDB_ODE_UNKNOWN_COMMIT_OPERATION_ID')) {
        Remove-Item "Env:\$name" -ErrorAction SilentlyContinue
    }
    if ($null -ne $process) {
        $process.Refresh()
        if (-not $process.HasExited) {
            $process.Kill()
            $process.WaitForExit()
        }
    }
    Stop-SmokeSidecarChild
    $resolvedRoot = [System.IO.Path]::GetFullPath($testRoot)
    $tempPrefix = $tempBase.TrimEnd([System.IO.Path]::DirectorySeparatorChar) + [System.IO.Path]::DirectorySeparatorChar
    if (-not $resolvedRoot.StartsWith($tempPrefix, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw 'Refusing to remove a smoke-test directory outside the system temp directory.'
    }
    if ($KeepArtifacts -and -not $smokePassed) {
        $retainedRoot = $resolvedRoot
        if ($ArtifactsRoot) {
            $destination = [System.IO.Path]::GetFullPath($ArtifactsRoot)
            if ($destination.StartsWith($resolvedRoot + [System.IO.Path]::DirectorySeparatorChar, [System.StringComparison]::OrdinalIgnoreCase) -or (Test-Path -LiteralPath $destination)) {
                throw 'The artifact destination must be new and outside the temporary smoke directory.'
            }
            $destinationParent = [System.IO.Path]::GetDirectoryName($destination)
            $null = New-Item -ItemType Directory -Path $destinationParent -Force
            $null = New-Item -ItemType Directory -Path $destination
            Get-ChildItem -LiteralPath $resolvedRoot -Force | Copy-Item -Destination $destination -Recurse
            Remove-Item -LiteralPath $resolvedRoot -Recurse -Force
            $retainedRoot = $destination
        }
        Write-Warning "Preserved failed smoke artifacts at $retainedRoot"
    } elseif (Test-Path -LiteralPath $resolvedRoot) {
        Remove-Item -LiteralPath $resolvedRoot -Recurse -Force
    }
}
