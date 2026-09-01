import Foundation
import Observation

enum FastModeClientError: Error, LocalizedError {
	case unavailable
	case timedOut
	case invalidResponse
	case rejected

	var errorDescription: String? {
		switch self {
		case .unavailable:
			return "Restart Decodex."
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

extension DecodexNativeClient: FastModeClient {
	func status() async throws -> Bool {
		try await fastModeRequest(
			DecodexNativeRequest(operation: "fast_mode_status")
		)
	}

	func setEnabled(_ enabled: Bool) async throws -> Bool {
		try await fastModeRequest(
			DecodexNativeRequest(
				operation: "set_fast_mode",
				enabled: enabled
			)
		)
	}

	private func fastModeRequest(
		_ request: DecodexNativeRequest
	) async throws -> Bool {
		do {
			let response: (
				authority: ResetCardAuthority,
				data: DecodexNativeFastModeWire
			) = try await perform(request, authority: nil)
			return response.data.enabled
		} catch let error as FastModeClientError {
			throw error
		} catch let error as ResetCardClientError {
			switch error {
			case .nativeClientUnavailable:
				throw FastModeClientError.unavailable
			case .timedOut:
				throw FastModeClientError.timedOut
			case .transportDisconnected:
				throw FastModeClientError.unavailable
			case .commandRejected:
				throw FastModeClientError.rejected
			case .outputTooLarge,
				.useDefinitelyNotDispatched, .usePotentiallyDispatched,
				.transportBackpressured, .invalidResponse, .service:
				throw FastModeClientError.invalidResponse
			}
		} catch {
			throw FastModeClientError.invalidResponse
		}
	}
}

private struct DecodexNativeFastModeWire: Decodable {
	let enabled: Bool

	init(from decoder: Decoder) throws {
		try requireExactFields(in: decoder, expected: ["enabled"])
		let container = try decoder.container(keyedBy: CodingKeys.self)
		enabled = try container.decode(Bool.self, forKey: .enabled)
	}

	private enum CodingKeys: String, CodingKey {
		case enabled
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

	init(client: any FastModeClient = DecodexNativeClient()) {
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
