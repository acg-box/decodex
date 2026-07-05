import Foundation
import Network
import Security


actor DashboardWebSocketConnection {
	let url: URL
	var connection: NWConnection?
	var buffer = Data()

	init(url: URL) {
		self.url = url
	}

	func connect() async throws {
		guard let host = url.host, let portValue = url.port else {
			throw DecodexAppBridgeError.invalidResponse("dashboard WebSocket URL is missing host or port")
		}
		guard let port = NWEndpoint.Port(rawValue: UInt16(portValue)) else {
			throw DecodexAppBridgeError.invalidResponse("dashboard WebSocket URL port is invalid")
		}

		let connection = NWConnection(host: NWEndpoint.Host(host), port: port, using: .tcp)
		self.connection = connection

		try await start(connection)
		try await sendHandshake(host: host, port: portValue)
		try await readHandshakeResponse()
	}

	func close() {
		connection?.cancel()
		connection = nil
		buffer.removeAll(keepingCapacity: false)
	}

	func readMessageData() async throws -> Data {
		while true {
			let frame = try await readFrame()

			switch frame.opcode {
			case 0x1, 0x2:
				return frame.payload
			case 0x8:
				throw DecodexAppBridgeError.invalidResponse("dashboard WebSocket closed")
			case 0x9:
				try await sendPong(frame.payload)
			case 0xA:
				continue
			default:
				continue
			}
		}
	}

	static func handshakeRequest(host: String, port: Int, path: String, key: String) -> Data {
		let lines = [
			"GET \(path) HTTP/1.1",
			"Host: \(host):\(port)",
			"Upgrade: websocket",
			"Connection: Upgrade",
			"Sec-WebSocket-Key: \(key)",
			"Sec-WebSocket-Version: 13",
			"",
			"",
		]

		return Data(lines.joined(separator: "\r\n").utf8)
	}
}

extension DashboardWebSocketConnection {
	func randomData(byteCount: Int) -> Data {
		var bytes = [UInt8](repeating: 0, count: byteCount)
		let status = SecRandomCopyBytes(kSecRandomDefault, bytes.count, &bytes)
		if status != errSecSuccess {
			for index in bytes.indices {
				bytes[index] = UInt8.random(in: UInt8.min...UInt8.max)
			}
		}

		return Data(bytes)
	}
}
