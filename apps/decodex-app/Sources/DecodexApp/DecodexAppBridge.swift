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
			return "Decodex App helper exited with status \(code): \(message)"
		case .invalidResponse(let message):
			return "Invalid Decodex App helper response: \(message)"
		}
	}
}

struct AppBridgeRequest: Encodable, Sendable {
	let operation: String
	let selector: String?
	let authJsonPath: String?
	let codexBin: String?
	let keepTempHome: Bool?

	enum CodingKeys: String, CodingKey {
		case operation
		case selector
		case authJsonPath = "auth_json_path"
		case codexBin = "codex_bin"
		case keepTempHome = "keep_temp_home"
	}

	static let accountList = AppBridgeRequest(operation: "account_list")
	static let accountClear = AppBridgeRequest(operation: "account_clear")

	static func accountUse(selector: String) -> AppBridgeRequest {
		AppBridgeRequest(operation: "account_use", selector: selector)
	}

	static func accountSelect(selector: String) -> AppBridgeRequest {
		AppBridgeRequest(operation: "account_select", selector: selector)
	}

	static func accountLogout(selector: String) -> AppBridgeRequest {
		AppBridgeRequest(operation: "account_logout", selector: selector)
	}

	static func accountLogin() -> AppBridgeRequest {
		AppBridgeRequest(operation: "account_login")
	}

	private init(
		operation: String,
		selector: String? = nil,
		authJsonPath: String? = nil,
		codexBin: String? = nil,
		keepTempHome: Bool? = nil
	) {
		self.operation = operation
		self.selector = selector
		self.authJsonPath = authJsonPath
		self.codexBin = codexBin
		self.keepTempHome = keepTempHome
	}
}

private struct AppBridgeEvent<Response: Decodable>: Decodable {
	let kind: String
	let text: String?
	let payload: Response?
	let message: String?
}

private final class AppBridgeEventParser<Response: Decodable>: @unchecked Sendable {
	private let decoder = JSONDecoder()
	private let lock = NSLock()
	private let onOutput: (@MainActor @Sendable (String) -> Void)?
	private var buffer = ""
	private var response: Response?
	private var bridgeError: String?

	init(onOutput: (@MainActor @Sendable (String) -> Void)? = nil) {
		self.onOutput = onOutput
	}

	func append(_ data: Data) throws {
		guard !data.isEmpty else {
			return
		}

		lock.lock()
		buffer += String(decoding: data, as: UTF8.self)
		let lines = completeLines()
		lock.unlock()

		for line in lines {
			try handle(line)
		}
	}

	func finish() throws -> Response {
		lock.lock()
		let remainder = buffer
		buffer = ""
		lock.unlock()

		if !remainder.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
			try handle(remainder)
		}

		if let bridgeError {
			throw DecodexAppBridgeError.commandFailed(1, bridgeError)
		}
		guard let response else {
			throw DecodexAppBridgeError.invalidResponse("missing result event")
		}

		return response
	}

	private func completeLines() -> [String] {
		var lines: [String] = []

		while let newlineIndex = buffer.firstIndex(of: "\n") {
			lines.append(String(buffer[..<newlineIndex]))
			buffer.removeSubrange(...newlineIndex)
		}

		return lines
	}

	private func handle(_ line: String) throws {
		let trimmed = line.trimmingCharacters(in: .whitespacesAndNewlines)
		guard !trimmed.isEmpty else {
			return
		}
		guard let data = trimmed.data(using: .utf8) else {
			throw DecodexAppBridgeError.invalidResponse("event is not UTF-8")
		}

		let event = try decoder.decode(AppBridgeEvent<Response>.self, from: data)

		switch event.kind {
		case "output":
			if let text = event.text {
				Task { @MainActor in
					onOutput?(text)
				}
			}
		case "result":
			guard let payload = event.payload else {
				throw DecodexAppBridgeError.invalidResponse("result event omitted payload")
			}
			response = payload
		case "error":
			bridgeError = event.message ?? "unknown helper error"
		default:
			throw DecodexAppBridgeError.invalidResponse("unknown event kind \(event.kind)")
		}
	}
}

struct DecodexAppBridge: Sendable {
	func runJSON<T: Decodable & Sendable>(_ request: AppBridgeRequest, as type: T.Type) async throws -> T {
		try await runStreaming(request, as: type, onOutput: nil)
	}

	func runStreaming<T: Decodable & Sendable>(
		_ request: AppBridgeRequest,
		as type: T.Type,
		onOutput: (@MainActor @Sendable (String) -> Void)?
	) async throws -> T {
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
		if let override = ProcessInfo.processInfo.environment["DECODEX_APP_HELPER"], !override.isEmpty {
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
}
