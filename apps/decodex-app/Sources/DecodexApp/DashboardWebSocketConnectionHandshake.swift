import Foundation

extension DashboardWebSocketConnection {
	func sendHandshake(host: String, port: Int) async throws {
		let key = websocketKey()
		let path = websocketRequestPath()

		try await send(Self.handshakeRequest(host: host, port: port, path: path, key: key))
	}

	func readHandshakeResponse() async throws {
		let delimiter = Data("\r\n\r\n".utf8)

		while buffer.range(of: delimiter) == nil {
			try await receiveMore()
		}

		guard let range = buffer.range(of: delimiter) else {
			throw DecodexAppBridgeError.invalidResponse("dashboard WebSocket handshake response is incomplete")
		}

		let headerData = buffer[..<range.upperBound]
		buffer.removeSubrange(..<range.upperBound)

		let header = String(decoding: headerData, as: UTF8.self)
		guard header.hasPrefix("HTTP/1.1 101") || header.hasPrefix("HTTP/1.0 101") else {
			throw DecodexAppBridgeError.invalidResponse("dashboard WebSocket handshake failed: \(header)")
		}
	}

	private func websocketRequestPath() -> String {
		var path = url.path.isEmpty ? "/" : url.path
		if let query = url.query, query.isEmpty == false {
			path += "?\(query)"
		}

		return path
	}

	private func websocketKey() -> String {
		randomData(byteCount: 16).base64EncodedString()
	}
}
