@testable import DecodexApp
import Foundation
import XCTest

final class CodexExecutableResolverTests: XCTestCase {
	func testExplicitOverridePrecedesInstalledApplicationAndPath() throws {
		let executables = Set([
			"/private/override/codex",
			"/Applications/ChatGPT.app/Contents/Resources/codex",
			"/usr/local/bin/codex",
		])

		let resolved = try CodexExecutableResolver.resolve(
			environment: [
				"CODEX_CLI_PATH": "/private/override/codex",
				"PATH": "/usr/local/bin",
			],
			applicationResourceURL: URL(
				fileURLWithPath: "/Applications/ChatGPT.app/Contents/Resources/codex"
			),
			isExecutableFile: executables.contains
		)

		XCTAssertEqual(resolved, "/private/override/codex")
	}

	func testInstalledApplicationPrecedesPathFallback() throws {
		let executables = Set([
			"/Applications/ChatGPT.app/Contents/Resources/codex",
			"/usr/local/bin/codex",
		])

		let resolved = try CodexExecutableResolver.resolve(
			environment: ["PATH": "/usr/local/bin"],
			applicationResourceURL: URL(
				fileURLWithPath: "/Applications/ChatGPT.app/Contents/Resources/codex"
			),
			isExecutableFile: executables.contains
		)

		XCTAssertEqual(
			resolved,
			"/Applications/ChatGPT.app/Contents/Resources/codex"
		)
	}

	func testPathFallbackUsesFirstExecutableCodex() throws {
		let resolved = try CodexExecutableResolver.resolve(
			environment: ["PATH": "/not-installed:/opt/homebrew/bin:/usr/local/bin"],
			applicationResourceURL: nil,
			isExecutableFile: { $0 == "/opt/homebrew/bin/codex" }
		)

		XCTAssertEqual(resolved, "/opt/homebrew/bin/codex")
	}

	func testPathFallbackReturnsTheResolvedExecutableInsteadOfASymlink() throws {
		let root = FileManager.default.temporaryDirectory
			.appendingPathComponent(UUID().uuidString, isDirectory: true)
		defer {
			try? FileManager.default.removeItem(at: root)
		}
		try FileManager.default.createDirectory(
			at: root,
			withIntermediateDirectories: true
		)
		let executable = root.appendingPathComponent("codex-real")
		try Data().write(to: executable)
		let symlink = root.appendingPathComponent("codex")
		try FileManager.default.createSymbolicLink(
			at: symlink,
			withDestinationURL: executable
		)
		let expected = executable
			.resolvingSymlinksInPath()
			.standardizedFileURL
			.path

		let resolved = try CodexExecutableResolver.resolve(
			environment: ["PATH": root.path],
			applicationResourceURL: nil,
			isExecutableFile: { $0 == expected }
		)

		XCTAssertEqual(resolved, expected)
	}

	func testInvalidExplicitOverrideFailsWithoutFallback() {
		XCTAssertThrowsError(
			try CodexExecutableResolver.resolve(
				environment: [
					"CODEX_CLI_PATH": "relative/codex",
					"PATH": "/usr/local/bin",
				],
				applicationResourceURL: URL(
					fileURLWithPath: "/Applications/ChatGPT.app/Contents/Resources/codex"
				),
				isExecutableFile: { _ in true }
			)
		) { error in
			XCTAssertEqual(
				error as? CodexExecutableResolutionError,
				.invalidOverride
			)
		}
	}

	func testUnavailableWhenNoCandidateIsExecutable() {
		XCTAssertThrowsError(
			try CodexExecutableResolver.resolve(
				environment: ["PATH": "/usr/local/bin"],
				applicationResourceURL: nil,
				isExecutableFile: { _ in false }
			)
		) { error in
			XCTAssertEqual(
				error as? CodexExecutableResolutionError,
				.unavailable
			)
		}
	}
}
