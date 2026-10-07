param(
    [string]$EvidenceRoot = (Join-Path $PSScriptRoot '..\evidence\native-e2e')
)

$ErrorActionPreference = 'Stop'
if ([System.Environment]::OSVersion.Platform -ne [System.PlatformID]::Win32NT) {
    throw 'This entry point runs the Windows/NTFS profile. macOS and Linux profiles are deferred to M9-07.'
}

$workspaceRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..\..'))
$odeRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$manifestPath = Join-Path $PSScriptRoot '..\e2e\native-suite.json'
$manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
$caseSpecs = @{}
$caseDrivers = @{}
foreach ($caseSpec in @($manifest.cases)) {
    $caseId = [string]$caseSpec.id
    if ([string]::IsNullOrWhiteSpace($caseId) -or $caseSpecs.ContainsKey($caseId)) {
        throw 'The native E2E case catalog contains a missing or duplicate case id.'
    }
    $relativeDriver = [string]$caseSpec.drivers.windows
    if ([string]::IsNullOrWhiteSpace($relativeDriver) -or [System.IO.Path]::IsPathRooted($relativeDriver) -or $relativeDriver.Contains('..')) {
        throw "The Windows driver for native E2E case '$caseId' is missing or not repository-relative."
    }
    $driverPath = [System.IO.Path]::GetFullPath((Join-Path $odeRoot $relativeDriver.Replace('/', [System.IO.Path]::DirectorySeparatorChar)))
    $odeRootPrefix = $odeRoot.TrimEnd([char[]]@('\', '/')) + [System.IO.Path]::DirectorySeparatorChar
    if (-not $driverPath.StartsWith($odeRootPrefix, [System.StringComparison]::OrdinalIgnoreCase) -or
        -not (Test-Path -LiteralPath $driverPath -PathType Leaf)) {
        throw "The Windows driver for native E2E case '$caseId' is missing or escapes the suite root."
    }
    $caseSpecs[$caseId] = $caseSpec
    $caseDrivers[$caseId] = $driverPath
}
$gitCommit = (& git -C $workspaceRoot rev-parse HEAD 2>&1 | Out-String).Trim()
$gitBranch = (& git -C $workspaceRoot branch --show-current 2>&1 | Out-String).Trim()
$gitDirty = @(& git -C $workspaceRoot status --porcelain).Count -gt 0
$utcStart = [DateTimeOffset]::UtcNow
$runId = 'm8-26a-' + $utcStart.ToString("yyyyMMdd'T'HHmmss'Z'") + '-' + [guid]::NewGuid().ToString('N').Substring(0, 8)
$resolvedEvidenceRoot = [System.IO.Path]::GetFullPath($EvidenceRoot)
$runRoot = Join-Path $resolvedEvidenceRoot $runId
if (Test-Path -LiteralPath $runRoot) {
    throw "Evidence directory already exists: $runRoot"
}
$null = New-Item -ItemType Directory -Path $runRoot -Force
$null = New-Item -ItemType Directory -Path (Join-Path $runRoot 'logs')
$null = New-Item -ItemType Directory -Path (Join-Path $runRoot 'raw')

$cases = [System.Collections.Generic.List[object]]::new()
$builds = [System.Collections.Generic.List[object]]::new()
$failures = [System.Collections.Generic.List[object]]::new()
$recoveryCliEvidence = $null
$priorTargetDirectory = Get-Item Env:\CARGO_TARGET_DIR -ErrorAction SilentlyContinue
$targetDirectory = Join-Path $odeRoot 'target'
$env:CARGO_TARGET_DIR = $targetDirectory
$cargoCommand = Get-Command cargo.exe -ErrorAction SilentlyContinue
$cargoPath = if ($null -ne $cargoCommand) { $cargoCommand.Source } else { Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe' }

function Format-Utc([DateTimeOffset]$Value) {
    return $Value.ToUniversalTime().ToString("yyyy-MM-dd'T'HH:mm:ss.fff'Z'")
}

function Protect-LocalPaths([string]$Value) {
    $protected = $Value
    foreach ($localPath in @($workspaceRoot, [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath()), $env:USERPROFILE)) {
        if ($localPath) {
            $protected = [regex]::Replace($protected, [regex]::Escape($localPath), '<local-path>', [System.Text.RegularExpressions.RegexOptions]::IgnoreCase)
        }
    }
    return $protected
}

function Add-CaseResult(
    [string]$CaseId,
    [string]$Mode,
    [string]$Status,
    [Nullable[DateTimeOffset]]$StartedAt,
    [Nullable[DateTimeOffset]]$FinishedAt,
    [Nullable[int]]$ExitCode,
    [int]$ChecksPassed,
    [string]$Details,
    [string[]]$ArtifactPaths
) {
    $cases.Add([pscustomobject]@{
        case_id = $CaseId
        mode = $Mode
        status = $Status
        started_at = if ($StartedAt.HasValue) { Format-Utc $StartedAt.Value } else { $null }
        finished_at = if ($FinishedAt.HasValue) { Format-Utc $FinishedAt.Value } else { $null }
        exit_code = if ($ExitCode.HasValue) { $ExitCode.Value } else { $null }
        checks_passed = $ChecksPassed
        details = $Details
        artifacts = @($ArtifactPaths)
    })
}

function Invoke-Build([string]$Mode, [string[]]$CargoArguments, [string]$AppPath, [string]$EnginePath) {
    $started = [DateTimeOffset]::UtcNow
    $logPath = Join-Path $runRoot ("logs\build-$Mode.log")
    Push-Location $odeRoot
    try {
        $buildOutput = @(& $cargoPath @CargoArguments 2>&1)
        $exitCode = $LASTEXITCODE
    } catch {
        $buildOutput = @($_.Exception.Message)
        $exitCode = 1
    } finally {
        Pop-Location
    }
    $buildOutput | ForEach-Object { Protect-LocalPaths $_.ToString() } | Set-Content -LiteralPath $logPath -Encoding UTF8
    if ($exitCode -ne 0 -or -not (Test-Path -LiteralPath $AppPath -PathType Leaf)) {
        $message = "The $Mode native desktop build failed (exit code $exitCode)."
        $failures.Add([pscustomobject]@{ case_id = "build_$Mode"; message = $message; artifact_paths = @("logs\build-$Mode.log") })
        throw $message
    }
    $appItem = Get-Item -LiteralPath $AppPath
    $engineItem = if ($EnginePath -and (Test-Path -LiteralPath $EnginePath -PathType Leaf)) { Get-Item -LiteralPath $EnginePath } else { $null }
    $builds.Add([pscustomobject]@{
        mode = $Mode
        app_path = [System.IO.Path]::GetRelativePath($workspaceRoot, $AppPath)
        app_sha256 = (Get-FileHash -LiteralPath $AppPath -Algorithm SHA256).Hash.ToLowerInvariant()
        app_bytes = [long]$appItem.Length
        engine_path = if ($null -ne $engineItem) { [System.IO.Path]::GetRelativePath($workspaceRoot, $EnginePath) } else { $null }
        engine_sha256 = if ($null -ne $engineItem) { (Get-FileHash -LiteralPath $EnginePath -Algorithm SHA256).Hash.ToLowerInvariant() } else { $null }
        engine_bytes = if ($null -ne $engineItem) { [long]$engineItem.Length } else { $null }
    })
    $finished = [DateTimeOffset]::UtcNow
    Add-CaseResult "build_$Mode" $Mode 'PASS' $started $finished 0 1 "Cargo native build completed; log: logs\build-$Mode.log" @("logs\build-$Mode.log")
}

function Invoke-RecoveryCliBuild {
    $started = [DateTimeOffset]::UtcNow
    $logPath = Join-Path $runRoot 'logs\build-recovery-cli.log'
    $rootTargetDirectory = Join-Path $workspaceRoot 'target'
    $env:CARGO_TARGET_DIR = $rootTargetDirectory
    Push-Location $workspaceRoot
    try {
        $buildOutput = @(& $cargoPath 'build' '--locked' '--offline' '-p' 'worlddb-cli' 2>&1)
        $exitCode = $LASTEXITCODE
    } catch {
        $buildOutput = @($_.Exception.Message)
        $exitCode = 1
    } finally {
        Pop-Location
        $env:CARGO_TARGET_DIR = $targetDirectory
    }
    $buildOutput | ForEach-Object { Protect-LocalPaths $_.ToString() } | Set-Content -LiteralPath $logPath -Encoding UTF8
    $cliPath = Join-Path $rootTargetDirectory 'debug\worlddb-cli.exe'
    if ($exitCode -ne 0 -or -not (Test-Path -LiteralPath $cliPath -PathType Leaf)) {
        $message = "The read-only recovery CLI build failed (exit code $exitCode)."
        $failures.Add([pscustomobject]@{ case_id = 'build_recovery_cli'; message = $message; artifact_paths = @('logs\build-recovery-cli.log') })
        throw $message
    }
    $cliItem = Get-Item -LiteralPath $cliPath
    $script:recoveryCliEvidence = [pscustomobject]@{
        path = [System.IO.Path]::GetRelativePath($workspaceRoot, $cliPath)
        sha256 = (Get-FileHash -LiteralPath $cliPath -Algorithm SHA256).Hash.ToLowerInvariant()
        bytes = [long]$cliItem.Length
    }
    $finished = [DateTimeOffset]::UtcNow
    Add-CaseResult 'build_recovery_cli' 'both' 'PASS' $started $finished 0 1 'Read-only recovery CLI built for crash-recovery verification.' @('logs\build-recovery-cli.log')
}

function Invoke-Smoke([string]$CaseId, [string]$Mode, [string]$ScriptPath, [hashtable]$Arguments, [int]$MinimumPassChecks) {
    $started = [DateTimeOffset]::UtcNow
    $modeFolder = $CaseId + '-' + $Mode
    $rawPath = Join-Path $runRoot ("raw\" + $modeFolder)
    $logRelative = "logs\$modeFolder.log"
    $logPath = Join-Path $runRoot $logRelative
    $details = ''
    $exitCode = 0
    $checksPassed = 0
    $status = 'FAIL'
    try {
        $Arguments['KeepArtifacts'] = $true
        $Arguments['ArtifactsRoot'] = $rawPath
        $output = @(& $ScriptPath @Arguments *>&1)
        $lines = @($output | ForEach-Object { Protect-LocalPaths $_.ToString() })
        $lines | Set-Content -LiteralPath $logPath -Encoding UTF8
        $summaryLine = $lines | Where-Object { $_.TrimStart().StartsWith('{') } | Select-Object -Last 1
        if (-not $summaryLine) {
            throw 'The native smoke did not produce a JSON summary.'
        }
        $summary = $summaryLine | ConvertFrom-Json
        $properties = @($summary.PSObject.Properties)
        $checksPassed = @($properties | Where-Object { $_.Value -eq 'PASS' }).Count
        $invalid = @($properties | Where-Object { $_.Value -eq 'FAIL' })
        if ($checksPassed -lt $MinimumPassChecks -or $invalid.Count -gt 0) {
            throw "Expected at least $MinimumPassChecks PASS checks; got $checksPassed. Non-PASS fields: $($invalid.Name -join ', ')."
        }
        $status = 'PASS'
        $details = "$checksPassed checks passed; summary is recorded in $logRelative."
    } catch {
        $exitCode = 1
        $details = Protect-LocalPaths $_.Exception.Message
        $failures.Add([pscustomobject]@{
            case_id = $CaseId
            message = $details
            artifact_paths = @($logRelative, $modeFolder)
        })
        if (-not (Test-Path -LiteralPath $logPath)) {
            $details | Set-Content -LiteralPath $logPath -Encoding UTF8
        }
    }
    $finished = [DateTimeOffset]::UtcNow
    $artifacts = @($logRelative)
    if (Test-Path -LiteralPath $rawPath -PathType Container) {
        $artifacts += "raw\$modeFolder"
    }
    Add-CaseResult $CaseId $Mode $status $started $finished $exitCode $checksPassed $details $artifacts
}

function Invoke-ConfiguredSmoke([string]$CaseId, [string]$Mode, [hashtable]$Arguments, [int]$MinimumPassChecks) {
    if (-not $script:caseSpecs.ContainsKey($CaseId)) {
        throw "Native E2E case '$CaseId' is not declared in the shared case catalog."
    }
    $caseSpec = $script:caseSpecs[$CaseId]
    if (@($caseSpec.modes) -notcontains $Mode) {
        throw "Native E2E case '$CaseId' does not declare process mode '$Mode'."
    }
    Invoke-Smoke $CaseId $Mode $script:caseDrivers[$CaseId] $Arguments $MinimumPassChecks
}

function Add-NotRun([string]$CaseId, [string]$Mode, [string]$Details) {
    Add-CaseResult $CaseId $Mode 'NOT_RUN' $null $null $null 0 $Details @()
}

try {
    if (-not (Test-Path -LiteralPath $cargoPath -PathType Leaf)) {
        throw 'Rust Cargo was not found.'
    }
    Invoke-RecoveryCliBuild
    $appPath = Join-Path $targetDirectory 'debug\worlddb-ode-desktop-shell.exe'
    $enginePath = Join-Path $targetDirectory 'debug\worlddb_ode_engine.exe'
    $manifestPath = Join-Path $odeRoot 'Cargo.toml'

    Invoke-Build 'in-process' @('build', '--locked', '--offline', '--manifest-path', $manifestPath, '--workspace') $appPath $null
    $profileArgs = @{
        Mode = 'in-process'
        ExecutablePath = $appPath
    }
    Invoke-ConfiguredSmoke 'native_ipc_in_process' 'in-process' $profileArgs 50
    $profileArgs = @{
        Mode = 'in-process'
        WorkspaceRoot = $workspaceRoot
        ExecutablePath = $appPath
    }
    Invoke-ConfiguredSmoke 'competing_process_in_process' 'in-process' $profileArgs 4
    $profileArgs = @{
        Mode = 'in-process'
        ExecutablePath = $appPath
    }
    Invoke-ConfiguredSmoke 'keyboard_navigation' 'in-process' $profileArgs 4
    $profileArgs = @{
        Mode = 'in-process'
        ExecutablePath = $appPath
        CrashDuringCommit = $true
        RecoveryCliPath = (Join-Path $workspaceRoot 'target\debug\worlddb-cli.exe')
    }
    Invoke-ConfiguredSmoke 'commit_crash_recovery' 'in-process' $profileArgs 3

    Invoke-Build 'sidecar' @('build', '--locked', '--offline', '--manifest-path', $manifestPath, '--workspace', '--no-default-features', '--features', 'sidecar') $appPath $enginePath
    $profileArgs = @{
        Mode = 'sidecar'
        ExecutablePath = $appPath
        EngineExecutablePath = $enginePath
    }
    Invoke-ConfiguredSmoke 'native_ipc_sidecar' 'sidecar' $profileArgs 50
    $profileArgs = @{
        Mode = 'sidecar'
        WorkspaceRoot = $workspaceRoot
        ExecutablePath = $appPath
        EngineExecutablePath = $enginePath
    }
    Invoke-ConfiguredSmoke 'competing_process_sidecar' 'sidecar' $profileArgs 5
    $profileArgs = @{
        Mode = 'sidecar'
        ExecutablePath = $appPath
    }
    Invoke-ConfiguredSmoke 'keyboard_navigation' 'sidecar' $profileArgs 4
    $profileArgs = @{
        Mode = 'sidecar'
        ExecutablePath = $appPath
        CrashDuringCommit = $true
        RecoveryCliPath = (Join-Path $workspaceRoot 'target\debug\worlddb-cli.exe')
    }
    Invoke-ConfiguredSmoke 'commit_crash_recovery' 'sidecar' $profileArgs 3
} catch {
    $message = $_.Exception.Message
    if ($failures.Count -eq 0 -or $failures[$failures.Count - 1].message -ne $message) {
        $failures.Add([pscustomobject]@{ case_id = 'suite_setup'; message = $message; artifact_paths = @() })
    }
    foreach ($caseId in @('native_ipc_in_process', 'competing_process_in_process', 'native_ipc_sidecar', 'competing_process_sidecar')) {
        if (@($cases | Where-Object { $_.case_id -eq $caseId }).Count -eq 0) {
            Add-CaseResult $caseId 'both' 'NOT_RUN' $null $null $null 0 'Not run because an earlier build or suite step failed.' @()
        }
    }
    foreach ($caseSpec in @(
        @{ case_id = 'keyboard_navigation'; mode = 'in-process' },
        @{ case_id = 'keyboard_navigation'; mode = 'sidecar' },
        @{ case_id = 'commit_crash_recovery'; mode = 'in-process' },
        @{ case_id = 'commit_crash_recovery'; mode = 'sidecar' }
    )) {
        if (@($cases | Where-Object { $_.case_id -eq $caseSpec.case_id -and $_.mode -eq $caseSpec.mode }).Count -eq 0) {
            Add-NotRun $caseSpec.case_id $caseSpec.mode 'Not run because an earlier build or suite step failed; implementation remains outstanding.'
        }
    }
}
finally {
    if ($null -eq $priorTargetDirectory) {
        Remove-Item Env:\CARGO_TARGET_DIR -ErrorAction SilentlyContinue
    } else {
        Set-Item Env:\CARGO_TARGET_DIR -Value $priorTargetDirectory.Value
    }
}

$osInfo = Get-CimInstance Win32_OperatingSystem
$tempRoot = [System.IO.Path]::GetPathRoot([System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath()))
$filesystem = ([System.IO.DriveInfo]::new($tempRoot)).DriveFormat
$rustcCommand = Get-Command rustc.exe -ErrorAction SilentlyContinue
$rustcPath = if ($null -ne $rustcCommand) { $rustcCommand.Source } else { Join-Path $env:USERPROFILE '.cargo\bin\rustc.exe' }
$cargoVersion = (& $cargoPath --version 2>&1 | Out-String).Trim()
$rustcVersion = if (Test-Path -LiteralPath $rustcPath -PathType Leaf) { (& $rustcPath --version 2>&1 | Out-String).Trim() } else { 'unavailable' }

$failedCases = @($cases | Where-Object { $_.status -eq 'FAIL' })
$incompleteCases = @($cases | Where-Object { $_.status -in @('NOT_RUN', 'DEFERRED') })
$overallStatus = if ($failedCases.Count -gt 0 -or $failures.Count -gt 0) { 'FAIL' } elseif ($incompleteCases.Count -gt 0) { 'INCOMPLETE' } else { 'PASS' }
$artifactRows = @(
    Get-ChildItem -LiteralPath $runRoot -File -Recurse |
        Where-Object { $_.Name -ne 'manifest.json' } |
        ForEach-Object {
            [pscustomobject]@{
                path = $_.FullName.Substring($runRoot.Length).TrimStart('\').Replace('\', '/')
                sha256 = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
                bytes = [long]$_.Length
            }
        }
)
$report = [pscustomobject]@{
    schema_version = 1
    suite_id = 'worlddb-ode-native-e2e'
    suite_version = [string]$manifest.suite_version
    task_id = 'M8-26a'
    run_id = $runId
    status = $overallStatus
    started_at = Format-Utc $utcStart
    finished_at = Format-Utc ([DateTimeOffset]::UtcNow)
    repository = [pscustomobject]@{
        commit = $gitCommit
        dirty = $gitDirty
        branch = $gitBranch
    }
    environment = [pscustomobject]@{
        os = 'Windows'
        os_version = "$($osInfo.Caption) $($osInfo.Version) (build $($osInfo.BuildNumber))"
        architecture = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
        filesystem = [string]$filesystem
        filesystem_root = $tempRoot
        shell = "PowerShell $($PSVersionTable.PSVersion)"
        runner = 'scripts/run-native-e2e.ps1'
        rustc = $rustcVersion
        cargo = $cargoVersion
    }
    builds = @($builds.ToArray())
    recovery_cli = $recoveryCliEvidence
    cases = @($cases.ToArray())
    artifacts = $artifactRows
    failures = @($failures.ToArray())
}
$reportPath = Join-Path $runRoot 'manifest.json'
$report | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath $reportPath -Encoding UTF8

Write-Output "Native E2E run $runId completed with status $overallStatus."
Write-Output "Evidence: $reportPath"
Write-Output "Cases: $($cases.Count) total; $(@($cases | Where-Object status -eq 'PASS').Count) PASS; $(@($cases | Where-Object status -eq 'NOT_RUN').Count) NOT_RUN."
if ($overallStatus -eq 'FAIL') {
    throw "Native E2E run failed. Review $reportPath and the archived failure artifacts."
}
