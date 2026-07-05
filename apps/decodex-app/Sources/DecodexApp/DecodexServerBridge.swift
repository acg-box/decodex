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

	let defaultBaseURL = DecodexServerBridge.makeDefaultBaseURL()
	let defaultListenAddress = "127.0.0.1:8192"
	let liveCheckFreshness: TimeInterval = 5
	var serverBaseURL: URL?
	var liveCheckBaseURL: URL?
	var liveCheckedAt: Date?
	var startedProcess: Process?

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

	static func configuredServerURL(from value: String) throws -> URL? {
		let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
		guard trimmed.isEmpty == false else {
			return nil
		}
		guard let url = URL(string: trimmed),
			let scheme = url.scheme?.lowercased(),
			(scheme == "http" || scheme == "https"),
			url.host != nil
		else {
			throw DecodexAppBridgeError.invalidResponse(
				"DECODEX_APP_SERVER_URL must be an absolute http(s) URL"
			)
		}

		return url
	}

	static func bundledServerArguments(listenAddress: String) -> [String] {
		[
			"serve",
			"--listen-address", listenAddress,
		]
	}

	static func makeDefaultBaseURL() -> URL {
		guard let url = URL(string: "http://127.0.0.1:8192") else {
			preconditionFailure("default Decodex server URL must be valid")
		}

		return url
	}

	private func configuredServerURL() throws -> URL? {
		let value = ProcessInfo.processInfo.environment["DECODEX_APP_SERVER_URL"] ?? ""

		return try Self.configuredServerURL(from: value)
	}

	private func decodexExecutableURL() throws -> URL {
		if let override = ProcessInfo.processInfo.environment["DECODEX_APP_DECODEX"], override.isEmpty == false {
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
}

extension DecodexServerBridge {
	func ensureServer() async throws -> URL {
		if let configured = try configuredServerURL() {
			if serverBaseURL == configured, hasFreshLiveCheck(configured) {
				return configured
			}
			switch await probeServer(configured) {
			case .live:
				noteLive(configured)

				return configured
			case .reachable(let reason):
				throw DecodexAppBridgeError.invalidResponse(
					"Decodex server at \(configured.absoluteString) is reachable but not app-compatible: \(reason)"
				)
			case .unreachable:
				throw DecodexAppBridgeError.invalidResponse(
					"Configured Decodex App server \(configured.absoluteString) is unreachable. Start that server or unset DECODEX_APP_SERVER_URL."
				)
			}
		}

		if let serverBaseURL, hasFreshLiveCheck(serverBaseURL) {
			return serverBaseURL
		}

		if let serverBaseURL, await probeServer(serverBaseURL) == .live {
			noteLive(serverBaseURL)

			return serverBaseURL
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

	func send<T: Decodable & Sendable>(
		_ route: ServerRoute,
		baseURL: URL,
		as type: T.Type
	) async throws -> T {
		let url = try routeURL(baseURL: baseURL, route: route)
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

	func startBundledServer() throws {
		if let startedProcess, startedProcess.isRunning {
			return
		}

		let process = Process()
		let nullDevice = FileHandle(forWritingAtPath: "/dev/null")

		process.executableURL = try decodexExecutableURL()
		process.arguments = Self.bundledServerArguments(listenAddress: defaultListenAddress)
		process.standardOutput = nullDevice
		process.standardError = nullDevice

		do {
			try process.run()
		} catch {
			throw DecodexAppBridgeError.launchFailed(error.localizedDescription)
		}

		startedProcess = process
	}

	func hasFreshLiveCheck(_ baseURL: URL) -> Bool {
		guard liveCheckBaseURL == baseURL, let liveCheckedAt else {
			return false
		}

		return Date().timeIntervalSince(liveCheckedAt) < liveCheckFreshness
	}

	func noteLive(_ baseURL: URL) {
		serverBaseURL = baseURL
		liveCheckBaseURL = baseURL
		liveCheckedAt = Date()
	}

	func clearLive(_ baseURL: URL) {
		if serverBaseURL == baseURL {
			serverBaseURL = nil
		}
		if liveCheckBaseURL == baseURL {
			liveCheckBaseURL = nil
			liveCheckedAt = nil
		}
	}

	private func routeURL(baseURL: URL, route: ServerRoute) throws -> URL {
		let parts = route.path.split(separator: "?", maxSplits: 1).map(String.init)
		guard var components = URLComponents(url: baseURL, resolvingAgainstBaseURL: false) else {
			throw DecodexAppBridgeError.invalidResponse("Decodex server URL is invalid")
		}

		components.path = "/" + parts[0]
		if parts.count == 2 {
			components.query = parts[1]
		}

		guard let url = components.url else {
			throw DecodexAppBridgeError.invalidResponse("Decodex server route URL is invalid")
		}

		return url
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
}
