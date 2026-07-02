import Foundation

enum DecodexServerProbe: Equatable {
	case live
	case reachable(String)
	case unreachable
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

	private func configuredServerURL() throws -> URL? {
		let value = ProcessInfo.processInfo.environment["DECODEX_APP_SERVER_URL"] ?? ""

		return try Self.configuredServerURL(from: value)
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
