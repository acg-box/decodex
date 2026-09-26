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
    func testToolBridgePreservesOneHostOperationAndExactBrowserReply() async throws {
        _ = NSApplication.shared
        let html = #"""
        <script>
        window.addEventListener('message',event=>{
          const m=event.data;
          if(m.id==='init' && m.result && m.result.hostCapabilities.serverTools){
            parent.postMessage({jsonrpc:'2.0',method:'ui/notifications/initialized'},'*');
            const call={jsonrpc:'2.0',id:'browser-id',method:'tools/call',params:{name:'calculate',arguments:{value:7}}};
            parent.postMessage(call,'*');
            parent.postMessage(call,'*');
            parent.postMessage({...call,params:{name:'calculate',arguments:{value:999}}},'*');
            parent.postMessage({...call,id:'second'},'*');
          }
          if(m.id==='second' && m.error) parent.postMessage({jsonrpc:'2.0',id:'busy-refused',method:'ping'},'*');
          if(m.id==='browser-id' && m.result && m.result.structuredContent.value===42 && m.result.content[0].text.length===8*1024*1024-1024)
            parent.postMessage({jsonrpc:'2.0',id:'exact-result',method:'ping'},'*');
        });
        parent.postMessage({jsonrpc:'2.0',id:'init',method:'ui/initialize',params:{protocolVersion:'2026-01-26'}},'*');
        </script>
        """#
        let data = try JSONSerialization.data(withJSONObject: [
            "item": ["type": "mcpToolCall", "mcpAppUi": ["resourceUri": "ui://fixture/view"]],
            "resources": [["uri": "ui://fixture/view", "mimeType": "text/html;profile=mcp-app", "text": html]]
        ])
        var calls: [[String: Any]] = []
        var observed: Set<String> = []
        let view = try McpAppView(document: McpAppDocument(data: data), toolCallsEnabled: true) { event in
            if event["type"] as? String == "tool_call" { calls.append(event) }
            if let id = event["id"] as? String { observed.insert(id) }
        }
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 720, height: 480), styleMask: [.titled], backing: .buffered, defer: false)
        window.contentView = view.webView
        window.orderFront(nil)
        defer { view.close(); window.orderOut(nil) }
        let deadline = Date().addingTimeInterval(20)
        while Date() < deadline && !observed.contains("busy-refused") {
            try await Task.sleep(for: .milliseconds(20))
        }
        XCTAssertTrue(observed.contains("busy-refused"))
        XCTAssertEqual(calls.count, 1)
        let call = try XCTUnwrap(calls.first)
        let operation = try XCTUnwrap(call["operationId"] as? String)
        XCTAssertNotNil(UUID(uuidString: operation))
        XCTAssertNotEqual(operation, "browser-id")
        XCTAssertEqual((call["arguments"] as? [String: Int])?["value"], 7)
        XCTAssertFalse(view.resolveTool(operation: "browser-id", result: [:], error: nil))
        let largeResult = String(repeating: "x", count: 8 * 1024 * 1024 - 1024)
        XCTAssertTrue(view.resolveTool(operation: operation, result: ["content": [["type": "text", "text": largeResult]], "structuredContent": ["value": 42]], error: nil))
        XCTAssertFalse(view.resolveTool(operation: operation, result: [:], error: nil))
        while Date() < deadline && !observed.contains("exact-result") {
            try await Task.sleep(for: .milliseconds(20))
        }
        XCTAssertTrue(observed.contains("exact-result"))
    }

    func testTeardownWaitsForExactReplyAndBoundsAnUnresponsiveWidget() async throws {
        _ = NSApplication.shared
        for responds in [true, false] {
            let html = """
            <script>
            window.addEventListener('message',event=>{
              const m=event.data;
              if(m.id==='init' && m.result) parent.postMessage({jsonrpc:'2.0',method:'ui/notifications/initialized'},'*');
              if(m.method==='ui/resource-teardown') {
                parent.postMessage({jsonrpc:'2.0',id:'wrong-teardown',result:{}},'*');
                parent.postMessage({jsonrpc:'2.0',id:'during-close',method:'tools/call',params:{name:'calculate',arguments:{value:999}}},'*');
                if(\(responds ? "true" : "false")) setTimeout(()=>parent.postMessage({jsonrpc:'2.0',id:m.id,result:{}},'*'),30);
              }
            });
            parent.postMessage({jsonrpc:'2.0',id:'init',method:'ui/initialize',params:{protocolVersion:'2026-01-26'}},'*');
            </script>
            """
            let data = try JSONSerialization.data(withJSONObject: [
                "item": ["type": "mcpToolCall", "mcpAppUi": ["resourceUri": "ui://fixture/view"]],
                "resources": [["uri": "ui://fixture/view", "mimeType": "text/html;profile=mcp-app", "text": html]]
            ])
            var calls = 0
            let view = try McpAppView(document: McpAppDocument(data: data), toolCallsEnabled: true) { event in
                if event["type"] as? String == "tool_call" { calls += 1 }
            }
            let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 720, height: 480), styleMask: [.titled], backing: .buffered, defer: false)
            window.contentView = view.webView
            window.orderFront(nil)
            defer { view.close(immediate: true); window.orderOut(nil) }
            let initializedDeadline = Date().addingTimeInterval(20)
            while !view.initialized && Date() < initializedDeadline { try await Task.sleep(for: .milliseconds(20)) }
            XCTAssertTrue(view.initialized)
            view.close()
            view.close()
            XCTAssertTrue(view.closed)
            XCTAssertFalse(view.initialized)
            XCTAssertFalse(view.disposed)
            let closedDeadline = Date().addingTimeInterval(3)
            while !view.disposed && Date() < closedDeadline { try await Task.sleep(for: .milliseconds(20)) }
            XCTAssertTrue(view.disposed)
            XCTAssertEqual(view.teardownAcknowledged, responds)
            XCTAssertEqual(calls, 0, "A closing view must never create new tool authority")
            XCTAssertNil(view.webView.navigationDelegate)
            XCTAssertNil(view.webView.uiDelegate)
        }
    }

}
