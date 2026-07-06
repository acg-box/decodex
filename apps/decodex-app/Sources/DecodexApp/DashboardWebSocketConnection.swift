import Foundation
import Network

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
