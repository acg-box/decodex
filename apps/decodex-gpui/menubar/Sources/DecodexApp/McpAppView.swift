import AppKit
import CoreFoundation
import WebKit

/// One disposable, nonpersistent browser for one source-bound native widget document.
@MainActor
final class McpAppView: NSObject, WKScriptMessageHandler, WKNavigationDelegate, WKUIDelegate {
    let webView: WKWebView
    private let document: McpAppDocument
    private(set) var initialized = false
    private(set) var closed = false
    private var handshake = false
    private let toolCallsEnabled: Bool
    private var pendingTool: (operation: String, rpcID: Any)?
    private let observe: ([String: Any]) -> Void

    init(document: McpAppDocument, toolCallsEnabled: Bool = false, observe: @escaping ([String: Any]) -> Void) throws {
        self.document = document
        self.toolCallsEnabled = toolCallsEnabled
        self.observe = observe
        let configuration = WKWebViewConfiguration()
        configuration.websiteDataStore = .nonPersistent()
        configuration.preferences.javaScriptCanOpenWindowsAutomatically = false
        webView = WKWebView(frame: NSRect(x: 0, y: 0, width: 720, height: 480), configuration: configuration)
        super.init()
        configuration.userContentController.add(self, name: "mcpApp")
        webView.navigationDelegate = self
        webView.uiDelegate = self
        let source = try Self.json(document.sandboxedHTML)
        // Only the opaque child can send messages through this relay. The native handler
        // additionally rejects messages posted directly from child or nested frames.
        webView.loadHTMLString("""
        <!doctype html><meta charset="utf-8">
        <style>html,body,iframe{margin:0;width:100%;height:100%;border:0;overflow:hidden}</style>
        <iframe id="widget" sandbox="allow-scripts" allow="camera 'none'; microphone 'none'; geolocation 'none'; clipboard-write 'none'"></iframe>
        <script>
        const view=document.getElementById('widget');
        window.addEventListener('message',event=>{
          if(event.source!==view.contentWindow || !event.data || event.data.jsonrpc!=='2.0') return;
          window.webkit.messageHandlers.mcpApp.postMessage(event.data);
        });
        window.decodexDeliver=message=>view.contentWindow.postMessage(message,'*');
        view.srcdoc=\(source);
        </script>
        """, baseURL: nil)
    }

    func userContentController(_ userContentController: WKUserContentController, didReceive message: WKScriptMessage) {
        guard !closed, message.frameInfo.isMainFrame, message.name == "mcpApp",
              let value = message.body as? [String: Any], value["jsonrpc"] as? String == "2.0",
              let method = value["method"] as? String,
              JSONSerialization.isValidJSONObject(value),
              let encoded = try? JSONSerialization.data(withJSONObject: value), encoded.count <= 256 * 1024 else { return }
        let id = value["id"]
        if let id {
            if let number = id as? NSNumber {
                guard CFGetTypeID(number) != CFBooleanGetTypeID() else { return }
            } else if !(id is String) { return }
        }
        switch method {
        case "ui/initialize":
            guard !handshake, let id,
                  let parameters = value["params"] as? [String: Any],
                  parameters["protocolVersion"] as? String == "2026-01-26" else {
                reject(id, code: -32602, message: "Unsupported initialization")
                return
            }
            handshake = true
            deliver(["jsonrpc": "2.0", "id": id, "result": [
                "protocolVersion": "2026-01-26", "hostInfo": ["name": "Decodex", "version": "1"],
                "hostCapabilities": hostCapabilities,
                "hostContext": ["platform": "desktop", "displayMode": "inline", "availableDisplayModes": ["inline"],
                                "containerDimensions": ["width": 720, "height": 480],
                                "locale": Locale.current.identifier, "timeZone": TimeZone.current.identifier]
            ]])
        case "ui/notifications/initialized":
            guard handshake, !initialized, id == nil else { return }
            initialized = true
            deliver(["jsonrpc": "2.0", "method": "ui/notifications/tool-input",
                     "params": ["arguments": document.item["arguments"] ?? [:]]])
            if let result = document.item["result"] as? [String: Any] {
                deliver(["jsonrpc": "2.0", "method": "ui/notifications/tool-result", "params": result])
            }
            observe(["type": "initialized"])
        case "tools/call":
            guard initialized, toolCallsEnabled, let id else {
                reject(id, code: -32601, message: "Tool calls are unavailable")
                return
            }
            // A repeated browser request cannot mint another operation or replace intent.
            if let pendingTool {
                if NSDictionary(dictionary: ["id": pendingTool.rpcID]).isEqual(to: ["id": id]) { return }
                reject(id, code: -32000, message: "Another app call is awaiting resolution")
                return
            }
            guard let parameters = value["params"] as? [String: Any],
                  let name = parameters["name"] as? String, !name.isEmpty,
                  let arguments = (parameters["arguments"] ?? [:]) as? [String: Any],
                  encoded.count <= 64 * 1024 else {
                reject(id, code: -32602, message: "Invalid or oversized tool invocation")
                return
            }
            let operation = UUID().uuidString
            pendingTool = (operation, id)
            observe(["type": "tool_call", "operationId": operation, "tool": name, "arguments": arguments])
        case "ping":
            guard initialized, let id else { return }
            deliver(["jsonrpc": "2.0", "id": id, "result": [:]])
            observe(["type": "ping", "id": id])
        default:
            reject(id, code: -32601, message: "This host capability is not available")
        }
    }

    private var hostCapabilities: [String: Any] {
        var capabilities: [String: Any] = ["sandbox": ["permissions": [:]]]
        if toolCallsEnabled { capabilities["serverTools"] = [:] }
        return capabilities
    }

    /// Only the native controller can resolve the operation; browser IDs are never authority.
    func resolveTool(operation: String, result: [String: Any]?, error: String?) -> Bool {
        guard !closed, let pendingTool, pendingTool.operation == operation,
              (result != nil) != (error != nil) else { return false }
        if let result {
            deliver(["jsonrpc": "2.0", "id": pendingTool.rpcID, "result": result])
        } else {
            reject(pendingTool.rpcID, code: -32000, message: error!)
        }
        self.pendingTool = nil
        return true
    }

    private func reject(_ id: Any?, code: Int, message: String) {
        guard let id else { return }
        deliver(["jsonrpc": "2.0", "id": id, "error": ["code": code, "message": message]])
    }

    private func deliver(_ message: [String: Any]) {
        guard !closed, let json = try? Self.json(message) else { return }
        webView.evaluateJavaScript("window.decodexDeliver(\(json))", completionHandler: nil)
    }

    func close() {
        guard !closed else { return }
        closed = true
        pendingTool = nil
        initialized = false
        webView.stopLoading()
        webView.configuration.userContentController.removeScriptMessageHandler(forName: "mcpApp")
        webView.navigationDelegate = nil
        webView.uiDelegate = nil
        webView.removeFromSuperview()
    }

    func webView(_ webView: WKWebView, decidePolicyFor navigationAction: WKNavigationAction,
                 decisionHandler: @escaping @MainActor (WKNavigationActionPolicy) -> Void) {
        let url = navigationAction.request.url
        if closed { decisionHandler(.cancel); return }
        if navigationAction.targetFrame?.isMainFrame == true {
            decisionHandler(url?.absoluteString == "about:blank" ? .allow : .cancel)
        } else {
            // Keep the loaded widget in its policy-bearing srcdoc. External frame
            // navigation requires a separate declared-origin implementation.
            decisionHandler(url?.absoluteString == "about:srcdoc" ? .allow : .cancel)
        }
    }

    func webView(_ webView: WKWebView, requestMediaCapturePermissionFor origin: WKSecurityOrigin,
                 initiatedByFrame frame: WKFrameInfo, type: WKMediaCaptureType,
                 decisionHandler: @escaping @MainActor (WKPermissionDecision) -> Void) { decisionHandler(.deny) }

    func webView(_ webView: WKWebView, createWebViewWith configuration: WKWebViewConfiguration,
                 for navigationAction: WKNavigationAction, windowFeatures: WKWindowFeatures) -> WKWebView? { nil }

    func webView(_ webView: WKWebView, runOpenPanelWith parameters: WKOpenPanelParameters,
                 initiatedByFrame frame: WKFrameInfo, completionHandler: @escaping @MainActor ([URL]?) -> Void) { completionHandler(nil) }

    func webView(_ webView: WKWebView, runJavaScriptAlertPanelWithMessage message: String,
                 initiatedByFrame frame: WKFrameInfo, completionHandler: @escaping @MainActor () -> Void) { completionHandler() }

    func webView(_ webView: WKWebView, runJavaScriptConfirmPanelWithMessage message: String,
                 initiatedByFrame frame: WKFrameInfo, completionHandler: @escaping @MainActor (Bool) -> Void) { completionHandler(false) }

    func webView(_ webView: WKWebView, runJavaScriptTextInputPanelWithPrompt prompt: String, defaultText: String?,
                 initiatedByFrame frame: WKFrameInfo, completionHandler: @escaping @MainActor (String?) -> Void) { completionHandler(nil) }

    func webViewWebContentProcessDidTerminate(_ webView: WKWebView) {
        observe(["type": "unavailable"])
        close()
    }

    private static func json(_ value: Any) throws -> String {
        let data = try JSONSerialization.data(withJSONObject: value, options: [.fragmentsAllowed, .sortedKeys])
        return String(decoding: data, as: UTF8.self).replacingOccurrences(of: "<", with: "\\u003c")
            .replacingOccurrences(of: "\u{2028}", with: "\\u2028").replacingOccurrences(of: "\u{2029}", with: "\\u2029")
    }
}
