import AppKit
import WebKit
import XCTest
@testable import DecodexApp

@MainActor
final class McpAppHostTests: XCTestCase {
    func testNativeABIShowsOnlyOneBoundDocumentAndClosesItsChildWindow() throws {
        _ = NSApplication.shared
        let parent = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 800, height: 600), styleMask: [.titled], backing: .buffered, defer: false)
        let view = NSView(frame: parent.contentView!.bounds)
        parent.contentView = view
        parent.orderFront(nil)
        defer { parent.orderOut(nil) }
        XCTAssertEqual(decodexMcpAppABIVersion(), 1)
        XCTAssertNil(decodexMcpAppCreate(nil))
        let host = try XCTUnwrap(decodexMcpAppCreate(Unmanaged.passUnretained(view).toOpaque()))
        defer { decodexMcpAppDestroy(host) }
        XCTAssertFalse("{}".withCString { decodexMcpAppCommand(host, $0) })
        let command = #"{"operation":"load","document":{"item":{"type":"mcpToolCall","mcpAppUi":{"resourceUri":"ui://fixture/view"}},"resources":[{"uri":"ui://fixture/view","mimeType":"text/html;profile=mcp-app","text":"<button>Fixture</button>"}]}}"#
        XCTAssertTrue(command.withCString { decodexMcpAppCommand(host, $0) })
        XCTAssertEqual(parent.childWindows?.count, 1)
        XCTAssertFalse(command.withCString { decodexMcpAppCommand(host, $0) }, "One host cannot silently replace its source")
        XCTAssertTrue(#"{"operation":"close"}"#.withCString { decodexMcpAppCommand(host, $0) })
        XCTAssertTrue(parent.childWindows?.isEmpty ?? true)
        let event = try XCTUnwrap(decodexMcpAppPoll(host))
        XCTAssertEqual(String(cString: event), #"{"type":"closed"}"#)
        XCTAssertNil(decodexMcpAppPoll(host))
        XCTAssertFalse(command.withCString { decodexMcpAppCommand(host, $0) })
    }
    func testWindowDisplayModeHonorsWidgetCapabilitiesAndRestoresInlineSize() async throws {
        _ = NSApplication.shared
        for supportsFullscreen in [true, false] {
            let parent = NSWindow(contentRect: NSRect(x: 100, y: 100, width: 1100, height: 760), styleMask: [.titled], backing: .buffered, defer: false)
            parent.orderFront(nil)
            let host = McpAppHost(parent: parent)
            defer { host.dispose(); parent.orderOut(nil) }
            let html = """
            <script>
            window.addEventListener('message', event=>{
              const m=event.data;
              if(m.id==='init' && m.result){
                parent.postMessage({jsonrpc:'2.0',method:'ui/notifications/initialized'},'*');
                parent.postMessage({jsonrpc:'2.0',id:'initial-'+m.result.hostContext.displayMode,method:'ping'},'*');
              }
              if(m.method==='ui/notifications/host-context-changed') parent.postMessage({jsonrpc:'2.0',id:'size-'+m.params.containerDimensions.width+'-'+m.params.containerDimensions.height,method:'ping'},'*');
              if(m.method==='test/inline') parent.postMessage({jsonrpc:'2.0',id:'inline',method:'ui/request-display-mode',params:{mode:'inline'}},'*');
              if(m.id==='inline' && m.result) parent.postMessage({jsonrpc:'2.0',id:'restored-'+m.result.mode,method:'ping'},'*');
              if(m.method==='test/pip') parent.postMessage({jsonrpc:'2.0',id:'pip',method:'ui/request-display-mode',params:{mode:'pip'}},'*');
              if(m.id==='pip' && m.result) parent.postMessage({jsonrpc:'2.0',id:'unsupported-'+m.result.mode,method:'ping'},'*');
            });
            parent.postMessage({jsonrpc:'2.0',id:'init',method:'ui/initialize',params:{protocolVersion:'2026-01-26',appCapabilities:{availableDisplayModes:\(supportsFullscreen ? "['inline','fullscreen']" : "['inline']")}}},'*');
            </script>
            """
            let load: [String: Any] = ["operation": "load", "document": [
                "item": ["type": "mcpToolCall", "mcpAppUi": ["resourceUri": "ui://fixture/view", "preferredModelDisplayMode": "fullscreen"]],
                "resources": [["uri": "ui://fixture/view", "mimeType": "text/html;profile=mcp-app", "text": html]]
            ]]
            XCTAssertTrue(host.command(String(decoding: try JSONSerialization.data(withJSONObject: load), as: UTF8.self)))
            var observed: Set<String> = []
            func collect() {
                while let pointer = host.poll() {
                    let value = try? JSONSerialization.jsonObject(with: Data(String(cString: pointer).utf8)) as? [String: Any]
                    if let id = value?["id"] as? String { observed.insert(id) }
                }
            }
            let deadline = Date().addingTimeInterval(20)
            let initial = supportsFullscreen ? "initial-fullscreen" : "initial-inline"
            while !observed.contains(initial) && Date() < deadline { collect(); try await Task.sleep(for: .milliseconds(20)) }
            XCTAssertTrue(observed.contains(initial))
            let panel = try XCTUnwrap(parent.childWindows?.first)
            let webView = try XCTUnwrap(panel.contentView as? WKWebView)
            if supportsFullscreen { XCTAssertEqual(panel.frame, parent.frame) }
            else { XCTAssertEqual(webView.bounds.size, NSSize(width: 720, height: 480)) }
            webView.evaluateJavaScript("window.decodexDeliver({method:'test/inline'})", completionHandler: nil)
            while !observed.contains("restored-inline") && Date() < deadline { collect(); try await Task.sleep(for: .milliseconds(20)) }
            XCTAssertTrue(observed.contains("restored-inline"))
            XCTAssertEqual(webView.bounds.size, NSSize(width: 720, height: 480))
            webView.evaluateJavaScript("window.decodexDeliver({method:'test/pip'})", completionHandler: nil)
            while !observed.contains("unsupported-inline") && Date() < deadline { collect(); try await Task.sleep(for: .milliseconds(20)) }
            XCTAssertTrue(observed.contains("unsupported-inline"))
            panel.setContentSize(NSSize(width: 800, height: 600))
            while !observed.contains("size-800-600") && Date() < deadline { collect(); try await Task.sleep(for: .milliseconds(20)) }
            XCTAssertTrue(observed.contains("size-800-600"))
        }
    }

}
