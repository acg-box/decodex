import Foundation

private struct AppBridgeEvent<Response: Decodable>: Decodable {
	let kind: String
	let text: String?
	let payload: Response?
	let message: String?
}

final class AppBridgeEventParser<Response: Decodable>: @unchecked Sendable {
	private let decoder = JSONDecoder()
	private let lock = NSLock()
	private let onOutput: (@MainActor @Sendable (String) -> Void)?
	private var buffer = ""
	private var response: Response?
	private var bridgeError: String?

	init(onOutput: (@MainActor @Sendable (String) -> Void)? = nil) {
		self.onOutput = onOutput
	}

	func append(_ data: Data) throws {
		guard data.isEmpty == false else {
			return
		}

		lock.lock()
		buffer += String(decoding: data, as: UTF8.self)
		let lines = completeLines()
		lock.unlock()

		for line in lines {
			try handle(line)
		}
	}

	func finish() throws -> Response {
		lock.lock()
		let remainder = buffer
		buffer = ""
		lock.unlock()

		if remainder.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty == false {
			try handle(remainder)
		}

		if let bridgeError {
			throw DecodexAppBridgeError.commandFailed(1, bridgeError)
		}
		guard let response else {
			throw DecodexAppBridgeError.invalidResponse("missing result event")
		}

		return response
	}

	private func completeLines() -> [String] {
		var lines: [String] = []

		while let newlineIndex = buffer.firstIndex(of: "\n") {
			lines.append(String(buffer[..<newlineIndex]))
			buffer.removeSubrange(...newlineIndex)
		}

		return lines
	}

	private func handle(_ line: String) throws {
		let trimmed = line.trimmingCharacters(in: .whitespacesAndNewlines)
		guard trimmed.isEmpty == false else {
			return
		}
		guard let data = trimmed.data(using: .utf8) else {
			throw DecodexAppBridgeError.invalidResponse("event is not UTF-8")
		}

		let event = try decoder.decode(AppBridgeEvent<Response>.self, from: data)

		switch event.kind {
		case "output":
			if let text = event.text {
				Task { @MainActor in
					onOutput?(text)
				}
			}
		case "result":
			guard let payload = event.payload else {
				throw DecodexAppBridgeError.invalidResponse("result event omitted payload")
			}
			response = payload
		case "error":
			bridgeError = event.message ?? "unknown helper error"
		default:
			throw DecodexAppBridgeError.invalidResponse("unknown event kind \(event.kind)")
		}
	}
}
