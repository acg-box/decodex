import Foundation
import XCTest

final class ResetCardArchitectureTests: XCTestCase {
	func testRepeatedAccountRowsDoNotShareAnAppearanceIdentity() throws {
		let sourceURL = URL(fileURLWithPath: #filePath)
			.deletingLastPathComponent()
			.deletingLastPathComponent()
			.deletingLastPathComponent()
			.appendingPathComponent("Sources/DecodexApp", isDirectory: true)
		let glassSurface = try String(
			contentsOf: sourceURL.appendingPathComponent("PanelGlassSurface.swift"),
			encoding: .utf8
		)
		let accountPanel = try String(
			contentsOf: sourceURL.appendingPathComponent("AccountPanelView.swift"),
			encoding: .utf8
		)

		XCTAssertFalse(glassSurface.contains(".id(appearanceID)"))
		XCTAssertFalse(accountPanel.contains("LazyVStack"))
		XCTAssertTrue(accountPanel.contains("id: \\.element.id"))
	}

	func testProductionSourceUsesOnlyTheNativeResetCardAuthority() throws {
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
			"ResetCardCLIClient",
			"FastModeCLIClient",
			"Process(",
			"Contents/Helpers",
			"decodex-cli",
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

	func testBundleStagesTheAppAndNativeClientWithoutCLI() throws {
		let testsURL = URL(fileURLWithPath: #filePath)
			.deletingLastPathComponent()
		let appURL = testsURL
			.deletingLastPathComponent()
			.deletingLastPathComponent()
		let script = try String(
			contentsOf: appURL.appendingPathComponent("script/build_and_run.sh"),
			encoding: .utf8
		)

		XCTAssertTrue(
			script.contains(#"NATIVE_CLIENT_NAME="libdecodex_app_client_ffi.dylib""#)
		)
		XCTAssertTrue(script.contains("-p decodex-app-client-ffi"))
		XCTAssertTrue(
			script.contains(#"cp "$NATIVE_CLIENT_BINARY" "$APP_NATIVE_CLIENT""#)
		)
		XCTAssertTrue(script.contains(#""$APP_NATIVE_CLIENT""#))
		for retiredPackageTerm in [
			"CLI_NAME",
			"APP_CLI_BINARY",
			"-p decodex-cli",
			"Contents/Helpers",
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
