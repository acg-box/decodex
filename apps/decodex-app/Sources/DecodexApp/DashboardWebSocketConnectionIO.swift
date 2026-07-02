import Foundation
import Network

extension DashboardWebSocketConnection {
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
