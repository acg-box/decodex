import Foundation

struct ServerRoute {
	let method: String
	let path: String
	let body: Data?
}

actor DecodexServerBridge {
	static let shared = DecodexServerBridge()

	private let defaultBaseURL = URL(string: "http://127.0.0.1:8912")!
	private let defaultListenAddress = "127.0.0.1:8912"
	private var serverBaseURL: URL?
	private var startedProcess: Process?

	func run<T: Decodable & Sendable>(_ request: AppBridgeRequest, as type: T.Type) async throws -> T {
		guard let route = try request.serverRoute() else {
			throw DecodexAppBridgeError.invalidResponse("request is not supported by Decodex server")
		}

		let baseURL = try await ensureServer()
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

		return try JSONDecoder().decode(type, from: data)
	}

	private func ensureServer() async throws -> URL {
		if let serverBaseURL, await isLive(serverBaseURL) {
			return serverBaseURL
		}

		if let configured = configuredServerURL(), await isLive(configured) {
			serverBaseURL = configured

			return configured
		}

		if await isLive(defaultBaseURL) {
			serverBaseURL = defaultBaseURL

			return defaultBaseURL
		}

		try startBundledServer()

		for _ in 0..<40 {
			if await isLive(defaultBaseURL) {
				serverBaseURL = defaultBaseURL

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

	private func isLive(_ baseURL: URL) async -> Bool {
		let url = baseURL.appendingPathComponent("livez")
		var request = URLRequest(url: url)

		request.timeoutInterval = 0.75

		do {
			let (data, response) = try await URLSession.shared.data(for: request)
			let body = String(decoding: data, as: UTF8.self)

			return (response as? HTTPURLResponse)?.statusCode == 200 && body == "ok"
		} catch {
			return false
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
			"--interval", "30s",
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
