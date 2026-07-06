import Foundation

extension DecodexServerBridge {
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
}
