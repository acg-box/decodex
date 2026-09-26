import AppKit
import WebKit
import XCTest
@testable import DecodexApp

@MainActor
final class McpAppViewTests: XCTestCase {
    func testRealWebKitInitializesOpaqueWidgetAndDeliversToolData() async throws {
        _ = NSApplication.shared
        let html = #"""
        <button id="value">Waiting</button><script>
        window.addEventListener('message',event=>{
          const m=event.data;
          if(m.id===1 && m.result){
            if(m.result.hostCapabilities.serverTools) throw new Error('unexpected authority');
            parent.postMessage({jsonrpc:'2.0',method:'ui/notifications/initialized'},'*');
          }
          if(m.method==='ui/notifications/tool-result'){
            document.getElementById('value').textContent=String(m.params.structuredContent.value);
            try { window.webkit.messageHandlers.mcpApp.postMessage({jsonrpc:'2.0',id:'direct-child',method:'ping'}); } catch (_) {}
            parent.postMessage({jsonrpc:'2.0',id:'received-'+m.params.structuredContent.value,method:'ping'},'*');
            try { parent.document.body.innerHTML='escaped'; }
            catch (_) { parent.postMessage({jsonrpc:'2.0',id:'opaque',method:'ping'},'*'); }
          }
        });
        parent.postMessage({jsonrpc:'2.0',id:1,method:'ui/initialize',params:{protocolVersion:'2026-01-26',appCapabilities:{}}},'*');
        </script>
        """#
        let data = try JSONSerialization.data(withJSONObject: [
            "item": ["type": "mcpToolCall", "mcpAppUi": ["resourceUri": "ui://fixture/view"],
                     "arguments": ["value": 7], "result": ["content": [], "structuredContent": ["value": 7]]],
            "resources": [["uri": "ui://fixture/view", "mimeType": "text/html;profile=mcp-app", "text": html]]
        ])
        var observed: Set<String> = []
        let view = try McpAppView(document: McpAppDocument(data: data)) { event in
            if let id = event["id"] as? String { observed.insert(id) }
        }
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 720, height: 480), styleMask: [.titled], backing: .buffered, defer: false)
        window.contentView = view.webView
        window.orderFront(nil)
        defer { view.close(); window.orderOut(nil) }
        let deadline = Date().addingTimeInterval(20)
        while Date() < deadline && !observed.isSuperset(of: ["received-7", "opaque"]) {
            try await Task.sleep(for: .milliseconds(20))
        }
        XCTAssertTrue(view.initialized)
        XCTAssertTrue(observed.contains("received-7"))
        XCTAssertTrue(observed.contains("opaque"), "Widget must not access the host DOM")
        try await Task.sleep(for: .milliseconds(50))
        XCTAssertFalse(observed.contains("direct-child"), "Child frames cannot call the native handler directly")
        XCTAssertFalse(view.webView.configuration.websiteDataStore.isPersistent)
        view.close()
        XCTAssertTrue(view.closed)
        XCTAssertFalse(view.initialized)
    }
}
