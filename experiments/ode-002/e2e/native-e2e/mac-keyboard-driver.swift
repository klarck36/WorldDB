import AppKit
import CoreGraphics
import Foundation

guard CommandLine.arguments.count == 3,
      let rawProcessID = Int32(CommandLine.arguments[1]),
      rawProcessID > 0 else {
    fputs("Expected a target process ID and one allowlisted keyboard action.\n", stderr)
    exit(2)
}

guard CGPreflightPostEventAccess() else {
    fputs("The macOS runner has not granted permission to post Quartz keyboard events.\n", stderr)
    exit(3)
}

guard let source = CGEventSource(stateID: .hidSystemState) else {
    fputs("Could not create a Quartz keyboard event source.\n", stderr)
    exit(4)
}

let targetProcessID = pid_t(rawProcessID)
guard let targetApplication = NSRunningApplication(processIdentifier: targetProcessID) else {
    fputs("The target desktop application is no longer running.\n", stderr)
    exit(5)
}
if !targetApplication.isActive {
    _ = targetApplication.activate(options: [.activateAllWindows])
}
let activationDeadline = Date(timeIntervalSinceNow: 3)
while Date() < activationDeadline {
    if targetApplication.isActive,
       NSWorkspace.shared.frontmostApplication?.processIdentifier == targetProcessID {
        break
    }
    _ = RunLoop.current.run(mode: .default, before: Date(timeIntervalSinceNow: 0.1))
}
guard targetApplication.isActive,
      NSWorkspace.shared.frontmostApplication?.processIdentifier == targetProcessID else {
    fputs("Could not make the target desktop application frontmost for native keyboard input.\n", stderr)
    exit(6)
}

let keyCode: CGKeyCode
let flags: CGEventFlags
switch CommandLine.arguments[2] {
case "tab":
    keyCode = 48
    flags = []
case "enter":
    keyCode = 36
    flags = []
case "control-tab":
    keyCode = 48
    flags = .maskControl
case "control-f7":
    keyCode = 98
    flags = .maskControl
case "fn-control-f7":
    keyCode = 98
    flags = [.maskControl, .maskSecondaryFn]
default:
    fputs("Unsupported keyboard action.\n", stderr)
    exit(2)
}

for isDown in [true, false] {
    guard let event = CGEvent(keyboardEventSource: source, virtualKey: keyCode, keyDown: isDown) else {
        fputs("Could not create a Quartz keyboard event.\n", stderr)
        exit(5)
    }
    event.flags = flags
    // Native focus traversal is handled by AppKit and WebKit only when the
    // activated application receives the event through the normal event stream.
    event.post(tap: .cghidEventTap)
    Thread.sleep(forTimeInterval: 0.05)
}
