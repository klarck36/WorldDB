import CoreGraphics
import Foundation

guard CommandLine.arguments.count == 2 else {
    fputs("Expected one allowlisted keyboard action.\n", stderr)
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

let keyCode: CGKeyCode
let flags: CGEventFlags
switch CommandLine.arguments[1] {
case "tab":
    keyCode = 48
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
    event.post(tap: .cghidEventTap)
    Thread.sleep(forTimeInterval: 0.05)
}
