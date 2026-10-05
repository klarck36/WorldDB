param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('in-process', 'sidecar')]
    [string]$Mode,
    [string]$WorkspaceRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..\..')).Path,
    [string]$ExecutablePath = (Join-Path $WorkspaceRoot 'target\ode-002\debug\worlddb-ode-desktop-shell.exe'),
    [switch]$KeepArtifacts,
    [string]$ArtifactsRoot
)

$ErrorActionPreference = 'Stop'
if ($ArtifactsRoot -and -not $KeepArtifacts) {
    throw 'ArtifactsRoot requires KeepArtifacts.'
}

$executable = [System.IO.Path]::GetFullPath($ExecutablePath)
if (-not (Test-Path -LiteralPath $executable -PathType Leaf)) {
    throw 'Build the selected ODE-002 mode before running this smoke check.'
}

$tempBase = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
$testRoot = Join-Path $tempBase ('worlddb-ode002-' + $Mode + '-' + [guid]::NewGuid().ToString('N'))
$databaseRoot = Join-Path $testRoot 'database'
$null = New-Item -ItemType Directory -Path $testRoot
$firstResult = Join-Path $testRoot 'first.json'
$secondResult = Join-Path $testRoot 'second.json'
$reopenResult = Join-Path $testRoot 'reopen.json'
$first = $null
$second = $null
$reopen = $null
$smokePassed = $false

function Start-HiddenWorldDbApp([string]$Name, [string]$ResultPath, [int]$HoldMs) {
    $env:WORLDDB_ODE_DATABASE = $databaseRoot
    $env:WORLDDB_ODE_RESULT = $ResultPath
    $env:WORLDDB_ODE_AUTOCLOSE_MS = [string]$HoldMs
    $stdout = Join-Path $testRoot ($Name + '.stdout.log')
    $stderr = Join-Path $testRoot ($Name + '.stderr.log')
    return Start-Process -FilePath $executable -WindowStyle Hidden -PassThru `
        -RedirectStandardOutput $stdout -RedirectStandardError $stderr
}

function Wait-ForResult([System.Diagnostics.Process]$Process, [string]$Path) {
    $deadline = [DateTime]::UtcNow.AddSeconds(30)
    while (-not (Test-Path -LiteralPath $Path)) {
        $Process.Refresh()
        if ($Process.HasExited) {
            return $false
        }
        if ([DateTime]::UtcNow -ge $deadline) {
            throw 'Timed out waiting for the native-window startup result.'
        }
        Start-Sleep -Milliseconds 100
    }
    return $true
}

try {
    $first = Start-HiddenWorldDbApp 'first' $firstResult 8000
    if (-not (Wait-ForResult $first $firstResult)) {
        throw 'The first app instance failed before creating both windows and acquiring the writer lock.'
    }
    $firstReport = Get-Content -LiteralPath $firstResult -Raw | ConvertFrom-Json
    if ($firstReport.mode -ne ($Mode -replace '-', '_')) {
        throw 'The executable was not built in the selected process mode.'
    }
    if (@($firstReport.windows).Count -ne 2 -or $firstReport.engine.writer_owned -ne $true) {
        throw 'The first instance did not verify both native windows and the writer lock.'
    }

    $second = Start-HiddenWorldDbApp 'second' $secondResult 250
    if (-not $second.WaitForExit(30000)) {
        $second.Kill()
        throw 'The competing app instance did not terminate.'
    }
    $second.Refresh()
    if ($second.ExitCode -eq 0 -or (Test-Path -LiteralPath $secondResult)) {
        throw 'A second app instance acquired a writer lock or reported success.'
    }

    if (-not $first.WaitForExit(30000)) {
        $first.Kill()
        throw 'The first app instance did not release its writer lock.'
    }
    $first.Refresh()
    if ($null -ne $first.ExitCode -and $first.ExitCode -ne 0) {
        throw 'The first app instance did not shut down cleanly.'
    }

    $reopen = Start-HiddenWorldDbApp 'reopen' $reopenResult 250
    if (-not (Wait-ForResult $reopen $reopenResult) -or -not $reopen.WaitForExit(30000)) {
        $reopen.Kill()
        throw 'The database could not be reopened after the original writer exited.'
    }
    $reopen.Refresh()
    $reopenReport = Get-Content -LiteralPath $reopenResult -Raw | ConvertFrom-Json
    if (($null -ne $reopen.ExitCode -and $reopen.ExitCode -ne 0) -or $reopenReport.engine.writer_owned -ne $true) {
        throw 'The database writer lock was not released and reacquired correctly.'
    }

    $engineProcessIds = @(
        [int]$firstReport.engine.engine_process_id,
        [int]$reopenReport.engine.engine_process_id
    )
    $liveEngineProcesses = @(
        Get-Process -Name worlddb_ode_engine -ErrorAction SilentlyContinue |
            Where-Object { $engineProcessIds -contains $_.Id }
    )
    if ($Mode -eq 'sidecar' -and $liveEngineProcesses.Count -ne 0) {
        throw 'The sidecar process remained alive after its parent application exited.'
    }

    $summary = [pscustomobject]@{
        mode = $Mode
        two_native_windows = 'PASS'
        first_writer_lock = 'PASS'
        competing_writer_rejected = 'PASS'
        reopen_after_shutdown = 'PASS'
        sidecar_children_reaped = if ($Mode -eq 'sidecar') { 'PASS' } else { 'not_applicable' }
        expected_webview_teardown_diagnostic = 'Chrome_WidgetWin_0 / 1412 may be logged by WebView2 on shutdown'
    }
    $summary | ConvertTo-Json -Compress
    $smokePassed = $true
}
finally {
    foreach ($process in @($first, $second, $reopen)) {
        if ($null -ne $process) {
            $process.Refresh()
            if (-not $process.HasExited) {
                $process.Kill()
                $process.WaitForExit()
            }
        }
    }
    $resolvedTestRoot = [System.IO.Path]::GetFullPath($testRoot)
    $tempPrefix = $tempBase.TrimEnd([System.IO.Path]::DirectorySeparatorChar) + [System.IO.Path]::DirectorySeparatorChar
    if (-not $resolvedTestRoot.StartsWith($tempPrefix, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw 'Refusing to remove a test directory outside the system temp directory.'
    }
    if ($KeepArtifacts -and -not $smokePassed) {
        $retainedRoot = $resolvedTestRoot
        if ($ArtifactsRoot) {
            $destination = [System.IO.Path]::GetFullPath($ArtifactsRoot)
            if ($destination.StartsWith($resolvedTestRoot + [System.IO.Path]::DirectorySeparatorChar, [System.StringComparison]::OrdinalIgnoreCase) -or (Test-Path -LiteralPath $destination)) {
                throw 'The artifact destination must be new and outside the temporary test directory.'
            }
            $destinationParent = [System.IO.Path]::GetDirectoryName($destination)
            $null = New-Item -ItemType Directory -Path $destinationParent -Force
            $null = New-Item -ItemType Directory -Path $destination
            Get-ChildItem -LiteralPath $resolvedTestRoot -Force | Copy-Item -Destination $destination -Recurse
            Remove-Item -LiteralPath $resolvedTestRoot -Recurse -Force
            $retainedRoot = $destination
        }
        Write-Warning "Preserved failed writer-lock artifacts at $retainedRoot"
    } elseif (Test-Path -LiteralPath $resolvedTestRoot) {
        Remove-Item -LiteralPath $resolvedTestRoot -Recurse -Force
    }
    Remove-Item Env:\WORLDDB_ODE_DATABASE -ErrorAction SilentlyContinue
    Remove-Item Env:\WORLDDB_ODE_RESULT -ErrorAction SilentlyContinue
    Remove-Item Env:\WORLDDB_ODE_AUTOCLOSE_MS -ErrorAction SilentlyContinue
}
