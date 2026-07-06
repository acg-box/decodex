import Foundation
import Security

extension DashboardWebSocketConnection {
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

struct WebSocketFrame {
	let opcode: UInt8
	let payload: Data
}

private extension UInt64 {
	var bigEndianBytes: [UInt8] {
		withUnsafeBytes(of: bigEndian) { bytes in
			Array(bytes)
		}
	}
}
