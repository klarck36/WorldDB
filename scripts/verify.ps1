[CmdletBinding()]
param(
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]] $VerifyArgs
)

$ErrorActionPreference = 'Stop'
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'

if (Test-Path -LiteralPath $cargoBin) {
    $env:PATH = "$cargoBin;$env:PATH"
}
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    throw 'Cargo was not found. Install the toolchain in rust-toolchain.toml and add cargo to PATH.'
}

if (-not $env:WORLDDB_VERIFY_PROFILE) { $env:WORLDDB_VERIFY_PROFILE = 'dev' }
if (-not $env:WORLDDB_PYTHON) { $env:WORLDDB_PYTHON = 'python' }
if (-not $env:CARGO_TARGET_DIR) {
    $targetBase = if ($env:LOCALAPPDATA) { $env:LOCALAPPDATA } else { $env:TEMP }
    $env:CARGO_TARGET_DIR = Join-Path $targetBase 'WorldDB\verify-target'
}

Push-Location $repoRoot
try {
    if ($VerifyArgs) {
        & cargo xtask verify @VerifyArgs
    } else {
        & cargo xtask verify
    }
    $verifyExitCode = $LASTEXITCODE
} finally {
    Pop-Location
}
exit $verifyExitCode
