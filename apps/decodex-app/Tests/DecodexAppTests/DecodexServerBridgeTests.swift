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
}
