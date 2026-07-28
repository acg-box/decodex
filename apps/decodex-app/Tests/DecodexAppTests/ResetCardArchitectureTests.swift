import Foundation
import XCTest

final class ResetCardArchitectureTests: XCTestCase {
	func testProductionSourceHasOneCLIResetCardAuthorityAndNoRetiredUISurfaces() throws {
		let testsURL = URL(fileURLWithPath: #filePath)
			.deletingLastPathComponent()
		let appURL = testsURL
			.deletingLastPathComponent()
			.deletingLastPathComponent()
		let sourceURL = appURL
			.appendingPathComponent("Sources/DecodexApp", isDirectory: true)
		let fileManager = FileManager.default
		let swiftFiles = try XCTUnwrap(
			fileManager.enumerator(
				at: sourceURL,
				includingPropertiesForKeys: [.isRegularFileKey]
			)
		)
			.compactMap { $0 as? URL }
			.filter { url in
				url.pathExtension == "swift"
			}

		XCTAssertFalse(swiftFiles.isEmpty)
		let banned = [
			"account/rateLimitResetCredit/consume",
			"app-server",
			"reset-card accounts",
			"CODEX_HOME",
			"access_token",
			"auth_json_path",
			"creditID",
			"creditId",
			"credit_id",
			"AccountStore",
			"DecodexAppBridge",
			"DecodexServerBridge",
			"DashboardWebSocket",
			"URLSessionWebSocketTask",
			"127.0.0.1:8192",
			"localhost:8192",
			"legacy",
			"vNext",
			"VNext",
		]
		for fileURL in swiftFiles {
			let source = try String(contentsOf: fileURL, encoding: .utf8)
			for term in banned {
				XCTAssertFalse(
					source.contains(term),
					"\(fileURL.lastPathComponent) contains retired authority or UI term \(term)"
				)
			}
		}

		for retiredFile in [
			"AccountStore.swift",
			"DecodexAppBridge.swift",
			"DecodexServerBridge.swift",
			"DashboardWebSocketConnection.swift",
			"LoginSheetView.swift",
			"OperatorSnapshotModels.swift",
		] {
			XCTAssertFalse(
				fileManager.fileExists(
					atPath: sourceURL.appendingPathComponent(retiredFile).path
				),
				"\(retiredFile) must not remain in the clean-break app."
			)
		}
	}

	func testBundleStagesOnlyTheAppAndDecodexCLIExecutables() throws {
		let testsURL = URL(fileURLWithPath: #filePath)
			.deletingLastPathComponent()
		let appURL = testsURL
			.deletingLastPathComponent()
			.deletingLastPathComponent()
		let script = try String(
			contentsOf: appURL.appendingPathComponent("script/build_and_run.sh"),
			encoding: .utf8
		)

		XCTAssertTrue(script.contains(#"CLI_NAME="decodex-cli""#))
		XCTAssertTrue(script.contains("-p decodex-cli"))
		XCTAssertTrue(script.contains(#"cp "$CLI_BINARY" "$APP_CLI_BINARY""#))
		for retiredPackageTerm in [
			"decodex-app-helper",
			"-p decodexd",
			"decodex-server",
			"SERVER_NAME",
			"DAEMON_NAME",
			":8192",
		] {
			XCTAssertFalse(
				script.contains(retiredPackageTerm),
				"Build script still packages retired component \(retiredPackageTerm)."
			)
		}
	}
}
