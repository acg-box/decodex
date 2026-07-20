@testable import DecodexApp
import Foundation
import XCTest

final class DecodexAppBridgeTests: XCTestCase {
	func testCodexExecutableOverrideTakesPrecedence() throws {
		let override = "/custom/Codex CLI/codex"
		let pathCandidate = "/tools/codex"
		let appCandidate = "/Applications/ChatGPT.app/Contents/Resources/codex"

		XCTAssertEqual(
			try DecodexAppBridge.codexExecutablePath(
				environment: [
					"CODEX_CLI_PATH": "  \(override)\n",
					"PATH": "/tools",
				],
				applicationResourceURL: URL(fileURLWithPath: appCandidate),
				isExecutableFile: { [override, pathCandidate, appCandidate].contains($0) }
			),
			override
		)
	}

	func testCodexExecutableUsesApplicationResourceBeforePath() throws {
		let pathCandidate = "/tools/codex"
		let appCandidate = "/Applications/ChatGPT.app/Contents/Resources/codex"

		XCTAssertEqual(
			try DecodexAppBridge.codexExecutablePath(
				environment: ["PATH": "/usr/bin:/tools:/bin"],
				applicationResourceURL: URL(fileURLWithPath: appCandidate),
				isExecutableFile: { [pathCandidate, appCandidate].contains($0) }
			),
			appCandidate
		)
	}

	func testBlankCodexExecutableOverrideDoesNotBlockAutomaticDiscovery() throws {
		let appCandidate = "/Applications/ChatGPT.app/Contents/Resources/codex"

		XCTAssertEqual(
			try DecodexAppBridge.codexExecutablePath(
				environment: [
					"CODEX_CLI_PATH": "  \n",
					"PATH": "/tools",
				],
				applicationResourceURL: URL(fileURLWithPath: appCandidate),
				isExecutableFile: { $0 == appCandidate }
			),
			appCandidate
		)
	}

	func testCodexExecutableUsesPathWhenApplicationResourceIsUnavailable() throws {
		let pathCandidate = "/tools/codex"
		let appCandidate = "/Applications/ChatGPT.app/Contents/Resources/codex"

		XCTAssertEqual(
			try DecodexAppBridge.codexExecutablePath(
				environment: ["PATH": "/usr/bin:/tools:/bin"],
				applicationResourceURL: URL(fileURLWithPath: appCandidate),
				isExecutableFile: { $0 == pathCandidate }
			),
			pathCandidate
		)
	}

	func testCodexExecutableUsesApplicationResourceWithMinimalPath() throws {
		let appCandidate = "/Applications/ChatGPT.app/Contents/Resources/codex"

		XCTAssertEqual(
			try DecodexAppBridge.codexExecutablePath(
				environment: ["PATH": "/usr/bin:/bin:/usr/sbin:/sbin"],
				applicationResourceURL: URL(fileURLWithPath: appCandidate),
				isExecutableFile: { $0 == appCandidate }
			),
			appCandidate
		)
	}

	func testInvalidCodexExecutableOverrideFailsClearly() {
		XCTAssertThrowsError(
			try DecodexAppBridge.codexExecutablePath(
				environment: [
					"CODEX_CLI_PATH": "/missing/codex",
					"PATH": "/tools",
				],
				applicationResourceURL: nil,
				isExecutableFile: { $0 == "/tools/codex" }
			)
		) { error in
			XCTAssertEqual(
				error.localizedDescription,
				"CODEX_CLI_PATH does not point to an executable Codex CLI."
			)
		}
	}

	func testCodexExecutableOverrideRejectsDirectory() throws {
		let directory = FileManager.default.temporaryDirectory
			.appendingPathComponent(UUID().uuidString, isDirectory: true)
		try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: false)
		defer { try? FileManager.default.removeItem(at: directory) }

		XCTAssertThrowsError(
			try DecodexAppBridge.codexExecutablePath(
				environment: ["CODEX_CLI_PATH": directory.path],
				applicationResourceURL: nil,
				isExecutableFile: { $0 == directory.path }
			)
		) { error in
			XCTAssertEqual(
				error.localizedDescription,
				"CODEX_CLI_PATH does not point to an executable Codex CLI."
			)
		}
	}

	func testMissingCodexExecutableFailsClearly() {
		XCTAssertThrowsError(
			try DecodexAppBridge.codexExecutablePath(
				environment: ["PATH": "/usr/bin:/bin"],
				applicationResourceURL: nil,
				isExecutableFile: { _ in false }
			)
		) { error in
			XCTAssertEqual(
				error.localizedDescription,
				"Codex CLI executable was not found. Install the Codex app, add codex to PATH, or set CODEX_CLI_PATH to its executable path."
			)
		}
	}

	func testAccountLoginRequestEncodesCodexExecutableAsOneValue() throws {
		let codexBin = "/Applications/Codex Preview.app/Contents/Resources/codex"
		let data = try JSONEncoder().encode(AppBridgeRequest.accountLogin(codexBin: codexBin))
		let payload = try XCTUnwrap(JSONSerialization.jsonObject(with: data) as? [String: Any])

		XCTAssertEqual(payload["operation"] as? String, "account_login")
		XCTAssertEqual(payload["codex_bin"] as? String, codexBin)
		XCTAssertEqual(payload["include_usage"] as? Bool, true)
	}
}
