@testable import DecodexApp
import XCTest

final class DecodexServerBridgeTests: XCTestCase {
	func testBundledServerArgumentsStartServeOnListenAddress() {
		XCTAssertEqual(
			DecodexServerBridge.bundledServerArguments(listenAddress: "127.0.0.1:8192"),
			[
				"serve",
				"--listen-address",
				"127.0.0.1:8192",
			]
		)
	}

	func testConfiguredServerURLRequiresAbsoluteHTTPURL() throws {
		XCTAssertNil(try DecodexServerBridge.configuredServerURL(from: ""))
		XCTAssertEqual(
			try DecodexServerBridge.configuredServerURL(from: " http://127.0.0.1:57399 ")?.absoluteString,
			"http://127.0.0.1:57399"
		)
		XCTAssertThrowsError(try DecodexServerBridge.configuredServerURL(from: "127.0.0.1:57399"))
		XCTAssertThrowsError(try DecodexServerBridge.configuredServerURL(from: "file:///tmp/mock"))
	}

	func testDashboardWebSocketHandshakeRequestEndsWithHeaderTerminator() {
		let request = DashboardWebSocketConnection.handshakeRequest(
			host: "127.0.0.1",
			port: 8_192,
			path: "/dashboard/control",
			key: "test-key"
		)
		let text = String(decoding: request, as: UTF8.self)

		XCTAssertTrue(text.hasSuffix("\r\n\r\n"))
		XCTAssertTrue(text.contains("GET /dashboard/control HTTP/1.1\r\n"))
		XCTAssertTrue(text.contains("Sec-WebSocket-Key: test-key\r\n"))
	}
}
