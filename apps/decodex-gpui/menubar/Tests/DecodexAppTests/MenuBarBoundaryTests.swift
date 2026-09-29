import Foundation
import XCTest

/// Static ownership checks only. These do not assess visual quality or runtime behavior.
final class MenuBarBoundaryTests: XCTestCase {
	private var packageURL: URL {
		URL(fileURLWithPath: #filePath).deletingLastPathComponent()
			.deletingLastPathComponent().deletingLastPathComponent()
	}

	func testHostOptsOutOfAutomaticAndSuddenTermination() throws {
		let repository = packageURL.deletingLastPathComponent()
			.deletingLastPathComponent().deletingLastPathComponent()
		let data = try Data(contentsOf: repository.appendingPathComponent(
			"apps/decodex-gpui/packaging/Info.plist"
		))
		let info = try XCTUnwrap(
			PropertyListSerialization.propertyList(from: data, format: nil) as? [String: Any]
		)
		XCTAssertEqual(info["NSSupportsAutomaticTermination"] as? Bool, false)
		XCTAssertEqual(info["NSSupportsSuddenTermination"] as? Bool, false)
	}

	func testMenuBarDoesNotOwnProviderCredentialsOrResetExecution() throws {
		let root = packageURL.appendingPathComponent("Sources/DecodexApp")
		let files = try XCTUnwrap(FileManager.default.enumerator(
			at: root, includingPropertiesForKeys: nil
		)).compactMap { $0 as? URL }.filter { $0.pathExtension == "swift" }
		XCTAssertFalse(files.isEmpty)
		// This boundary is intentionally static: provider authority belongs to the daemon.
		for file in files {
			let source = try String(contentsOf: file, encoding: .utf8)
			for marker in [
				"account/rateLimitResetCredit/consume", "CODEX_HOME", "access_token",
				"auth_json_path", "creditID", "creditId", "credit_id", "Process(",
				"URLSessionWebSocketTask",
			] {
				XCTAssertFalse(source.contains(marker), "\(file.lastPathComponent): \(marker)")
			}
		}
	}
}
