import Foundation
import XCTest
@testable import DecodexApp

final class McpAppDocumentTests: XCTestCase {
    private func document(policy: [String: Any] = [:], resources: [[String: Any]]? = nil) throws -> Data {
        try JSONSerialization.data(withJSONObject: [
            "item": ["type": "mcpToolCall", "mcpAppUi": ["resourceUri": "ui://fixture/view", "preferredModelDisplayMode": "fullscreen"]],
            "resources": resources ?? [["uri": "ui://fixture/view", "mimeType": "text/html;profile=mcp-app", "text": "<button>Test</button>", "_meta": ["ui": ["csp": policy]]]]
        ])
    }

    func testExactResourceAndRestrictiveDefaultPolicy() throws {
        let view = try McpAppDocument(data: document())
        XCTAssertEqual(view.html, "<button>Test</button>")
        XCTAssertEqual(view.preferredDisplayMode, "fullscreen")
        XCTAssertTrue(view.contentSecurityPolicy.contains("connect-src 'none'"))
        XCTAssertTrue(view.contentSecurityPolicy.contains("frame-src 'none'"))
        XCTAssertTrue(view.contentSecurityPolicy.contains("form-action 'none'"))
        XCTAssertTrue(view.sandboxedHTML.hasPrefix("<!doctype html><meta http-equiv=\"Content-Security-Policy\""))
    }

    func testDeclaredOriginsCannotInjectPolicyOrReadLocalFiles() throws {
        let view = try McpAppDocument(data: document(policy: [
            "connectDomains": ["https://api.example.com", "wss://events.example.com"],
            "resourceDomains": ["https://*.example.com/"]
        ]))
        XCTAssertTrue(view.contentSecurityPolicy.contains("connect-src https://api.example.com wss://events.example.com"))
        for origin in ["file:///tmp/private", "data:text/html,test", "https://host; default-src *", "https://user@host", "https://host/path", "*", "https://host?q=x", "https://bad.*.host"] {
            XCTAssertThrowsError(try McpAppDocument(data: document(policy: ["resourceDomains": [origin]])), origin)
        }
    }

    func testMissingDuplicateOrAmbiguousResourcesAreRejected() throws {
        let valid: [String: Any] = ["uri": "ui://fixture/view", "mimeType": "text/html;profile=mcp-app", "text": "<p>One</p>"]
        XCTAssertThrowsError(try McpAppDocument(data: document(resources: [])))
        XCTAssertThrowsError(try McpAppDocument(data: document(resources: [valid, valid])))
        var ambiguous = valid
        ambiguous["blob"] = Data("other".utf8).base64EncodedString()
        XCTAssertThrowsError(try McpAppDocument(data: document(resources: [ambiguous])))
        var blob = valid
        blob.removeValue(forKey: "text")
        blob["blob"] = Data("<p>Bytes</p>".utf8).base64EncodedString()
        XCTAssertEqual(try McpAppDocument(data: document(resources: [blob])).html, "<p>Bytes</p>")
    }
    func testNullableNativeLegacyFieldsRemainReadable() throws {
        let bytes = try JSONSerialization.data(withJSONObject: [
            "item": ["type": "mcpToolCall", "mcpAppUi": NSNull(), "mcpAppResourceUri": NSNull(),
                     "appContext": ["resourceUri": "ui://fixture/view"]],
            "resources": [["uri": "ui://fixture/view", "mimeType": "text/html;profile=mcp-app", "text": "legacy", "blob": NSNull()]]
        ])
        let view = try McpAppDocument(data: bytes)
        XCTAssertEqual(view.html, "legacy")
        XCTAssertEqual(view.preferredDisplayMode, "inline")
    }

}
