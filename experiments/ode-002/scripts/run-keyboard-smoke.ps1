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
if (-not ('WorldDbKeyboardSmokeNative' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class WorldDbKeyboardSmokeNative {
    [StructLayout(LayoutKind.Sequential)]
    public struct NativeRect { public int Left; public int Top; public int Right; public int Bottom; }
    [StructLayout(LayoutKind.Sequential)]
    public struct NativePoint { public int X; public int Y; }
    [DllImport("user32.dll", SetLastError = true)]
    public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll", SetLastError = true)]
    public static extern uint GetWindowThreadProcessId(IntPtr windowHandle, out uint processId);
    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool AttachThreadInput(uint attachThreadId, uint attachToThreadId, bool attach);
    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool BringWindowToTop(IntPtr windowHandle);
    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool SetForegroundWindow(IntPtr windowHandle);
    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool ShowWindow(IntPtr windowHandle, int command);
    [DllImport("user32.dll", SetLastError = true)]
    public static extern IntPtr SetActiveWindow(IntPtr windowHandle);
    [DllImport("user32.dll", SetLastError = true)]
    public static extern IntPtr SetFocus(IntPtr windowHandle);
    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool GetWindowRect(IntPtr windowHandle, out NativeRect rect);
    [DllImport("user32.dll", SetLastError = true)]
    public static extern IntPtr WindowFromPoint(NativePoint point);
    [DllImport("user32.dll", SetLastError = true)]
    public static extern IntPtr GetAncestor(IntPtr windowHandle, uint flags);
    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll", SetLastError = true)]
    public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extraInfo);
    [DllImport("user32.dll", EntryPoint = "GetWindowLongPtrW", SetLastError = true)]
    public static extern IntPtr GetWindowLongPtr(IntPtr windowHandle, int index);
    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    public static extern short VkKeyScan(char character);
    [DllImport("user32.dll", SetLastError = true)]
    public static extern void keybd_event(byte virtualKey, byte scanCode, uint flags, UIntPtr extraInfo);
    [DllImport("kernel32.dll")]
    public static extern uint GetCurrentThreadId();
}
'@
}

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
foreach ($name in @('WORLDDB_ODE_DATABASE', 'WORLDDB_ODE_PROJECT_SMOKE_ROOT', 'WORLDDB_ODE_SHOW_WINDOWS', 'WORLDDB_ODE_AUTOCLOSE_MS', 'WORLDDB_M8_26_CRASH_AFTER_WAL_COMMIT_SYNC', 'WORLDDB_M8_26_CRASH_SIGNAL_PATH', 'WEBVIEW2_USER_DATA_FOLDER')) {
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

function Test-TestProcessId([int]$CandidateProcessId) {
    if ($CandidateProcessId -eq $process.Id) { return $true }
    $processTree = @(Get-CimInstance -ClassName Win32_Process -Property ProcessId, ParentProcessId)
    $trustedIds = [System.Collections.Generic.HashSet[int]]::new()
    $null = $trustedIds.Add([int]$process.Id)
    $changed = $true
    while ($changed) {
        $changed = $false
        foreach ($entry in $processTree) {
            if ($trustedIds.Contains([int]$entry.ParentProcessId) -and $trustedIds.Add([int]$entry.ProcessId)) {
                $changed = $true
            }
        }
    }
    return $trustedIds.Contains($CandidateProcessId)
}

function Assert-KeyboardTarget(
    [System.Windows.Automation.AutomationElement]$ExpectedElement,
    [System.Windows.Automation.AutomationElement]$WindowElement,
    [string]$ExpectedName,
    [System.Windows.Automation.ControlType]$ExpectedControlType
) {
    $focused = [System.Windows.Automation.AutomationElement]::FocusedElement
    if ($focused.Current.ProcessId -ne $ExpectedElement.Current.ProcessId -or
        $focused.Current.ControlType -ne $ExpectedControlType -or
        $focused.Current.Name -ne $ExpectedName) {
        throw "Keyboard focus is '$($focused.Current.Name)' ($($focused.Current.ControlType.ProgrammaticName)), not '$ExpectedName'."
    }
    $foregroundHandle = [WorldDbKeyboardSmokeNative]::GetForegroundWindow()
    [uint32]$foregroundProcessId = 0
    $null = [WorldDbKeyboardSmokeNative]::GetWindowThreadProcessId($foregroundHandle, [ref]$foregroundProcessId)
    if ($foregroundProcessId -ne $WindowElement.Current.ProcessId) {
        throw 'The WorldDB native window is not the foreground keyboard target; no further keys were sent.'
    }
    return $focused
}

function Activate-TestWindow([System.Windows.Automation.AutomationElement]$WindowElement) {
    $windowHandle = [IntPtr]$WindowElement.Current.NativeWindowHandle
    $foregroundHandle = [WorldDbKeyboardSmokeNative]::GetForegroundWindow()
    [uint32]$foregroundProcessId = 0
    $foregroundThreadId = [WorldDbKeyboardSmokeNative]::GetWindowThreadProcessId($foregroundHandle, [ref]$foregroundProcessId)
    [uint32]$windowProcessId = 0
    $windowThreadId = [WorldDbKeyboardSmokeNative]::GetWindowThreadProcessId($windowHandle, [ref]$windowProcessId)
    $callingThreadId = [WorldDbKeyboardSmokeNative]::GetCurrentThreadId()
    $attachments = [System.Collections.Generic.List[object]]::new()
    foreach ($threadId in @($foregroundThreadId, $windowThreadId)) {
        if ($threadId -ne 0 -and $threadId -ne $callingThreadId -and
            -not @($attachments | Where-Object { $_.thread_id -eq $threadId }).Count) {
            $didAttach = [WorldDbKeyboardSmokeNative]::AttachThreadInput($callingThreadId, $threadId, $true)
            if ($didAttach) { $attachments.Add([pscustomobject]@{ thread_id = $threadId }) }
        }
    }
    try {
        $null = [WorldDbKeyboardSmokeNative]::ShowWindow($windowHandle, 5)
        $null = [WorldDbKeyboardSmokeNative]::BringWindowToTop($windowHandle)
        $style = [WorldDbKeyboardSmokeNative]::GetWindowLongPtr($windowHandle, -16).ToInt64()
        if (($style -band 0x00C00000) -ne 0x00C00000) {
            throw 'The WorldDB test window has no safe title-bar activation point; no mouse or keyboard input was sent.'
        }
        $rect = [WorldDbKeyboardSmokeNative+NativeRect]::new()
        if (-not [WorldDbKeyboardSmokeNative]::GetWindowRect($windowHandle, [ref]$rect)) {
            throw 'Windows could not report the WorldDB test window bounds; no input was sent.'
        }
        $point = [WorldDbKeyboardSmokeNative+NativePoint]::new()
        $point.X = $rect.Left + 60
        $point.Y = $rect.Top + 10
        $hitHandle = [WorldDbKeyboardSmokeNative]::WindowFromPoint($point)
        $hitRoot = [WorldDbKeyboardSmokeNative]::GetAncestor($hitHandle, 2)
        if ($hitRoot -ne $windowHandle) {
            throw 'The visible title-bar point does not belong to the WorldDB test window; no mouse or keyboard input was sent.'
        }
        if (-not [WorldDbKeyboardSmokeNative]::SetCursorPos($point.X, $point.Y)) {
            throw 'Windows could not position the cursor over the verified WorldDB title bar; no keyboard input was sent.'
        }
        [WorldDbKeyboardSmokeNative]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
        [WorldDbKeyboardSmokeNative]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
        $null = [WorldDbKeyboardSmokeNative]::SetForegroundWindow($windowHandle)
        $null = [WorldDbKeyboardSmokeNative]::SetActiveWindow($windowHandle)
        $null = [WorldDbKeyboardSmokeNative]::SetFocus($windowHandle)
    } finally {
        foreach ($attachment in $attachments) {
            $null = [WorldDbKeyboardSmokeNative]::AttachThreadInput($callingThreadId, [uint32]$attachment.thread_id, $false)
        }
    }
    Start-Sleep -Milliseconds 150
    $currentForeground = [WorldDbKeyboardSmokeNative]::GetForegroundWindow()
    [uint32]$currentForegroundProcessId = 0
    $null = [WorldDbKeyboardSmokeNative]::GetWindowThreadProcessId($currentForeground, [ref]$currentForegroundProcessId)
    if ($currentForegroundProcessId -ne $WindowElement.Current.ProcessId) {
        throw 'Windows did not activate the WorldDB test window; no keyboard input was sent.'
    }
}

function Ensure-TestElementVisible([System.Windows.Automation.AutomationElement]$TargetElement) {
    if (-not (Test-TestProcessId $TargetElement.Current.ProcessId)) {
        throw 'The target control is outside the launched WorldDB process tree.'
    }
    if ($TargetElement.Current.IsOffscreen) {
        try {
            $scrollPattern = $TargetElement.GetCurrentPattern([System.Windows.Automation.ScrollItemPattern]::Pattern)
            $scrollPattern.ScrollIntoView()
        } catch {
            throw 'The target control is offscreen and cannot be scrolled into view.'
        }
        Start-Sleep -Milliseconds 150
    }
    if ($TargetElement.Current.IsOffscreen) {
        throw 'The target control remained offscreen.'
    }
}

function Click-TestElement(
    [System.Windows.Automation.AutomationElement]$TargetElement,
    [System.Windows.Automation.AutomationElement]$WindowElement
) {
    Ensure-TestElementVisible $TargetElement
    $clickablePoint = $null
    try {
        $clickablePoint = $TargetElement.GetClickablePoint()
    } catch {
        throw 'The target control has no accessibility-confirmed click point; no input was sent.'
    }
    $point = [WorldDbKeyboardSmokeNative+NativePoint]::new()
    $point.X = [int][Math]::Round($clickablePoint.X)
    $point.Y = [int][Math]::Round($clickablePoint.Y)
    $hitHandle = [WorldDbKeyboardSmokeNative]::WindowFromPoint($point)
    $hitRoot = [WorldDbKeyboardSmokeNative]::GetAncestor($hitHandle, 2)
    if ($hitRoot -ne [IntPtr]$WindowElement.Current.NativeWindowHandle) {
        throw 'The visible control point is occluded or outside the WorldDB window; no input was sent.'
    }
    if (-not [WorldDbKeyboardSmokeNative]::SetCursorPos($point.X, $point.Y)) {
        throw 'Windows could not position the cursor over the verified WorldDB control; no input was sent.'
    }
    [WorldDbKeyboardSmokeNative]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
    [WorldDbKeyboardSmokeNative]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 100
}

function Send-NativeCharacter([char]$Character) {
    $mapping = [WorldDbKeyboardSmokeNative]::VkKeyScan($Character)
    if ($mapping -eq -1) {
        throw "The current Windows keyboard layout cannot type '$Character'."
    }
    $virtualKey = [byte]($mapping -band 0x00FF)
    $modifiers = [byte](($mapping -shr 8) -band 0x00FF)
    if (($modifiers -band 0x01) -ne 0) {
        [WorldDbKeyboardSmokeNative]::keybd_event(0x10, 0, 0, [UIntPtr]::Zero)
    }
    [WorldDbKeyboardSmokeNative]::keybd_event($virtualKey, 0, 0, [UIntPtr]::Zero)
    [WorldDbKeyboardSmokeNative]::keybd_event($virtualKey, 0, 0x0002, [UIntPtr]::Zero)
    if (($modifiers -band 0x01) -ne 0) {
        [WorldDbKeyboardSmokeNative]::keybd_event(0x10, 0, 0x0002, [UIntPtr]::Zero)
    }
}

function Send-NativeKey([byte]$VirtualKey) {
    [WorldDbKeyboardSmokeNative]::keybd_event($VirtualKey, 0, 0, [UIntPtr]::Zero)
    [WorldDbKeyboardSmokeNative]::keybd_event($VirtualKey, 0, 0x0002, [UIntPtr]::Zero)
}

function Send-NativeChord([byte]$ModifierVirtualKey, [byte]$VirtualKey) {
    [WorldDbKeyboardSmokeNative]::keybd_event($ModifierVirtualKey, 0, 0, [UIntPtr]::Zero)
    Send-NativeKey $VirtualKey
    [WorldDbKeyboardSmokeNative]::keybd_event($ModifierVirtualKey, 0, 0x0002, [UIntPtr]::Zero)
}

try {
    $env:WORLDDB_ODE_DATABASE = $databaseRoot
    $env:WORLDDB_ODE_PROJECT_SMOKE_ROOT = Join-Path $testRoot 'project'
    $env:WORLDDB_ODE_SHOW_WINDOWS = '1'
    $env:WEBVIEW2_USER_DATA_FOLDER = Join-Path $testRoot 'webview-profile'
    $null = New-Item -ItemType Directory -Path $env:WEBVIEW2_USER_DATA_FOLDER -Force
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
    if ($CrashDuringCommit) {
        if (-not (Test-TestProcessId $nameInput.Current.ProcessId) -or -not (Test-TestProcessId $createButton.Current.ProcessId)) {
            throw 'The crash-recovery controls did not belong to the launched WorldDB process.'
        }
        Activate-TestWindow $window
        Click-TestElement $nameInput $window
        $nameInput.SetFocus()
        Start-Sleep -Milliseconds 150
        $null = Assert-KeyboardTarget $nameInput $window 'Neuer Projektname' ([System.Windows.Automation.ControlType]::Edit)
        Ensure-TestElementVisible $createButton
        $null = Assert-KeyboardTarget $nameInput $window 'Neuer Projektname' ([System.Windows.Automation.ControlType]::Edit)
        if (-not $createButton.Current.IsEnabled) {
            throw 'The crash-recovery create action was not enabled.'
        }
        Click-TestElement $createButton $window
    } else {
        Activate-TestWindow $window
        Click-TestElement $nameInput $window
        $nameInput.SetFocus()
        Start-Sleep -Milliseconds 150
        $null = Assert-KeyboardTarget $nameInput $window 'Neuer Projektname' ([System.Windows.Automation.ControlType]::Edit)
        Send-NativeChord 0x11 0x41
        $null = Assert-KeyboardTarget $nameInput $window 'Neuer Projektname' ([System.Windows.Automation.ControlType]::Edit)
        Send-NativeKey 0x08
        Start-Sleep -Milliseconds 150
        $valuePattern = $nameInput.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern)
        if ($valuePattern.Current.Value -ne '') {
            throw 'Keyboard selection clearing did not empty the project-name input.'
        }
        Click-TestElement $nameInput $window
        $null = Assert-KeyboardTarget $nameInput $window 'Neuer Projektname' ([System.Windows.Automation.ControlType]::Edit)
        Send-NativeKey 0x23
        $typingTrace = [System.Collections.Generic.List[string]]::new()
        foreach ($character in 'KeyboardSuiteProject'.ToCharArray()) {
            $null = Assert-KeyboardTarget $nameInput $window 'Neuer Projektname' ([System.Windows.Automation.ControlType]::Edit)
            Send-NativeCharacter $character
            Start-Sleep -Milliseconds 75
            $typingTrace.Add("$character=$($valuePattern.Current.Value)")
        }
        Start-Sleep -Milliseconds 250
        $enteredName = $valuePattern.Current.Value
        if ($enteredName -ne 'KeyboardSuiteProject') {
            throw "Keyboard text entry did not reach the project-name input (observed '$enteredName'; trace: $($typingTrace -join ', '))."
        }

        $null = Assert-KeyboardTarget $nameInput $window 'Neuer Projektname' ([System.Windows.Automation.ControlType]::Edit)
        Ensure-TestElementVisible $createButton
        $null = Assert-KeyboardTarget $nameInput $window 'Neuer Projektname' ([System.Windows.Automation.ControlType]::Edit)
        Send-NativeKey 0x09
        Start-Sleep -Milliseconds 150
        $focused = [System.Windows.Automation.AutomationElement]::FocusedElement
        if ($focused.Current.ProcessId -ne $createButton.Current.ProcessId -or $focused.Current.ControlType -ne [System.Windows.Automation.ControlType]::Button -or $focused.Current.Name -ne 'Neues Projekt') {
            throw "Tab navigation focused '$($focused.Current.Name)' ($($focused.Current.ControlType.ProgrammaticName)), not Neues Projekt."
        }
        $null = Assert-KeyboardTarget $createButton $window 'Neues Projekt' ([System.Windows.Automation.ControlType]::Button)
        Send-NativeKey 0x0D
    }
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
            create_action_activated_by_native_click = 'PASS'
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
        $createdNameStatus = Find-Element $window ([System.Windows.Automation.ControlType]::Text) 'KeyboardSuiteProject'
        if ($null -eq $createdNameStatus) {
            $visibleText = @(Find-Element $window ([System.Windows.Automation.ControlType]::Text) '' | ForEach-Object { $_.Current.Name } | Where-Object { $_ }) -join ' | '
            throw "The created project did not expose the expected exact name (input value pattern observed '$enteredName'; visible text: $visibleText)."
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
