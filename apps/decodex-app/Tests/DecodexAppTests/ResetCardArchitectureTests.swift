import Foundation
import XCTest

final class ResetCardArchitectureTests: XCTestCase {
	func testSwiftResetCardProductionPathHasNoProviderOrCredentialAuthority() throws {
		let testsURL = URL(fileURLWithPath: #filePath)
			.deletingLastPathComponent()
		let sourceURL = testsURL
			.deletingLastPathComponent()
			.deletingLastPathComponent()
			.appendingPathComponent("Sources/DecodexApp", isDirectory: true)
		let fileManager = FileManager.default
		let resetFiles = try XCTUnwrap(
			fileManager.enumerator(
				at: sourceURL,
				includingPropertiesForKeys: [.isRegularFileKey]
			)
		)
			.compactMap { $0 as? URL }
			.filter { url in
				guard url.pathExtension == "swift" else {
					return false
				}
				let name = url.lastPathComponent.lowercased()
				return name.contains("reset")
					|| name == "accountstoreactions.swift"
					|| name == "accountusagesummaryviews.swift"
			}

		XCTAssertFalse(resetFiles.isEmpty)
		let banned = [
			"account/rateLimitResetCredit/consume",
			"app-server",
			"CODEX_HOME",
			"access_token",
			"auth_json_path",
			"creditID",
			"creditId",
			"credit_id",
		]
		for fileURL in resetFiles {
			let source = try String(contentsOf: fileURL, encoding: .utf8)
			for term in banned {
				XCTAssertFalse(
					source.contains(term),
					"\(fileURL.lastPathComponent) contains forbidden reset authority term \(term)"
				)
			}
		}

		XCTAssertFalse(
			fileManager.fileExists(
				atPath: sourceURL
					.appendingPathComponent("CodexResetCreditBridge.swift")
					.path
			)
		)
	}
}
