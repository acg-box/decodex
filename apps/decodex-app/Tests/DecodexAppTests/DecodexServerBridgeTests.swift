@testable import DecodexApp
import XCTest

final class DecodexServerBridgeTests: XCTestCase {
	func testBundledServerArgumentsAllowUnverifiedCodex() {
		XCTAssertEqual(
			DecodexServerBridge.bundledServerArguments(listenAddress: "127.0.0.1:8192"),
			[
				"serve",
				"--allow-unverified-codex",
				"--listen-address",
				"127.0.0.1:8192",
			]
		)
	}
}
