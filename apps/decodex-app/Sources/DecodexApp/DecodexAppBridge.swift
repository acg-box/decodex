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

struct AppBridgeRequest: Encodable, Sendable {
	let operation: String
	let selector: String?
	let authJsonPath: String?
	let codexBin: String?
	let keepTempHome: Bool?
	let includeUsage: Bool?
	let forceRefresh: Bool?
	let enabled: Bool?

	enum CodingKeys: String, CodingKey {
		case operation
		case selector
		case authJsonPath = "auth_json_path"
		case codexBin = "codex_bin"
		case keepTempHome = "keep_temp_home"
		case includeUsage = "include_usage"
		case forceRefresh = "force_refresh"
		case enabled
	}

	static func accountList(forceRefresh: Bool = false) -> AppBridgeRequest {
		AppBridgeRequest(
			operation: "account_list",
			includeUsage: true,
			forceRefresh: forceRefresh
		)
	}

	static let accountClear = AppBridgeRequest(operation: "account_clear", includeUsage: true)

	static func accountUse(selector: String) -> AppBridgeRequest {
		AppBridgeRequest(operation: "account_use", selector: selector)
	}

	static func accountSelect(selector: String) -> AppBridgeRequest {
		AppBridgeRequest(operation: "account_select", selector: selector, includeUsage: true)
	}

	static func accountLogout(selector: String) -> AppBridgeRequest {
		AppBridgeRequest(operation: "account_logout", selector: selector, includeUsage: true)
	}

	static func accountLogin() -> AppBridgeRequest {
		AppBridgeRequest(operation: "account_login", includeUsage: true)
	}

	static let codexFastModeStatus = AppBridgeRequest(operation: "codex_fast_mode_status")

	static func codexFastModeSet(enabled: Bool) -> AppBridgeRequest {
		AppBridgeRequest(operation: "codex_fast_mode_set", enabled: enabled)
	}

	private init(
		operation: String,
		selector: String? = nil,
		authJsonPath: String? = nil,
		codexBin: String? = nil,
		keepTempHome: Bool? = nil,
		includeUsage: Bool? = nil,
		forceRefresh: Bool? = nil,
		enabled: Bool? = nil
	) {
		self.operation = operation
		self.selector = selector
		self.authJsonPath = authJsonPath
		self.codexBin = codexBin
		self.keepTempHome = keepTempHome
		self.includeUsage = includeUsage
		self.forceRefresh = forceRefresh
		self.enabled = enabled
	}
}

extension AppBridgeRequest {
	func serverRoute() throws -> ServerRoute? {
		switch operation {
		case "account_list":
			let suffix = forceRefresh == true ? "?refresh=1" : ""

			return ServerRoute(method: "GET", path: "api/accounts\(suffix)", body: nil)
		case "account_select":
			return try jsonPost("api/accounts/select")
		case "account_clear":
			return try jsonPost("api/accounts/clear")
		case "account_logout":
			return try jsonPost("api/accounts/logout")
		case "account_import":
			return try jsonPost("api/accounts/import")
		case "account_use":
			return try jsonPost("api/accounts/use")
		default:
			return nil
		}
	}

	private func jsonPost(_ path: String) throws -> ServerRoute {
		ServerRoute(method: "POST", path: path, body: try JSONEncoder().encode(self))
	}
}

extension AppBridgeRequest {
	var requiresHelper: Bool {
		switch operation {
		case
			"account_import",
			"account_login",
			"codex_fast_mode_status",
			"codex_fast_mode_set":
			return true
		default:
			return false
		}
	}
}

private struct AppBridgeEvent<Response: Decodable>: Decodable {
	let kind: String
	let text: String?
	let payload: Response?
	let message: String?
}

final class AppBridgeEventParser<Response: Decodable>: @unchecked Sendable {
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
		guard data.isEmpty == false else {
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

		if remainder.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty == false {
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
		guard trimmed.isEmpty == false else {
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
}
