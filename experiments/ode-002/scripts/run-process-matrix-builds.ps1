param(
    [switch]$KeepArtifacts
)

$ErrorActionPreference = 'Stop'
$workspaceRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$cargoCommand = Get-Command cargo.exe -ErrorAction SilentlyContinue
$cargoPath = if ($null -ne $cargoCommand) {
    $cargoCommand.Source
} else {
    Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe'
}
if (-not (Test-Path -LiteralPath $cargoPath -PathType Leaf)) { throw 'Rust Cargo was not found.' }

$priorEnvironment = @{}
$environmentNames = @(
    'WORLDDB_ODE_APP_BUILD_ID', 'WORLDDB_ODE_ENGINE_BUILD_ID', 'WORLDDB_ODE_DATABASE',
    'WORLDDB_ODE_RESULT', 'WORLDDB_ODE_AUTOCLOSE_MS', 'WORLDDB_ODE_STREAM_TEST',
    'WORLDDB_ODE_PANIC_TEST', 'WORLDDB_ODE_UPDATE_TEST', 'WORLDDB_ODE_ENGINE_EXECUTABLE',
    'WORLDDB_ODE_ENGINE_UPDATE'
)
foreach ($name in $environmentNames) {
    $existing = Get-Item "Env:\$name" -ErrorAction SilentlyContinue
    $priorEnvironment[$name] = if ($null -eq $existing) { $null } else { $existing.Value }
}

$tempBase = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
$stageRoot = Join-Path $tempBase ('worlddb-ode002-matrix-builds-' + [guid]::NewGuid().ToString('N'))
$null = New-Item -ItemType Directory -Path $stageRoot
$targetRoot = Join-Path $workspaceRoot 'target\debug'

function Invoke-CargoBuild([string[]]$Arguments, [string]$Label) {
    & $cargoPath @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Label failed with Cargo exit code $LASTEXITCODE." }
}

try {
    $env:WORLDDB_ODE_APP_BUILD_ID = 'ode2-inprocess-v1'
    Remove-Item Env:\WORLDDB_ODE_ENGINE_BUILD_ID -ErrorAction SilentlyContinue
    Invoke-CargoBuild @('build', '--locked', '--offline', '--workspace') 'In-process v1 build'
    Copy-Item -LiteralPath (Join-Path $targetRoot 'worlddb-ode-desktop-shell.exe') -Destination (Join-Path $stageRoot 'in-process-v1.exe')

    $env:WORLDDB_ODE_APP_BUILD_ID = 'ode2-inprocess-v2'
    Invoke-CargoBuild @('build', '--locked', '--offline', '--workspace') 'In-process v2 build'
    Copy-Item -LiteralPath (Join-Path $targetRoot 'worlddb-ode-desktop-shell.exe') -Destination (Join-Path $stageRoot 'in-process-v2.exe')

    $env:WORLDDB_ODE_APP_BUILD_ID = 'ode2-sidecar'
    $env:WORLDDB_ODE_ENGINE_BUILD_ID = 'ode2-engine-v1'
    Invoke-CargoBuild @('build', '--locked', '--offline', '--workspace', '--no-default-features', '--features', 'sidecar') 'Sidecar v1 build'
    Copy-Item -LiteralPath (Join-Path $targetRoot 'worlddb-ode-desktop-shell.exe') -Destination (Join-Path $stageRoot 'sidecar.exe')
    Copy-Item -LiteralPath (Join-Path $targetRoot 'worlddb_ode_engine.exe') -Destination (Join-Path $stageRoot 'engine-v1.exe')

    $env:WORLDDB_ODE_ENGINE_BUILD_ID = 'ode2-engine-v2'
    Invoke-CargoBuild @('build', '--locked', '--offline', '--workspace', '--no-default-features', '--features', 'sidecar') 'Sidecar v2 build'
    Copy-Item -LiteralPath (Join-Path $targetRoot 'worlddb_ode_engine.exe') -Destination (Join-Path $stageRoot 'engine-v2.exe')

    & (Join-Path $PSScriptRoot 'run-process-matrix.ps1') `
        -InProcessExecutable (Join-Path $stageRoot 'in-process-v1.exe') `
        -UpdatedInProcessExecutable (Join-Path $stageRoot 'in-process-v2.exe') `
        -SidecarExecutable (Join-Path $stageRoot 'sidecar.exe') `
        -EngineExecutable (Join-Path $stageRoot 'engine-v1.exe') `
        -UpdatedEngineExecutable (Join-Path $stageRoot 'engine-v2.exe')
    if ($LASTEXITCODE -ne 0) { throw 'The ODE-002 process matrix failed.' }
}
finally {
    foreach ($name in $environmentNames) {
        if ($null -eq $priorEnvironment[$name]) {
            Remove-Item "Env:\$name" -ErrorAction SilentlyContinue
        } else {
            Set-Item "Env:\$name" -Value $priorEnvironment[$name]
        }
    }
    $resolvedStageRoot = [System.IO.Path]::GetFullPath($stageRoot)
    $tempPrefix = $tempBase.TrimEnd([System.IO.Path]::DirectorySeparatorChar) + [System.IO.Path]::DirectorySeparatorChar
    if (-not $resolvedStageRoot.StartsWith($tempPrefix, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw 'Refusing to remove staged build artifacts outside the system temp directory.'
    }
    if (-not $KeepArtifacts -and (Test-Path -LiteralPath $resolvedStageRoot)) {
        Remove-Item -LiteralPath $resolvedStageRoot -Recurse -Force
    }
    if ($KeepArtifacts) { Write-Output "Staged matrix executables: $resolvedStageRoot" }
}
