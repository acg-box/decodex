import AppKit
import Foundation

enum DecodexAppBridgeError: LocalizedError {
	case helperMissing(String)
	case launchFailed(String)
	case commandFailed(Int32, String)
	case invalidResponse(String)

	var errorDescription: String? {
		switch self {
		case .helperMissing(let message):
			return message
		case .launchFailed(let message):
			return message
		case .commandFailed(let code, let message):
			return "Decodex App bridge command failed with status \(code): \(message)"
		case .invalidResponse(let message):
			return "Invalid Decodex App bridge response: \(message)"
		}
	}
}

struct DecodexAppBridge: Sendable {
	static let codexApplicationBundleIdentifier = "com.openai.codex"
	static let codexExecutableOverrideKey = "CODEX_CLI_PATH"

	func runJSON<T: Decodable & Sendable>(_ request: AppBridgeRequest, as type: T.Type) async throws -> T {
		try await runStreaming(request, as: type, onOutput: nil)
	}

	func runStreaming<T: Decodable & Sendable>(
		_ request: AppBridgeRequest,
		as type: T.Type,
		onOutput: (@MainActor @Sendable (String) -> Void)?
	) async throws -> T {
		if onOutput == nil, try request.serverRoute() != nil {
			return try await DecodexServerBridge.shared.run(request, as: type)
		}
		guard request.requiresHelper else {
			throw DecodexAppBridgeError.invalidResponse(
				"operation \(request.operation) must be served by Decodex server"
			)
		}

		let helperURL = try helperExecutableURL()
		let requestData = try JSONEncoder().encode(request)

		return try await Task.detached(priority: .userInitiated) {
			let process = Process()
			let inputPipe = Pipe()
			let outputPipe = Pipe()
			let errorPipe = Pipe()
			let parser = AppBridgeEventParser<T>(onOutput: onOutput)

			process.executableURL = helperURL
			process.standardInput = inputPipe
			process.standardOutput = outputPipe
			process.standardError = errorPipe

			do {
				try process.run()
			} catch {
				throw DecodexAppBridgeError.launchFailed(error.localizedDescription)
			}

			inputPipe.fileHandleForWriting.write(requestData)
			inputPipe.fileHandleForWriting.write(Data([0x0a]))
			try inputPipe.fileHandleForWriting.close()

			while true {
				let data = outputPipe.fileHandleForReading.availableData
				if data.isEmpty {
					break
				}

				try parser.append(data)
			}

			process.waitUntilExit()

			let stderr = String(
				decoding: errorPipe.fileHandleForReading.readDataToEndOfFile(),
				as: UTF8.self
			)

			if process.terminationStatus != 0 {
				do {
					return try parser.finish()
				} catch DecodexAppBridgeError.commandFailed(let code, let message) {
					throw DecodexAppBridgeError.commandFailed(code, message)
				} catch {
					throw DecodexAppBridgeError.commandFailed(
						process.terminationStatus,
						stderr.isEmpty ? error.localizedDescription : stderr
					)
				}
			}

			return try parser.finish()
		}
		.value
	}

	private func helperExecutableURL() throws -> URL {
		if let override = ProcessInfo.processInfo.environment["DECODEX_APP_HELPER"], override.isEmpty == false {
			let overrideURL = URL(fileURLWithPath: override)
			if FileManager.default.isExecutableFile(atPath: overrideURL.path) {
				return overrideURL
			}
		}

		let bundledURL = Bundle.main.bundleURL
			.appendingPathComponent("Contents")
			.appendingPathComponent("Helpers")
			.appendingPathComponent("decodex-app-helper")
		if FileManager.default.isExecutableFile(atPath: bundledURL.path) {
			return bundledURL
		}

		throw DecodexAppBridgeError.helperMissing(
			"Bundled Decodex App helper is missing. Rebuild the app bundle with apps/decodex-app/script/build_and_run.sh."
		)
	}

	@MainActor
	func codexExecutablePath() throws -> String {
		let applicationURL = NSWorkspace.shared.urlForApplication(
			withBundleIdentifier: Self.codexApplicationBundleIdentifier
		)
		let resourceURL = applicationURL.flatMap {
			Bundle(url: $0)?.url(forResource: "codex", withExtension: nil)
		}

		return try Self.codexExecutablePath(
			environment: ProcessInfo.processInfo.environment,
			applicationResourceURL: resourceURL,
			isExecutableFile: FileManager.default.isExecutableFile(atPath:)
		)
	}

	static func codexExecutablePath(
		environment: [String: String],
		applicationResourceURL: URL?,
		isExecutableFile: (String) -> Bool
	) throws -> String {
		if let override = environment[codexExecutableOverrideKey]?
			.trimmingCharacters(in: .whitespacesAndNewlines),
			override.isEmpty == false
		{
			guard let path = executablePath(override, isExecutableFile: isExecutableFile) else {
				throw DecodexAppBridgeError.helperMissing(
					"CODEX_CLI_PATH does not point to an executable Codex CLI."
				)
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
			for directory in searchPath.split(separator: ":", omittingEmptySubsequences: true) {
				let candidate = URL(fileURLWithPath: String(directory), isDirectory: true)
					.appendingPathComponent("codex")
				if let path = executablePath(candidate.path, isExecutableFile: isExecutableFile) {
					return path
				}
			}
		}

		throw DecodexAppBridgeError.helperMissing(
			"Codex CLI executable was not found. Install the Codex app, add codex to PATH, or set CODEX_CLI_PATH to its executable path."
		)
	}

	private static func executablePath(
		_ path: String,
		isExecutableFile: (String) -> Bool
	) -> String? {
		let standardizedURL = URL(fileURLWithPath: path).standardizedFileURL
		let resourceValues = try? standardizedURL.resourceValues(forKeys: [.isDirectoryKey])
		guard resourceValues?.isDirectory != true else {
			return nil
		}

		return isExecutableFile(standardizedURL.path) ? standardizedURL.path : nil
	}
}
