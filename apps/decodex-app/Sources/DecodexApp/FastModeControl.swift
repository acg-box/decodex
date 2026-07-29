import Foundation
import Observation

private let fastModeSchema = "decodex/fast-mode-cli/1"

enum FastModeClientError: Error, LocalizedError {
	case unavailable
	case timedOut
	case invalidResponse
	case rejected

	var errorDescription: String? {
		switch self {
		case .unavailable:
			return "The bundled Decodex CLI is unavailable."
		case .timedOut:
			return "Fast mode did not respond."
		case .invalidResponse:
			return "Fast mode returned an invalid response."
		case .rejected:
			return "Fast mode could not be changed safely."
		}
	}
}

protocol FastModeClient: Sendable {
	func status() async throws -> Bool
	func setEnabled(_ enabled: Bool) async throws -> Bool
}

struct FastModeCLIClient: FastModeClient, Sendable {
	private let runner: ResetCardCLIClient

	init() {
		runner = ResetCardCLIClient(timeout: 10)
	}

	func status() async throws -> Bool {
		try await request(arguments: ["--output", "json", "fast-mode", "status"])
	}

	func setEnabled(_ enabled: Bool) async throws -> Bool {
		try await request(arguments: [
			"--output",
			"json",
			"fast-mode",
			"set",
			"--enabled",
			enabled ? "true" : "false",
		])
	}

	private func request(arguments: [String]) async throws -> Bool {
		do {
			let result = try await runner.run(arguments: arguments)
			guard result.exitCode == 0 else {
				throw FastModeClientError.rejected
			}
			let document: FastModeDocument
			do {
				document = try JSONDecoder().decode(
					FastModeDocument.self,
					from: result.standardOutput
				)
			} catch {
				throw FastModeClientError.invalidResponse
			}
			guard document.schema == fastModeSchema,
				document.outcome == "success",
				document.command == (arguments.contains("set") ? "set" : "status")
			else {
				throw FastModeClientError.invalidResponse
			}
			return document.enabled
		} catch let error as FastModeClientError {
			throw error
		} catch let error as ResetCardClientError {
			switch error {
			case .executableMissing, .launchFailed:
				throw FastModeClientError.unavailable
			case .timedOut:
				throw FastModeClientError.timedOut
			case .outputTooLarge, .commandRejected,
				.useDefinitelyNotDispatched, .usePotentiallyDispatched,
				.commandFailed, .invalidResponse, .service:
				throw FastModeClientError.invalidResponse
			}
		} catch {
			throw FastModeClientError.invalidResponse
		}
	}
}

@MainActor
@Observable
final class FastModeStore {
	private(set) var isEnabled = false
	private(set) var isLoading = false
	private(set) var hasLoaded = false
	private(set) var errorMessage: String?
	@ObservationIgnored private let client: any FastModeClient

	init(client: any FastModeClient = FastModeCLIClient()) {
		self.client = client
	}

	func load() async {
		guard isLoading == false else {
			return
		}
		isLoading = true
		defer {
			isLoading = false
			hasLoaded = true
		}
		do {
			isEnabled = try await client.status()
			errorMessage = nil
		} catch {
			errorMessage = (error as? LocalizedError)?.errorDescription
				?? "Fast mode is unavailable."
		}
	}

	func toggle() async {
		guard isLoading == false else {
			return
		}
		isLoading = true
		defer {
			isLoading = false
			hasLoaded = true
		}
		do {
			isEnabled = try await client.setEnabled(isEnabled == false)
			errorMessage = nil
		} catch {
			errorMessage = (error as? LocalizedError)?.errorDescription
				?? "Fast mode is unavailable."
		}
	}

	func dismissError() {
		errorMessage = nil
	}
}

struct FastModeDocument: Decodable {
	let schema: String
	let command: String
	let outcome: String
	let enabled: Bool

	init(from decoder: Decoder) throws {
		try rejectUnknownFields(
			in: decoder,
			allowed: ["schema", "command", "outcome", "enabled"]
		)
		let raw = try decoder.container(keyedBy: FastModeCodingKey.self)
		schema = try raw.decode(String.self, forKey: .schema)
		command = try raw.decode(String.self, forKey: .command)
		outcome = try raw.decode(String.self, forKey: .outcome)
		enabled = try raw.decode(Bool.self, forKey: .enabled)
	}
}

private enum FastModeCodingKey: String, CodingKey {
	case schema
	case command
	case outcome
	case enabled
}
