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

	func start(_ connection: NWConnection) async throws {
		try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
			let resumeBox = DashboardConnectionResumeBox(continuation: continuation)

			connection.stateUpdateHandler = { state in
				switch state {
				case .ready:
					resumeBox.resume(.success(()))
				case .failed(let error):
					resumeBox.resume(.failure(error))
				case .cancelled:
					resumeBox.resume(.failure(DecodexAppBridgeError.invalidResponse("dashboard WebSocket connection cancelled")))
				default:
					break
				}
			}
			connection.start(queue: .global(qos: .userInitiated))
		}
	}

	func receiveMore() async throws {
		guard let connection else {
			throw DecodexAppBridgeError.invalidResponse("dashboard WebSocket is not connected")
		}

		let data = try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Data, Error>) in
			connection.receive(minimumIncompleteLength: 1, maximumLength: 64 * 1_024) {
				data,
				_,
				isComplete,
				error in
				if let error {
					continuation.resume(throwing: error)
					return
				}
				if let data, data.isEmpty == false {
					continuation.resume(returning: data)
					return
				}
				if isComplete {
					continuation.resume(throwing: DecodexAppBridgeError.invalidResponse("dashboard WebSocket ended"))
					return
				}

				continuation.resume(returning: Data())
			}
		}

		if data.isEmpty {
			return
		}

		buffer.append(data)
	}

	func send(_ data: Data) async throws {
		guard let connection else {
			throw DecodexAppBridgeError.invalidResponse("dashboard WebSocket is not connected")
		}

		try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
			connection.send(content: data, completion: .contentProcessed { error in
				if let error {
					continuation.resume(throwing: error)
				} else {
					continuation.resume(returning: ())
				}
			})
		}
	}

	func readFrame() async throws -> WebSocketFrame {
		while buffer.count < 2 {
			try await receiveMore()
		}

		let firstByte = buffer[buffer.startIndex]
		let secondByte = buffer[buffer.index(after: buffer.startIndex)]
		let opcode = firstByte & 0x0F
		let masked = (secondByte & 0x80) != 0
		var length = UInt64(secondByte & 0x7F)
		var headerLength = 2

		if length == 126 {
			while buffer.count < 4 {
				try await receiveMore()
			}
			length = UInt64(readUInt16(offset: 2))
			headerLength = 4
		} else if length == 127 {
			while buffer.count < 10 {
				try await receiveMore()
			}
			length = readUInt64(offset: 2)
			headerLength = 10
		}

		let maskLength = masked ? 4 : 0
		let payloadLength = try checkedPayloadLength(length)
		let frameLength = headerLength + maskLength + payloadLength

		while buffer.count < frameLength {
			try await receiveMore()
		}

		let maskStart = headerLength
		let payloadStart = headerLength + maskLength
		var payload = buffer.subdata(in: payloadStart..<frameLength)

		if masked {
			let mask = buffer.subdata(in: maskStart..<maskStart + maskLength)
			for index in payload.indices {
				payload[index] ^= mask[index % 4]
			}
		}

		buffer.removeSubrange(..<frameLength)

		return WebSocketFrame(opcode: opcode, payload: payload)
	}

	func sendPong(_ payload: Data) async throws {
		try await send(clientFrame(opcode: 0xA, payload: payload))
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

	private func clientFrame(opcode: UInt8, payload: Data) -> Data {
		var output = Data()
		let length = payload.count

		output.append(0x80 | opcode)
		if length <= 125 {
			output.append(0x80 | UInt8(length))
		} else if length <= UInt16.max {
			output.append(0x80 | 126)
			output.append(UInt8((length >> 8) & 0xFF))
			output.append(UInt8(length & 0xFF))
		} else {
			output.append(0x80 | 127)
			output.append(contentsOf: UInt64(length).bigEndianBytes)
		}

		let mask = randomData(byteCount: 4)
		output.append(mask)
		for index in payload.indices {
			output.append(payload[index] ^ mask[index % 4])
		}

		return output
	}

	private func checkedPayloadLength(_ length: UInt64) throws -> Int {
		guard length <= UInt64(Int.max) else {
			throw DecodexAppBridgeError.invalidResponse("dashboard WebSocket frame is too large")
		}

		return Int(length)
	}

	private func readUInt16(offset: Int) -> UInt16 {
		let first = UInt16(buffer[offset])
		let second = UInt16(buffer[offset + 1])

		return (first << 8) | second
	}

	private func readUInt64(offset: Int) -> UInt64 {
		var value: UInt64 = 0
		for byte in buffer[offset..<offset + 8] {
			value = (value << 8) | UInt64(byte)
		}

		return value
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

struct WebSocketFrame {
	let opcode: UInt8
	let payload: Data
}

final class DashboardConnectionResumeBox: @unchecked Sendable {
	private let lock = NSLock()
	private var resumed = false
	private let continuation: CheckedContinuation<Void, Error>

	init(continuation: CheckedContinuation<Void, Error>) {
		self.continuation = continuation
	}

	func resume(_ result: Result<Void, Error>) {
		lock.lock()
		defer {
			lock.unlock()
		}
		guard resumed == false else {
			return
		}
		resumed = true
		continuation.resume(with: result)
	}
}

private extension UInt64 {
	var bigEndianBytes: [UInt8] {
		withUnsafeBytes(of: bigEndian) { bytes in
			Array(bytes)
		}
	}
}
