import Foundation

struct ServerRoute {
	let method: String
	let path: String
	let body: Data?
}

private enum DecodexServerProbe: Equatable {
	case live
	case reachable(String)
	case unreachable
}

actor DecodexServerBridge {
	static let shared = DecodexServerBridge()

	private let defaultBaseURL = URL(string: "http://127.0.0.1:8912")!
	private let defaultListenAddress = "127.0.0.1:8912"
	private let liveCheckFreshness: TimeInterval = 5
	private var serverBaseURL: URL?
	private var liveCheckBaseURL: URL?
	private var liveCheckedAt: Date?
	private var startedProcess: Process?

	func dashboardURL() async throws -> URL {
		let baseURL = try await ensureServer()

		return baseURL.appendingPathComponent("dashboard")
	}

	func dashboardWebSocketURL() async throws -> URL {
		let baseURL = try await ensureServer()
		guard var components = URLComponents(url: baseURL, resolvingAgainstBaseURL: false) else {
			throw DecodexAppBridgeError.invalidResponse("Decodex server URL is invalid")
		}

		components.scheme = baseURL.scheme == "https" ? "wss" : "ws"
		components.path = "/dashboard/control"
		components.query = nil

		guard let url = components.url else {
			throw DecodexAppBridgeError.invalidResponse("Decodex dashboard WebSocket URL is invalid")
		}

		return url
	}

	func run<T: Decodable & Sendable>(_ request: AppBridgeRequest, as type: T.Type) async throws -> T {
		guard let route = try request.serverRoute() else {
			throw DecodexAppBridgeError.invalidResponse("request is not supported by Decodex server")
		}

		let baseURL = try await ensureServer()
		do {
			return try await send(route, baseURL: baseURL, as: type)
		} catch let error as DecodexAppBridgeError {
			throw error
		} catch let error as DecodingError {
			throw error
		} catch {
			clearLive(baseURL)

			return try await send(route, baseURL: try await ensureServer(), as: type)
		}
	}

	private func send<T: Decodable & Sendable>(
		_ route: ServerRoute,
		baseURL: URL,
		as type: T.Type
	) async throws -> T {
		let url = routeURL(baseURL: baseURL, route: route)
		var urlRequest = URLRequest(url: url)

		urlRequest.httpMethod = route.method
		urlRequest.timeoutInterval = 15
		if let body = route.body {
			urlRequest.httpBody = body
			urlRequest.setValue("application/json", forHTTPHeaderField: "Content-Type")
		}

		let (data, response) = try await URLSession.shared.data(for: urlRequest)
		guard let httpResponse = response as? HTTPURLResponse else {
			throw DecodexAppBridgeError.invalidResponse("Decodex server returned a non-HTTP response")
		}
		if httpResponse.statusCode == 404, route.path.hasPrefix("api/accounts") {
			throw DecodexAppBridgeError.invalidResponse(
				"Decodex server at \(baseURL.absoluteString) does not support /api/accounts. Restart decodex serve with the bundled app version."
			)
		}
		guard 200..<300 ~= httpResponse.statusCode else {
			let message = String(decoding: data, as: UTF8.self)
			throw DecodexAppBridgeError.commandFailed(
				Int32(httpResponse.statusCode),
				message.isEmpty ? "Decodex server request failed" : message
			)
		}

		noteLive(baseURL)

		return try JSONDecoder().decode(type, from: data)
	}

	private func ensureServer() async throws -> URL {
		if let serverBaseURL, hasFreshLiveCheck(serverBaseURL) {
			return serverBaseURL
		}

		if let serverBaseURL, await probeServer(serverBaseURL) == .live {
			noteLive(serverBaseURL)

			return serverBaseURL
		}

		if let configured = configuredServerURL() {
			switch await probeServer(configured) {
			case .live:
				noteLive(configured)

				return configured
			case .reachable(let reason):
				throw DecodexAppBridgeError.invalidResponse(
					"Decodex server at \(configured.absoluteString) is reachable but not app-compatible: \(reason)"
				)
			case .unreachable:
				break
			}
		}

		switch await probeServer(defaultBaseURL) {
		case .live:
			noteLive(defaultBaseURL)

			return defaultBaseURL
		case .reachable(let reason):
			throw DecodexAppBridgeError.invalidResponse(
				"Port \(defaultListenAddress) already has a server, but it is not app-compatible: \(reason). Decodex App will not start its bundled server over an existing process."
			)
		case .unreachable:
			break
		}

		try startBundledServer()

		for _ in 0..<40 {
			if await probeServer(defaultBaseURL) == .live {
				noteLive(defaultBaseURL)

				return defaultBaseURL
			}

			try await Task.sleep(nanoseconds: 100_000_000)
		}

		throw DecodexAppBridgeError.launchFailed("Decodex server did not become ready on \(defaultListenAddress)")
	}

	private func configuredServerURL() -> URL? {
		let value = ProcessInfo.processInfo.environment["DECODEX_APP_SERVER_URL"] ?? ""

		return value.isEmpty ? nil : URL(string: value)
	}

	private func hasFreshLiveCheck(_ baseURL: URL) -> Bool {
		guard liveCheckBaseURL == baseURL, let liveCheckedAt else {
			return false
		}

		return Date().timeIntervalSince(liveCheckedAt) < liveCheckFreshness
	}

	private func noteLive(_ baseURL: URL) {
		serverBaseURL = baseURL
		liveCheckBaseURL = baseURL
		liveCheckedAt = Date()
	}

	private func clearLive(_ baseURL: URL) {
		if serverBaseURL == baseURL {
			serverBaseURL = nil
		}
		if liveCheckBaseURL == baseURL {
			liveCheckBaseURL = nil
			liveCheckedAt = nil
		}
	}

	private func isLive(_ baseURL: URL) async -> Bool {
		await probeServer(baseURL) == .live
	}

	private func probeServer(_ baseURL: URL) async -> DecodexServerProbe {
		let url = baseURL.appendingPathComponent("livez")
		var request = URLRequest(url: url)

		request.timeoutInterval = 0.75

		do {
			let (data, response) = try await URLSession.shared.data(for: request)
			guard let httpResponse = response as? HTTPURLResponse else {
				return .reachable("non-HTTP response from /livez")
			}
			let body = String(decoding: data, as: UTF8.self)
			if httpResponse.statusCode == 200 && body.trimmingCharacters(in: .whitespacesAndNewlines) == "ok" {
				return .live
			}

			return .reachable("/livez returned HTTP \(httpResponse.statusCode)")
		} catch {
			return .unreachable
		}
	}

	private func startBundledServer() throws {
		if let startedProcess, startedProcess.isRunning {
			return
		}

		let process = Process()
		let nullDevice = FileHandle(forWritingAtPath: "/dev/null")

		process.executableURL = try decodexExecutableURL()
		process.arguments = [
			"serve",
			"--api-only",
			"--listen-address", defaultListenAddress,
		]
		process.standardOutput = nullDevice
		process.standardError = nullDevice

		do {
			try process.run()
		} catch {
			throw DecodexAppBridgeError.launchFailed(error.localizedDescription)
		}

		startedProcess = process
	}

	private func decodexExecutableURL() throws -> URL {
		if let override = ProcessInfo.processInfo.environment["DECODEX_APP_DECODEX"], !override.isEmpty {
			let overrideURL = URL(fileURLWithPath: override)
			if FileManager.default.isExecutableFile(atPath: overrideURL.path) {
				return overrideURL
			}
		}

		let bundledURL = Bundle.main.bundleURL
			.appendingPathComponent("Contents")
			.appendingPathComponent("Helpers")
			.appendingPathComponent("decodex")
		if FileManager.default.isExecutableFile(atPath: bundledURL.path) {
			return bundledURL
		}

		throw DecodexAppBridgeError.helperMissing(
			"Bundled decodex server is missing. Rebuild the app bundle with apps/decodex-app/script/build_and_run.sh."
		)
	}

	private func routeURL(baseURL: URL, route: ServerRoute) -> URL {
		let parts = route.path.split(separator: "?", maxSplits: 1).map(String.init)
		var components = URLComponents(url: baseURL, resolvingAgainstBaseURL: false)!

		components.path = "/" + parts[0]
		if parts.count == 2 {
			components.query = parts[1]
		}

		return components.url!
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
