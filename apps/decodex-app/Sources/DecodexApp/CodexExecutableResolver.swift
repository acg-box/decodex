import AppKit
import Foundation

enum CodexExecutableResolutionError: Error, Equatable, LocalizedError {
	case invalidOverride
	case unavailable

	var errorDescription: String? {
		switch self {
		case .invalidOverride:
			return "CODEX_CLI_PATH does not point to an executable Codex login tool."
		case .unavailable:
			return "Codex is not installed. Install the Codex app, add codex to PATH, or set CODEX_CLI_PATH."
		}
	}
}

enum CodexExecutableResolver {
	static let applicationBundleIdentifier = "com.openai.codex"
	static let overrideEnvironmentKey = "CODEX_CLI_PATH"

	@MainActor
	static func resolve() throws -> String {
		let applicationResourceURL = NSWorkspace.shared.urlForApplication(
			withBundleIdentifier: applicationBundleIdentifier
		).flatMap { applicationURL in
			Bundle(url: applicationURL)?.url(forResource: "codex", withExtension: nil)
		}
		return try resolve(
			environment: ProcessInfo.processInfo.environment,
			applicationResourceURL: applicationResourceURL,
			isExecutableFile: FileManager.default.isExecutableFile(atPath:)
		)
	}

	static func resolve(
		environment: [String: String],
		applicationResourceURL: URL?,
		isExecutableFile: (String) -> Bool
	) throws -> String {
		if let override = environment[overrideEnvironmentKey]?
			.trimmingCharacters(in: .whitespacesAndNewlines),
			override.isEmpty == false
		{
			guard let path = executablePath(
				override,
				isExecutableFile: isExecutableFile
			) else {
				throw CodexExecutableResolutionError.invalidOverride
			}
			return path
		}

		if let applicationResourceURL,
			let path = executablePath(
				applicationResourceURL.path,
				isExecutableFile: isExecutableFile
			)
		{
			return path
		}

		if let searchPath = environment["PATH"] {
			for directory in searchPath.split(
				separator: ":",
				omittingEmptySubsequences: true
			) {
				let candidate = URL(
					fileURLWithPath: String(directory),
					isDirectory: true
				).appendingPathComponent("codex")
				if let path = executablePath(
					candidate.path,
					isExecutableFile: isExecutableFile
				) {
					return path
				}
			}
		}

		throw CodexExecutableResolutionError.unavailable
	}

	private static func executablePath(
		_ value: String,
		isExecutableFile: (String) -> Bool
	) -> String? {
		guard value.hasPrefix("/") else {
			return nil
		}
		let inputURL = URL(fileURLWithPath: value).standardizedFileURL
		guard inputURL.path == value else {
			return nil
		}
		let executableURL = inputURL
			.resolvingSymlinksInPath()
			.standardizedFileURL
		guard executableURL.path.hasPrefix("/"),
			isExecutableFile(executableURL.path)
		else {
			return nil
		}
		return executableURL.path
	}
}
