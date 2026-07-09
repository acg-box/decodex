import Foundation

struct ServerRoute {
	let method: String
	let path: String
	let body: Data?
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

	func configuredServerURL() throws -> URL? {
		let value = ProcessInfo.processInfo.environment["DECODEX_APP_SERVER_URL"] ?? ""

		return try Self.configuredServerURL(from: value)
	}

	func decodexExecutableURL() throws -> URL {
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
