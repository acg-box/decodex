import AppKit

/// Native window lifetime. Rust retains task/account identity and owns tool authority.
@MainActor
final class McpAppHost: NSObject, NSWindowDelegate {
    private weak var parent: NSWindow?
    private var panel: NSPanel?
    private var view: McpAppView?
    private var events: [String] = []
    private var retainedEvent: UnsafeMutablePointer<CChar>?
    private var closed = false
    private var displayMode = "inline"
    private var inlineFrame: NSRect?

    init(parent: NSWindow) { self.parent = parent }

    func command(_ text: String) -> Bool {
        guard !closed, let data = text.data(using: .utf8), data.count <= 32 * 1024 * 1024,
              let command = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let operation = command["operation"] as? String else { return false }
        if operation == "close" { close(); return true }
        if operation == "tool_result" {
            guard let view, let identity = command["operationId"] as? String else { return false }
            return view.resolveTool(operation: identity, result: command["result"] as? [String: Any],
                                    error: command["error"] as? String)
        }
        guard data.count <= 8 * 1024 * 1024, operation == "load", view == nil, let parent,
              let document = command["document"], JSONSerialization.isValidJSONObject(document),
              let bytes = try? JSONSerialization.data(withJSONObject: document),
              let parsed = try? McpAppDocument(data: bytes) else { return false }
        do {
            let view = try McpAppView(document: parsed, toolCallsEnabled: command["toolCallsEnabled"] as? Bool == true,
                                      changeDisplayMode: { [weak self] mode in self?.setDisplayMode(mode) ?? "inline" }) { [weak self] in self?.emit($0) }
            let panel = NSPanel(contentRect: NSRect(x: 0, y: 0, width: 720, height: 480),
                                styleMask: [.titled, .closable, .resizable], backing: .buffered, defer: false)
            panel.title = "App"
            panel.isReleasedWhenClosed = false
            panel.delegate = self
            panel.contentView = view.webView
            panel.center()
            parent.addChildWindow(panel, ordered: .above)
            self.view = view
            self.panel = panel
            panel.makeKeyAndOrderFront(nil)
            return true
        } catch { return false }
    }

    /// Fullscreen uses the host window's available area, not a separate macOS Space.
    private func setDisplayMode(_ mode: String) -> String {
        guard !closed, let panel, let parent, ["inline", "fullscreen"].contains(mode) else { return displayMode }
        guard mode != displayMode else { return displayMode }
        if mode == "fullscreen" {
            inlineFrame = panel.frame
            displayMode = mode
            panel.setFrame(parent.frame, display: true)
        } else {
            displayMode = mode
            if let inlineFrame { panel.setFrame(inlineFrame, display: true) }
            inlineFrame = nil
        }
        view?.updateHostContext(mode: displayMode)
        return displayMode
    }

    func windowDidResize(_ notification: Notification) {
        view?.updateHostContext(mode: displayMode)
    }

    func poll() -> UnsafePointer<CChar>? {
        if let retainedEvent { free(retainedEvent); self.retainedEvent = nil }
        guard !events.isEmpty else { return nil }
        retainedEvent = strdup(events.removeFirst())
        return retainedEvent.map { UnsafePointer($0) }
    }

    private func emit(_ event: [String: Any]) {
        guard !closed,
              let data = try? JSONSerialization.data(withJSONObject: event),
              let text = String(data: data, encoding: .utf8) else { return }
        if events.count >= 32 {
            // Status pings cannot make an accepted tool request disappear.
            guard event["type"] as? String == "tool_call" else { return }
            events.removeFirst()
        }
        events.append(text)
    }

    func windowWillClose(_ notification: Notification) { close() }

    func close() {
        guard !closed else { return }
        events = [#"{"type":"closed"}"#]
        closed = true
        view?.close()
        view = nil
        if let panel {
            panel.delegate = nil
            parent?.removeChildWindow(panel)
            panel.close()
        }
        panel = nil
    }

    func dispose() {
        close()
        if let retainedEvent { free(retainedEvent); self.retainedEvent = nil }
    }
}

@_cdecl("decodex_mcp_app_abi_version")
public func decodexMcpAppABIVersion() -> UInt32 { 1 }

@_cdecl("decodex_mcp_app_create")
public func decodexMcpAppCreate(_ nativeView: UnsafeMutableRawPointer?) -> UnsafeMutableRawPointer? {
    guard Thread.isMainThread, let nativeView else { return nil }
    let address = UInt(bitPattern: nativeView)
    let host = MainActor.assumeIsolated {
        let view = Unmanaged<NSView>.fromOpaque(UnsafeMutableRawPointer(bitPattern: address)!).takeUnretainedValue()
        guard let window = view.window else { return UInt(0) }
        return UInt(bitPattern: Unmanaged.passRetained(McpAppHost(parent: window)).toOpaque())
    }
    return UnsafeMutableRawPointer(bitPattern: host)
}

@_cdecl("decodex_mcp_app_command")
public func decodexMcpAppCommand(_ pointer: UnsafeMutableRawPointer?, _ json: UnsafePointer<CChar>?) -> Bool {
    guard Thread.isMainThread, let pointer, let json else { return false }
    let address = UInt(bitPattern: pointer)
    let command = String(cString: json)
    return MainActor.assumeIsolated {
        guard let host = UnsafeMutableRawPointer(bitPattern: address) else { return false }
        return Unmanaged<McpAppHost>.fromOpaque(host).takeUnretainedValue().command(command)
    }
}

@_cdecl("decodex_mcp_app_poll")
public func decodexMcpAppPoll(_ pointer: UnsafeMutableRawPointer?) -> UnsafePointer<CChar>? {
    guard Thread.isMainThread, let pointer else { return nil }
    let address = UInt(bitPattern: pointer)
    let event: UInt = MainActor.assumeIsolated {
        guard let host = UnsafeMutableRawPointer(bitPattern: address) else { return 0 }
        return UInt(bitPattern: Unmanaged<McpAppHost>.fromOpaque(host).takeUnretainedValue().poll())
    }
    return UnsafePointer<CChar>(bitPattern: event)
}

@_cdecl("decodex_mcp_app_destroy")
public func decodexMcpAppDestroy(_ pointer: UnsafeMutableRawPointer?) {
    guard Thread.isMainThread, let pointer else { return }
    let address = UInt(bitPattern: pointer)
    MainActor.assumeIsolated {
        guard let host = UnsafeMutableRawPointer(bitPattern: address) else { return }
        Unmanaged<McpAppHost>.fromOpaque(host).takeRetainedValue().dispose()
    }
}
