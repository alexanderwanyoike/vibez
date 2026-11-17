// Drags files into Vibez the way Finder does: an NSDraggingSession begun
// from a window in this process, moved by synthetic mouse input.
//
// Usage: drag_macos <vibez pid> <screenshot folder> <interface scale> <step>...
// where each step is four arguments, <file or "click"> <x> <y> <name>, with
// x and y in Vibez's points from the top left of its content.

import AppKit

/// The height of a standard window title bar, above the content.
let titleBarHeight: CGFloat = 28

final class SourceView: NSView, NSDraggingSource {
    var file: URL?
    let label = NSTextField(labelWithString: "")

    override init(frame: NSRect) {
        super.init(frame: frame)
        label.frame = bounds.insetBy(dx: 8, dy: 40)
        label.alignment = .center
        addSubview(label)
    }

    required init?(coder: NSCoder) { nil }

    // The first click on an inactive window must start the drag too.
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }

    override func hitTest(_ point: NSPoint) -> NSView? {
        frame.contains(point) ? self : nil
    }

    override func mouseDown(with event: NSEvent) {
        guard let file else { return }
        let item = NSDraggingItem(pasteboardWriter: file as NSURL)
        item.setDraggingFrame(bounds.insetBy(dx: 60, dy: 30), contents: nil)
        beginDraggingSession(with: [item], event: event, source: self)
    }

    func draggingSession(
        _ session: NSDraggingSession,
        sourceOperationMaskFor context: NSDraggingContext
    ) -> NSDragOperation {
        .copy
    }

    func draggingSession(
        _ session: NSDraggingSession, endedAt screenPoint: NSPoint, operation: NSDragOperation
    ) {
        dropped.signal()
    }
}

let dropped = DispatchSemaphore(value: 0)
let arguments = CommandLine.arguments
let vibezPid = pid_t(arguments[1])!
let shots = URL(fileURLWithPath: arguments[2])
let interfaceScale = CGFloat(Double(arguments[3])!)
try FileManager.default.createDirectory(at: shots, withIntermediateDirectories: true)

let app = NSApplication.shared
app.setActivationPolicy(.regular)
let screen = NSScreen.main!.frame
let window = NSWindow(
    contentRect: NSRect(x: screen.maxX - 240, y: 80, width: 220, height: 120),
    styleMask: [.titled], backing: .buffered, defer: false)
window.title = "Drag source"
window.level = .floating
let source = SourceView(frame: NSRect(x: 0, y: 0, width: 220, height: 120))
window.contentView = source
window.makeKeyAndOrderFront(nil)

/// Vibez's content origin in global top-left coordinates, once its window shows.
func vibezContentOrigin() -> CGPoint {
    for _ in 0..<120 {
        let windows =
            CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]]
            ?? []
        for info in windows {
            guard info[kCGWindowOwnerPID as String] as? pid_t == vibezPid,
                info[kCGWindowLayer as String] as? Int == 0,
                let bounds = info[kCGWindowBounds as String] as? NSDictionary,
                let rect = CGRect(dictionaryRepresentation: bounds),
                rect.width > 400
            else { continue }
            return CGPoint(x: rect.minX, y: rect.minY + titleBarHeight)
        }
        Thread.sleep(forTimeInterval: 0.5)
    }
    fatalError("Vibez showed no window")
}

func post(_ type: CGEventType, at point: CGPoint) {
    CGEvent(mouseEventSource: nil, mouseType: type, mouseCursorPosition: point, mouseButton: .left)!
        .post(tap: .cghidEventTap)
}

func screenshot(_ name: String) {
    let path = shots.appendingPathComponent(name + ".png").path
    let capture = Process()
    capture.executableURL = URL(fileURLWithPath: "/usr/sbin/screencapture")
    capture.arguments = ["-x", path]
    try! capture.run()
    capture.waitUntilExit()
    print("Saved \(path)")
}

func drag(_ file: URL, to target: CGPoint, hoverShot: String) {
    var start = CGPoint.zero
    DispatchQueue.main.sync {
        source.file = file
        source.label.stringValue = file.lastPathComponent
        // Global coordinates have their origin at the top left of the main screen.
        let frame = window.frame
        start = CGPoint(x: frame.midX, y: screen.maxY - frame.midY)
    }
    post(.mouseMoved, at: start)
    Thread.sleep(forTimeInterval: 0.3)
    post(.leftMouseDown, at: start)
    Thread.sleep(forTimeInterval: 0.3)
    let steps = 40
    for step in 1...steps {
        let t = CGFloat(step) / CGFloat(steps)
        post(
            .leftMouseDragged,
            at: CGPoint(x: start.x + (target.x - start.x) * t, y: start.y + (target.y - start.y) * t))
        Thread.sleep(forTimeInterval: 0.025)
    }
    // A last nudge, so the drop target hears the drag at the target.
    post(.leftMouseDragged, at: CGPoint(x: target.x + 1, y: target.y))
    Thread.sleep(forTimeInterval: 0.15)
    post(.leftMouseDragged, at: target)
    Thread.sleep(forTimeInterval: 1.5)
    screenshot(hoverShot)
    post(.leftMouseUp, at: target)
    // Straight back off Vibez, so no ordinary mouse move reaches it after
    // the drop. A drop then lands only where the poll found a target.
    post(.mouseMoved, at: start)
    if dropped.wait(timeout: .now() + 10) == .timedOut {
        fatalError("the drag did not end")
    }
}

DispatchQueue.global().async {
    let origin = vibezContentOrigin()
    print("Vibez content at \(origin)")
    Thread.sleep(forTimeInterval: 3)
    var step = 4
    while step + 3 < arguments.count {
        let what = arguments[step]
        let target = CGPoint(
            x: origin.x + CGFloat(Double(arguments[step + 1])!) * interfaceScale,
            y: origin.y + CGFloat(Double(arguments[step + 2])!) * interfaceScale)
        let name = arguments[step + 3]
        if what == "click" {
            post(.mouseMoved, at: target)
            Thread.sleep(forTimeInterval: 0.2)
            post(.leftMouseDown, at: target)
            Thread.sleep(forTimeInterval: 0.08)
            post(.leftMouseUp, at: target)
            Thread.sleep(forTimeInterval: 1.5)
        } else {
            drag(URL(fileURLWithPath: what).standardizedFileURL, to: target, hoverShot: name + "-hover")
            Thread.sleep(forTimeInterval: 2.5)
        }
        screenshot(name)
        step += 4
    }
    DispatchQueue.main.async { app.terminate(nil) }
}

app.activate(ignoringOtherApps: true)
app.run()
