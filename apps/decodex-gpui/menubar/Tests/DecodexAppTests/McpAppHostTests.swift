import AppKit
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
}
