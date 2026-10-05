param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('in-process', 'sidecar')]
    [string]$Mode,
    [Parameter(Mandatory = $true)]
    [string]$ExecutablePath,
    [switch]$CrashDuringCommit,
    [string]$RecoveryCliPath,
    [switch]$KeepArtifacts,
    [string]$ArtifactsRoot,
    [ValidateRange(15, 120)]
    [int]$TimeoutSeconds = 60
)

$ErrorActionPreference = 'Stop'
if ($ArtifactsRoot -and -not $KeepArtifacts) {
    throw 'ArtifactsRoot requires KeepArtifacts.'
}
if ($CrashDuringCommit -and -not $RecoveryCliPath) {
    throw 'CrashDuringCommit requires RecoveryCliPath for read-only recovery verification.'
}
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type -AssemblyName System.Windows.Forms

$executable = [System.IO.Path]::GetFullPath($ExecutablePath)
if (-not (Test-Path -LiteralPath $executable -PathType Leaf)) {
    throw 'Build the selected ODE-002 mode before running this keyboard smoke check.'
}
$testRoot = Join-Path ([System.IO.Path]::GetTempPath()) ('worlddb-ode002-keyboard-' + [guid]::NewGuid().ToString('N'))
$null = New-Item -ItemType Directory -Path $testRoot
$databaseRoot = Join-Path $testRoot 'database'
$stdoutPath = Join-Path $testRoot 'desktop.stdout.log'
$stderrPath = Join-Path $testRoot 'desktop.stderr.log'
$resultPath = Join-Path $testRoot 'keyboard-result.json'
$crashSignalPath = Join-Path $testRoot 'crash-signal.txt'
$recoveryLogPath = Join-Path $testRoot 'recovery-inspect.jsonl'
$recoveryCli = if ($RecoveryCliPath) { [System.IO.Path]::GetFullPath($RecoveryCliPath) } else { $null }
if ($CrashDuringCommit -and -not (Test-Path -LiteralPath $recoveryCli -PathType Leaf)) {
    throw 'The recovery CLI executable does not exist.'
}
$savedEnvironment = @{}
foreach ($name in @('WORLDDB_ODE_DATABASE', 'WORLDDB_ODE_SHOW_WINDOWS', 'WORLDDB_ODE_AUTOCLOSE_MS', 'WORLDDB_M8_26_CRASH_AFTER_WAL_COMMIT_SYNC', 'WORLDDB_M8_26_CRASH_SIGNAL_PATH')) {
    $existing = Get-Item "Env:\$name" -ErrorAction SilentlyContinue
    $savedEnvironment[$name] = if ($null -eq $existing) { $null } else { $existing.Value }
}
$process = $null
$passed = $false

function Find-Element(
    [System.Windows.Automation.AutomationElement]$Root,
    [System.Windows.Automation.ControlType]$ControlType,
    [string]$Name
) {
    $conditions = [System.Collections.Generic.List[System.Windows.Automation.Condition]]::new()
    $conditions.Add([System.Windows.Automation.PropertyCondition]::new(
        [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
        $ControlType
    ))
    if ($Name) {
        $conditions.Add([System.Windows.Automation.PropertyCondition]::new(
            [System.Windows.Automation.AutomationElement]::NameProperty,
            $Name
        ))
    }
    $condition = [System.Windows.Automation.AndCondition]::new($conditions.ToArray())
    return $Root.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $condition)
}

function Wait-ForElement(
    [System.Windows.Automation.AutomationElement]$Root,
    [System.Windows.Automation.ControlType]$ControlType,
    [string]$Name,
    [int]$Timeout
) {
    $deadline = [DateTime]::UtcNow.AddSeconds($Timeout)
    while ([DateTime]::UtcNow -lt $deadline) {
        $element = Find-Element $Root $ControlType $Name
        if ($null -ne $element) { return $element }
        $process.Refresh()
        if ($process.HasExited) { throw "Desktop exited before UIA element '$Name' appeared (exit $($process.ExitCode))." }
        Start-Sleep -Milliseconds 150
    }
    throw "Timed out waiting for UIA element '$Name'."
}

try {
    $env:WORLDDB_ODE_DATABASE = $databaseRoot
    $env:WORLDDB_ODE_SHOW_WINDOWS = '1'
    Remove-Item Env:\WORLDDB_ODE_AUTOCLOSE_MS -ErrorAction SilentlyContinue
    if ($CrashDuringCommit) {
        $env:WORLDDB_M8_26_CRASH_AFTER_WAL_COMMIT_SYNC = '1'
        $env:WORLDDB_M8_26_CRASH_SIGNAL_PATH = $crashSignalPath
    } else {
        Remove-Item Env:\WORLDDB_M8_26_CRASH_AFTER_WAL_COMMIT_SYNC -ErrorAction SilentlyContinue
        Remove-Item Env:\WORLDDB_M8_26_CRASH_SIGNAL_PATH -ErrorAction SilentlyContinue
    }
    $process = Start-Process -FilePath $executable -PassThru -WindowStyle Normal `
        -RedirectStandardOutput $stdoutPath -RedirectStandardError $stderrPath
    $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    $window = $null
    while ([DateTime]::UtcNow -lt $deadline) {
        $process.Refresh()
        if ($process.HasExited) {
            $stderr = Get-Content -LiteralPath $stderrPath -Raw -ErrorAction SilentlyContinue
            throw "Desktop exited before a visible primary window appeared (exit $($process.ExitCode)). $stderr"
        }
        if ($process.MainWindowHandle -ne [IntPtr]::Zero) {
            $window = [System.Windows.Automation.AutomationElement]::FromHandle($process.MainWindowHandle)
            $createButton = Find-Element $window ([System.Windows.Automation.ControlType]::Button) 'Neues Projekt'
            if ($null -ne $createButton -and $createButton.Current.IsEnabled) { break }
        }
        Start-Sleep -Milliseconds 150
    }
    if ($null -eq $window -or $null -eq $createButton -or -not $createButton.Current.IsEnabled) {
        throw 'The visible primary window did not expose an enabled Neues Projekt button.'
    }

    $nameInput = Find-Element $window ([System.Windows.Automation.ControlType]::Edit) 'Neuer Projektname'
    if ($null -eq $nameInput) {
        $nameInput = Find-Element $window ([System.Windows.Automation.ControlType]::Edit) ''
    }
    if ($null -eq $nameInput) {
        throw 'The project-name input was not present in the native accessibility tree.'
    }
    $window.SetFocus()
    $nameInput.SetFocus()
    [System.Windows.Forms.SendKeys]::SendWait('^a')
    [System.Windows.Forms.SendKeys]::SendWait('KeyboardSuiteProject')
    Start-Sleep -Milliseconds 150
    $valuePattern = $nameInput.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern)
    $enteredName = $valuePattern.Current.Value
    if ($enteredName -ne 'KeyboardSuiteProject') {
        throw "Keyboard text entry did not reach the project-name input (observed '$enteredName')."
    }

    [System.Windows.Forms.SendKeys]::SendWait('{TAB}')
    Start-Sleep -Milliseconds 150
    $focused = [System.Windows.Automation.AutomationElement]::FocusedElement
    if ($focused.Current.ProcessId -ne $process.Id -or $focused.Current.ControlType -ne [System.Windows.Automation.ControlType]::Button -or $focused.Current.Name -ne 'Neues Projekt') {
        throw "Tab navigation focused '$($focused.Current.Name)' ($($focused.Current.ControlType.ProgrammaticName)), not Neues Projekt."
    }
    [System.Windows.Forms.SendKeys]::SendWait('{ENTER}')
    if ($CrashDuringCommit) {
        $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
        while (-not (Test-Path -LiteralPath $crashSignalPath -PathType Leaf) -and [DateTime]::UtcNow -lt $deadline) {
            $process.Refresh()
            if ($Mode -eq 'in-process' -and $process.HasExited) { break }
            Start-Sleep -Milliseconds 100
        }
        if (-not (Test-Path -LiteralPath $crashSignalPath -PathType Leaf)) {
            throw 'The process did not reach the durable WAL commit crash point.'
        }
        $signal = Get-Content -LiteralPath $crashSignalPath -Raw
        if ($signal -notmatch 'checkpoint=after_wal_commit_sync' -or $signal -notmatch 'exit_code=86') {
            throw 'The crash signal did not identify the expected durable commit boundary.'
        }
        $crashedProcessId = [int]([regex]::Match($signal, 'process_id=(\d+)').Groups[1].Value)
        if ($Mode -eq 'in-process') {
            if (-not $process.WaitForExit($TimeoutSeconds * 1000)) {
                throw 'The in-process desktop did not exit at the injected commit crash.'
            }
            $process.Refresh()
            if ($process.ExitCode -ne 86 -or $crashedProcessId -ne $process.Id) {
                throw "The in-process desktop exited with $($process.ExitCode), expected crash exit 86."
            }
        } elseif ($crashedProcessId -eq $process.Id) {
            throw 'The sidecar crash signal came from the desktop process instead of the engine process.'
        }
        Remove-Item Env:\WORLDDB_M8_26_CRASH_AFTER_WAL_COMMIT_SYNC -ErrorAction SilentlyContinue
        Remove-Item Env:\WORLDDB_M8_26_CRASH_SIGNAL_PATH -ErrorAction SilentlyContinue
        $process.Refresh()
        if (-not $process.HasExited) {
            $process.Kill()
            $process.WaitForExit()
        }
        $recoveryOutput = @(& $recoveryCli '--format' 'jsonl' 'v1' 'recovery' 'inspect' $databaseRoot 2>&1)
        $recoveryExitCode = $LASTEXITCODE
        $recoveryLines = @($recoveryOutput | ForEach-Object { $_.ToString() })
        $recoveryLines | Set-Content -LiteralPath $recoveryLogPath -Encoding UTF8
        if ($recoveryExitCode -ne 0) {
            throw "Read-only recovery inspection failed after the commit crash (exit $recoveryExitCode)."
        }
        $recoveryJsonLine = $recoveryLines | Where-Object { $_.TrimStart().StartsWith('{') } | Select-Object -Last 1
        if (-not $recoveryJsonLine) {
            throw 'Read-only recovery inspection did not produce JSONL evidence.'
        }
        $recoveryReport = $recoveryJsonLine | ConvertFrom-Json
        $recoveryData = $recoveryReport.outcome.data
        if ($recoveryReport.outcome.type -notin @('recovery_inspect', 'verify') -or $recoveryData.status -ne 'completed' -or -not $recoveryData.safe_revision) {
            throw 'Read-only recovery inspection did not return a completed safe revision.'
        }
        $summary = [pscustomobject]@{
            mode = $Mode
            keyboard_triggered_commit = 'PASS'
            crash_after_durable_wal_commit = 'PASS'
            recovery_read_only_inspection = 'PASS'
            crashed_process_id = $crashedProcessId
            recovered_safe_revision = [string]$recoveryData.safe_revision
        }
    } else {
        $closeButton = Wait-ForElement $window ([System.Windows.Automation.ControlType]::Button) 'Projekt schließen' $TimeoutSeconds
        $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
        while (-not $closeButton.Current.IsEnabled -and [DateTime]::UtcNow -lt $deadline) {
            Start-Sleep -Milliseconds 150
            $closeButton = Find-Element $window ([System.Windows.Automation.ControlType]::Button) 'Projekt schließen'
        }
        if ($null -eq $closeButton -or -not $closeButton.Current.IsEnabled) {
            throw 'Enter did not create the temporary project; Projekt schließen never became enabled.'
        }
        $summary = [pscustomobject]@{
            mode = $Mode
            native_window_accessibility = 'PASS'
            keyboard_text_entry = 'PASS'
            tab_navigation_to_create = 'PASS'
            enter_activated_create = 'PASS'
            created_project_name = $enteredName
        }
    }

    $summary | ConvertTo-Json -Compress | Set-Content -LiteralPath $resultPath -Encoding UTF8
    $summary | ConvertTo-Json -Compress
    $passed = $true
} finally {
    if ($null -ne $process) {
        $process.Refresh()
        if (-not $process.HasExited) {
            $process.Kill()
            $process.WaitForExit()
        }
    }
    foreach ($name in $savedEnvironment.Keys) {
        if ($null -eq $savedEnvironment[$name]) {
            Remove-Item "Env:\$name" -ErrorAction SilentlyContinue
        } else {
            Set-Item "Env:\$name" -Value $savedEnvironment[$name]
        }
    }
    $resolvedRoot = [System.IO.Path]::GetFullPath($testRoot)
    $tempPrefix = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath()).TrimEnd([System.IO.Path]::DirectorySeparatorChar) + [System.IO.Path]::DirectorySeparatorChar
    if (-not $resolvedRoot.StartsWith($tempPrefix, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw 'Refusing to remove a keyboard-smoke directory outside the system temp directory.'
    }
    if ($KeepArtifacts -and -not $passed) {
        $retainedRoot = $resolvedRoot
        if ($ArtifactsRoot) {
            $destination = [System.IO.Path]::GetFullPath($ArtifactsRoot)
            if ($destination.StartsWith($resolvedRoot + [System.IO.Path]::DirectorySeparatorChar, [System.StringComparison]::OrdinalIgnoreCase) -or (Test-Path -LiteralPath $destination)) {
                throw 'The artifact destination must be new and outside the temporary keyboard-smoke directory.'
            }
            $destinationParent = [System.IO.Path]::GetDirectoryName($destination)
            $null = New-Item -ItemType Directory -Path $destinationParent -Force
            $null = New-Item -ItemType Directory -Path $destination
            Get-ChildItem -LiteralPath $resolvedRoot -Force | Copy-Item -Destination $destination -Recurse
            Remove-Item -LiteralPath $resolvedRoot -Recurse -Force
            $retainedRoot = $destination
        }
        Write-Warning "Preserved failed keyboard-smoke artifacts at $retainedRoot"
    } elseif (Test-Path -LiteralPath $resolvedRoot) {
        Remove-Item -LiteralPath $resolvedRoot -Recurse -Force
    }
}
