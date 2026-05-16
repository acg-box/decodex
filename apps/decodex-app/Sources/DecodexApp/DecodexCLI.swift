import Foundation

struct CommandResult {
	let output: String
	let error: String
}

final class TranscriptBuffer: @unchecked Sendable {
	private let lock = NSLock()
	private var text = ""

	func append(_ data: Data) -> String? {
		guard !data.isEmpty else {
			return nil
		}

		let chunk = String(decoding: data, as: UTF8.self)

		lock.lock()
		text += chunk
		lock.unlock()

		return chunk
	}

	var value: String {
		lock.lock()
		defer {
			lock.unlock()
		}

		return text
	}
}

final class ContinuationGate: @unchecked Sendable {
	private let lock = NSLock()
	private var didResume = false

	func finish(_ continuation: CheckedContinuation<Void, Error>, with result: Result<Void, Error>) {
		lock.lock()
		defer {
			lock.unlock()
		}

		guard !didResume else {
			return
		}
		didResume = true

		switch result {
		case .success:
			continuation.resume(returning: ())
		case .failure(let error):
			continuation.resume(throwing: error)
		}
	}
}

enum DecodexCLIError: LocalizedError {
	case launchFailed(String)
	case commandFailed(Int32, String)
	case invalidJSON(String)

	var errorDescription: String? {
		switch self {
		case .launchFailed(let message):
			return message
		case .commandFailed(let code, let message):
			return "decodex exited with status \(code): \(message)"
		case .invalidJSON(let message):
			return "Invalid decodex JSON: \(message)"
		}
	}
}

struct DecodexCLI: Sendable {
	private let command: String

	init(command: String = ProcessInfo.processInfo.environment["DECODEX_CLI"] ?? "decodex") {
		self.command = command
	}

	func runJSON<T: Decodable>(_ arguments: [String], as type: T.Type) async throws -> T {
		let result = try await run(arguments)
		guard let data = result.output.data(using: .utf8) else {
			throw DecodexCLIError.invalidJSON("output is not UTF-8")
		}

		do {
			return try JSONDecoder().decode(type, from: data)
		} catch {
			throw DecodexCLIError.invalidJSON(error.localizedDescription)
		}
	}

	func run(_ arguments: [String]) async throws -> CommandResult {
		try await Task.detached(priority: .userInitiated) {
			let process = Process()
			let outputPipe = Pipe()
			let errorPipe = Pipe()

			process.executableURL = URL(fileURLWithPath: "/usr/bin/env")
			process.arguments = [self.command] + arguments
			process.standardOutput = outputPipe
			process.standardError = errorPipe

			do {
				try process.run()
			} catch {
				throw DecodexCLIError.launchFailed(error.localizedDescription)
			}

			process.waitUntilExit()

			let output = String(
				decoding: outputPipe.fileHandleForReading.readDataToEndOfFile(),
				as: UTF8.self
			)
			let error = String(
				decoding: errorPipe.fileHandleForReading.readDataToEndOfFile(),
				as: UTF8.self
			)

			if process.terminationStatus != 0 {
				throw DecodexCLIError.commandFailed(process.terminationStatus, error + output)
			}

			return CommandResult(output: output, error: error)
		}
		.value
	}

	func runStreaming(
		_ arguments: [String],
		onOutput: @escaping @MainActor @Sendable (String) -> Void
	) async throws {
		try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
			let process = Process()
			let outputPipe = Pipe()
			let errorPipe = Pipe()
			let transcript = TranscriptBuffer()
			let gate = ContinuationGate()

			let append: @Sendable (Data) -> Void = { data in
				guard let chunk = transcript.append(data) else {
					return
				}

				Task { @MainActor in
					onOutput(chunk)
				}
			}

			let finish: @Sendable (Result<Void, Error>) -> Void = { result in
				gate.finish(continuation, with: result)
			}

			outputPipe.fileHandleForReading.readabilityHandler = { handle in
				append(handle.availableData)
			}
			errorPipe.fileHandleForReading.readabilityHandler = { handle in
				append(handle.availableData)
			}

			process.executableURL = URL(fileURLWithPath: "/usr/bin/env")
			process.arguments = [command] + arguments
			process.standardOutput = outputPipe
			process.standardError = errorPipe
			process.terminationHandler = { process in
				if process.terminationStatus == 0 {
					finish(.success(()))
				} else {
					finish(.failure(DecodexCLIError.commandFailed(process.terminationStatus, transcript.value)))
				}
			}

			do {
				try process.run()
			} catch {
				finish(.failure(DecodexCLIError.launchFailed(error.localizedDescription)))
			}
		}
	}
}
