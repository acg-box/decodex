@testable import DecodexApp
import Foundation
import XCTest

final class FastModeControlTests: XCTestCase {
	func testFastModeDocumentRejectsUnknownFields() {
		let data = Data(
			"""
			{
			  "schema": "decodex/fast-mode-cli/1",
			  "command": "status",
			  "outcome": "success",
			  "enabled": true,
			  "extra": false
			}
			""".utf8
		)

		XCTAssertThrowsError(try JSONDecoder().decode(FastModeDocument.self, from: data))
	}

	func testFastModeDocumentReadsExactSuccess() throws {
		let data = Data(
			"""
			{
			  "schema": "decodex/fast-mode-cli/1",
			  "command": "status",
			  "outcome": "success",
			  "enabled": true
			}
			""".utf8
		)

		let value = try JSONDecoder().decode(FastModeDocument.self, from: data)
		XCTAssertEqual(value.schema, "decodex/fast-mode-cli/1")
		XCTAssertEqual(value.command, "status")
		XCTAssertEqual(value.outcome, "success")
		XCTAssertTrue(value.enabled)
	}
}
