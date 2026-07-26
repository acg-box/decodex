import Darwin
import Foundation

private let resetCardCLISchema = "decodex/reset-card-cli/1"
private let resetCardCLIItemLimit = 64

enum ResetCardServiceError: String, Decodable, Equatable, Sendable {
	case invalidRequest = "invalid_request"
	case accountNotFound = "account_not_found"
	case accountStateRejected = "account_state_rejected"
	case vaultUnavailable = "vault_unavailable"
	case schemaUnsupported = "schema_unsupported"
	case providerUnavailable = "provider_unavailable"
	case inventoryIncomplete = "inventory_incomplete"
	case inventoryChanged = "inventory_changed"
	case resourceExhausted = "resource_exhausted"
	case productStateUnavailable = "product_state_unavailable"
	case effectAmbiguous = "effect_ambiguous"

	var presentation: String {
		switch self {
		case .invalidRequest:
			return "The reset-card request was invalid."
		case .accountNotFound:
			return "The vNext account no longer exists."
		case .accountStateRejected:
			return "The vNext account cannot use a reset card in its current state."
		case .vaultUnavailable:
			return "The daemon credential vault is unavailable."
		case .schemaUnsupported:
			return "The selected Codex version does not support reset cards."
		case .providerUnavailable:
			return "The reset-card provider is unavailable."
		case .inventoryIncomplete:
			return "The daemon could not establish a complete reset-card inventory."
		case .inventoryChanged:
			return "The reset cards changed. Refresh and select the card again."
		case .resourceExhausted:
			return "The reset-card service reached a bounded resource limit."
		case .productStateUnavailable:
			return "Authoritative reset-card state is unavailable."
		case .effectAmbiguous:
			return "The reset-card effect needs authoritative reconciliation."
		}
	}
}

enum ResetCardUseOutcome: String, Decodable, Equatable, Sendable {
	case reset
	case nothingToReset = "nothing_to_reset"
	case noCredit = "no_credit"
	case alreadyRedeemed = "already_redeemed"

	var presentation: String {
		switch self {
		case .reset:
			return "Usage restored."
		case .nothingToReset:
			return "The account had nothing to reset."
		case .noCredit:
			return "No eligible reset card was available."
		case .alreadyRedeemed:
			return "The selected reset card was already used."
		}
	}
}

enum ResetCardOperationState: Equatable, Sendable {
	case notFound
	case prepared
	case effectAmbiguous
	case completed(ResetCardUseOutcome)
	case failedBeforeEffect(ResetCardServiceError)
	case unavailable(ResetCardServiceError)

	var presentation: String {
		switch self {
		case .notFound:
			return "No durable reset-card operation was found."
		case .prepared:
			return "Reset-card use was accepted."
		case .effectAmbiguous:
			return "Reset-card use is reconciling authoritative state."
		case .completed(let outcome):
			return outcome.presentation
		case .failedBeforeEffect(let error):
			return error.presentation
		case .unavailable(let error):
			return "Authoritative reset-card status is unavailable. \(error.presentation)"
		}
	}
}

enum ResetCardAdmissionState: String, Decodable, Equatable, Sendable {
	case available
	case depleted
}

enum ResetCardUseDispatchState: String, Decodable, Equatable, Sendable {
	case definitelyNotDispatched = "definitely_not_dispatched"
	case potentiallyDispatched = "potentially_dispatched"
	case durablyAccepted = "durably_accepted"
	case rejectedBeforeAcceptance = "rejected_before_acceptance"
}

struct ResetCardAccountRecord: Identifiable, Equatable, Sendable {
	let authority: ResetCardAuthority
	let accountID: String
	let displayLabel: String
	let accountRevision: UInt64
	let admissionState: ResetCardAdmissionState

	var id: String {
		accountID
	}
}

struct ResetCardInventory: Equatable, Sendable {
	let authority: ResetCardAuthority
	let accountID: String
	let accountRevision: UInt64
	let cards: [ResetCardDescriptor]
}

enum ResetCardClientError: Error, Equatable, LocalizedError, Sendable, CustomDebugStringConvertible {
	case executableMissing
	case launchFailed
	case timedOut
	case outputTooLarge
	case commandRejected
	case useDefinitelyNotDispatched
	case usePotentiallyDispatched
	case commandFailed
	case invalidResponse
	case service(ResetCardServiceError)

	var errorDescription: String? {
		switch self {
		case .executableMissing:
			return "The bundled Decodex CLI is unavailable."
		case .launchFailed:
			return "The Decodex CLI could not start."
		case .timedOut:
			return "The Decodex CLI request timed out."
		case .outputTooLarge:
			return "The Decodex CLI returned too much data."
		case .commandRejected:
			return "The reset-card request was rejected. Refresh and try again."
		case .useDefinitelyNotDispatched:
			return "The reset-card request was not dispatched. Resume the pending request with the same operation key."
		case .usePotentiallyDispatched:
			return "The reset-card request may have been dispatched. Resume the pending request to check authoritative state with the same operation key."
		case .commandFailed:
			return "The reset-card service is unavailable or rejected the CLI connection. Start and configure decodexd, then resume the pending request."
		case .invalidResponse:
			return "The Decodex CLI returned an invalid reset-card response."
		case .service(let error):
			return error.presentation
		}
	}

	var debugDescription: String {
		switch self {
		case .executableMissing:
			return "ResetCardClientError.executableMissing"
		case .launchFailed:
			return "ResetCardClientError.launchFailed"
		case .timedOut:
			return "ResetCardClientError.timedOut"
		case .outputTooLarge:
			return "ResetCardClientError.outputTooLarge"
		case .commandRejected:
			return "ResetCardClientError.commandRejected"
		case .useDefinitelyNotDispatched:
			return "ResetCardClientError.useDefinitelyNotDispatched"
		case .usePotentiallyDispatched:
			return "ResetCardClientError.usePotentiallyDispatched"
		case .commandFailed:
			return "ResetCardClientError.commandFailed"
		case .invalidResponse:
			return "ResetCardClientError.invalidResponse"
		case .service(let error):
			return "ResetCardClientError.service(\(error.rawValue))"
		}
	}
}

protocol ResetCardClient: Sendable {
	func accounts() async throws -> [ResetCardAccountRecord]
	func inventory(for account: ResetCardAccountRecord) async throws -> ResetCardInventory
	func use(_ attempt: ResetCardUseAttempt) async throws -> ResetCardOperationState
	func status(for attempt: ResetCardUseAttempt) async throws -> ResetCardOperationState
}

struct ResetCardCLIClient: ResetCardClient, Sendable, CustomDebugStringConvertible {
	static let executableOverrideKey = "DECODEX_APP_CLI"

	private let executableURL: URL?
	private let childEnvironment: [String: String]
	private let timeout: TimeInterval

	init() {
		let environment = ProcessInfo.processInfo.environment
		executableURL = Self.resolveExecutableURL(
			environment: environment,
			bundleURL: Bundle.main.bundleURL,
			isExecutableFile: FileManager.default.isExecutableFile(atPath:)
		)
		childEnvironment = Self.sanitizedChildEnvironment(from: environment)
		timeout = 75
	}

	init(
		executableURL: URL,
		environment: [String: String],
		timeout: TimeInterval = 15
	) {
		self.executableURL = executableURL.standardizedFileURL
		childEnvironment = Self.sanitizedChildEnvironment(from: environment)
		self.timeout = timeout
	}

	var debugDescription: String {
		"ResetCardCLIClient(executableConfigured: \(executableURL != nil))"
	}

	func accounts() async throws -> [ResetCardAccountRecord] {
		let processResult = try await run(
			arguments: Self.accountsArguments()
		)
		let document = try decode(
			ResetCardResultDocument<ResetCardAccountsWireResult>.self,
			from: processResult,
			command: "accounts"
		)

		switch document.result {
		case .available(let accounts):
			guard document.outcome == "available", processResult.exitCode == 0 else {
				throw ResetCardClientError.invalidResponse
			}
			let authority = try document.authority.authority
			return try accounts.map { try $0.record(authority: authority) }
		case .unavailable(let error):
			guard document.outcome == "unavailable" else {
				throw ResetCardClientError.invalidResponse
			}
			throw ResetCardClientError.service(error)
		}
	}

	func inventory(for account: ResetCardAccountRecord) async throws -> ResetCardInventory {
		guard Self.isCanonicalAccountID(account.accountID),
			Self.isValidAuthority(account.authority)
		else {
			throw ResetCardClientError.invalidResponse
		}

		let processResult = try await run(
			arguments: Self.listArguments(
				accountID: account.accountID,
				authority: account.authority
			)
		)
		let document = try decode(
			ResetCardResultDocument<ResetCardInventoryWireResult>.self,
			from: processResult,
			command: "list"
		)

		switch document.result {
		case .available(let inventory):
			guard document.outcome == "available", processResult.exitCode == 0 else {
				throw ResetCardClientError.invalidResponse
			}
			let authority = try document.authority.authority
			let value = try inventory.inventory(authority: authority)
			guard authority == account.authority,
				value.accountID == account.accountID
			else {
				throw ResetCardClientError.invalidResponse
			}

			return value
		case .unavailable(let error):
			guard document.outcome == "unavailable" else {
				throw ResetCardClientError.invalidResponse
			}
			throw ResetCardClientError.service(error)
		}
	}

	func use(_ attempt: ResetCardUseAttempt) async throws -> ResetCardOperationState {
		guard Self.isCanonicalAccountID(attempt.target.accountID),
			attempt.target.expectedRevision > 0,
			Self.isCanonicalUUID(attempt.idempotencyKey),
			Self.isValidAuthority(attempt.target.authority)
		else {
			throw ResetCardClientError.invalidResponse
		}

		let processResult = try await run(
			arguments: Self.useArguments(attempt: attempt)
		)
		let document = try decode(
			ResetCardUseDocument.self,
			from: processResult,
			command: "use"
		)

		guard document.idempotencyKey == attempt.idempotencyKey,
			let dispatchState = document.dispatchState
		else {
			throw ResetCardClientError.invalidResponse
		}

		switch dispatchState {
		case .durablyAccepted:
			guard let state = document.state,
				processResult.exitCode == state.expectedExitCode,
				document.outcome == state.outerOutcome,
				document.accountID == attempt.target.accountID,
				document.accountRevision == attempt.target.expectedRevision,
				try document.descriptor?.descriptor == attempt.target.descriptor,
				document.error == nil,
				document.failure == nil
			else {
				throw ResetCardClientError.invalidResponse
			}

			return state.state
		case .rejectedBeforeAcceptance:
			guard processResult.exitCode == 1,
				document.outcome == "rejected",
				document.error != nil,
				document.accountID == nil,
				document.descriptor == nil,
				document.accountRevision == nil,
				document.state == nil,
				document.failure == nil
			else {
				throw ResetCardClientError.invalidResponse
			}

			throw ResetCardClientError.commandRejected
		case .definitelyNotDispatched, .potentiallyDispatched:
			guard processResult.exitCode == 2,
				document.outcome == "failure",
				document.failure != nil,
				document.accountID == nil,
				document.descriptor == nil,
				document.accountRevision == nil,
				document.state == nil,
				document.error == nil
			else {
				throw ResetCardClientError.invalidResponse
			}

			switch dispatchState {
			case .definitelyNotDispatched:
				throw ResetCardClientError.useDefinitelyNotDispatched
			case .potentiallyDispatched:
				throw ResetCardClientError.usePotentiallyDispatched
			case .durablyAccepted, .rejectedBeforeAcceptance:
				throw ResetCardClientError.invalidResponse
			}
		}
	}

	func status(for attempt: ResetCardUseAttempt) async throws -> ResetCardOperationState {
		guard Self.isCanonicalUUID(attempt.idempotencyKey),
			Self.isValidAuthority(attempt.target.authority)
		else {
			throw ResetCardClientError.invalidResponse
		}

		let processResult = try await run(
			arguments: Self.statusArguments(attempt: attempt)
		)
		let document = try decode(
			ResetCardStatusDocument.self,
			from: processResult,
			command: "status"
		)
		guard processResult.exitCode == document.state.expectedExitCode,
			document.outcome == document.state.outerOutcome,
			document.idempotencyKey == attempt.idempotencyKey
		else {
			throw ResetCardClientError.commandFailed
		}

		return document.state.state
	}

	static func accountsArguments() -> [String] {
		["--output", "json", "reset-card", "accounts"]
	}

	static func listArguments(
		accountID: String,
		authority: ResetCardAuthority
	) -> [String] {
		authorityArguments(authority) + [
			"--output", "json", "reset-card", "list", "--account", accountID,
		]
	}

	static func useArguments(attempt: ResetCardUseAttempt) -> [String] {
		authorityArguments(attempt.target.authority) + [
			"--output",
			"json",
			"reset-card",
			"use",
			"--account",
			attempt.target.accountID,
			"--granted-at",
			String(attempt.target.descriptor.grantedAtUnixSeconds),
			"--expires-at",
			String(attempt.target.descriptor.expiresAtUnixSeconds),
			"--expected-revision",
			String(attempt.target.expectedRevision),
			"--idempotency-key",
			attempt.idempotencyKey,
			"--yes",
		]
	}

	static func statusArguments(attempt: ResetCardUseAttempt) -> [String] {
		authorityArguments(attempt.target.authority) + [
			"--output",
			"json",
			"reset-card",
			"status",
			"--idempotency-key",
			attempt.idempotencyKey,
		]
	}

	private static func authorityArguments(_ authority: ResetCardAuthority) -> [String] {
		[
			"--profile",
			authority.profileName,
			"--expected-server-id",
			authority.serverID,
		]
	}

	static func resolveExecutableURL(
		environment: [String: String],
		bundleURL: URL,
		isExecutableFile: (String) -> Bool
	) -> URL? {
		if let override = environment[executableOverrideKey]?
			.trimmingCharacters(in: .whitespacesAndNewlines),
			override.isEmpty == false
		{
			let candidate = URL(fileURLWithPath: override).standardizedFileURL
			if candidate.path == override || override.hasPrefix("/"),
				isExecutableFile(candidate.path)
			{
				return candidate
			}

			return nil
		}

		let bundled = bundleURL
			.appendingPathComponent("Contents", isDirectory: true)
			.appendingPathComponent("Helpers", isDirectory: true)
			.appendingPathComponent("decodex-cli")
			.standardizedFileURL

		return isExecutableFile(bundled.path) ? bundled : nil
	}

	static func sanitizedChildEnvironment(from environment: [String: String]) -> [String: String] {
		let allowedKeys = [
			"HOME",
			"USER",
			"LOGNAME",
			"TMPDIR",
			"LANG",
			"LC_ALL",
			"LC_CTYPE",
		]
		var sanitized = environment.filter { allowedKeys.contains($0.key) }
		if sanitized["LANG"] == nil, sanitized["LC_ALL"] == nil {
			sanitized["LC_ALL"] = "C"
		}

		return sanitized
	}

	private func run(
		arguments: [String]
	) async throws -> ResetCardProcessResult {
		guard let executableURL else {
			throw ResetCardClientError.executableMissing
		}

		return try await Task.detached(priority: .userInitiated) {
			try Self.runSynchronously(
				executableURL: executableURL,
				arguments: arguments,
				environment: childEnvironment,
				timeout: timeout
			)
		}
		.value
	}

	private func decode<Document: Decodable & ResetCardStableDocument>(
		_ type: Document.Type,
		from processResult: ResetCardProcessResult,
		command: String
	) throws -> Document {
		do {
			let document = try JSONDecoder().decode(Document.self, from: processResult.standardOutput)
			guard document.schema == resetCardCLISchema,
				document.command == command
			else {
				throw ResetCardClientError.invalidResponse
			}

			return document
		} catch let error as ResetCardClientError {
			throw error
		} catch {
			if processResult.exitCode != 0 {
				throw ResetCardClientError.commandFailed
			}

			throw ResetCardClientError.invalidResponse
		}
	}

	private static func runSynchronously(
		executableURL: URL,
		arguments: [String],
		environment: [String: String],
		timeout: TimeInterval
	) throws -> ResetCardProcessResult {
		let process = Process()
		let outputPipe = Pipe()
		let errorPipe = Pipe()
		let outputCapture = ResetCardBoundedCapture(limit: 320 * 1_024)
		let errorCapture = ResetCardBoundedCapture(limit: 8 * 1_024)

		process.executableURL = executableURL
		process.arguments = arguments
		process.environment = environment
		process.standardInput = FileHandle.nullDevice
		process.standardOutput = outputPipe
		process.standardError = errorPipe

		do {
			try process.run()
		} catch {
			throw ResetCardClientError.launchFailed
		}

		try? outputPipe.fileHandleForWriting.close()
		try? errorPipe.fileHandleForWriting.close()

		let readers = DispatchGroup()
		readers.enter()
		DispatchQueue.global(qos: .userInitiated).async {
			readPipe(outputPipe.fileHandleForReading, into: outputCapture)
			readers.leave()
		}
		readers.enter()
		DispatchQueue.global(qos: .utility).async {
			readPipe(errorPipe.fileHandleForReading, into: errorCapture)
			readers.leave()
		}

		let deadline = Date().addingTimeInterval(max(0.05, timeout))
		var timedOut = false
		while process.isRunning {
			if outputCapture.exceeded || errorCapture.exceeded {
				break
			}
			if Date() >= deadline {
				timedOut = true
				break
			}

			Thread.sleep(forTimeInterval: 0.01)
		}

		if process.isRunning {
			process.terminate()
			let terminationDeadline = Date().addingTimeInterval(0.25)
			while process.isRunning, Date() < terminationDeadline {
				Thread.sleep(forTimeInterval: 0.01)
			}
			if process.isRunning {
				_ = Darwin.kill(process.processIdentifier, SIGKILL)
			}
		}

		process.waitUntilExit()
		_ = readers.wait(timeout: .now() + 1)

		if timedOut {
			throw ResetCardClientError.timedOut
		}
		if outputCapture.exceeded || errorCapture.exceeded {
			throw ResetCardClientError.outputTooLarge
		}

		return ResetCardProcessResult(
			exitCode: process.terminationStatus,
			standardOutput: outputCapture.data
		)
	}

	private static func readPipe(
		_ handle: FileHandle,
		into capture: ResetCardBoundedCapture
	) {
		while true {
			let chunk = handle.availableData
			if chunk.isEmpty {
				break
			}

			capture.append(chunk)
		}
		try? handle.close()
	}

	static func isCanonicalAccountID(_ value: String) -> Bool {
		isCanonicalUUID(value)
	}

	static func isCanonicalUUID(_ value: String) -> Bool {
		guard let uuid = UUID(uuidString: value) else {
			return false
		}

		return uuid.uuidString.lowercased() == value
	}

	static func isValidAuthority(_ authority: ResetCardAuthority) -> Bool {
		let profile = authority.profileName

		return profile.isEmpty == false
			&& profile.utf8.count <= 64
			&& profile.utf8.allSatisfy {
				($0 >= 0x61 && $0 <= 0x7a)
					|| ($0 >= 0x41 && $0 <= 0x5a)
					|| ($0 >= 0x30 && $0 <= 0x39)
					|| $0 == 0x2d
					|| $0 == 0x5f
			}
			&& isCanonicalUUID(authority.serverID)
	}
}

private struct ResetCardProcessResult: Sendable {
	let exitCode: Int32
	let standardOutput: Data
}

private final class ResetCardBoundedCapture: @unchecked Sendable {
	private let limit: Int
	private let lock = NSLock()
	private var storage = Data()
	private var didExceed = false

	init(limit: Int) {
		self.limit = limit
	}

	func append(_ chunk: Data) {
		lock.withLock {
			guard didExceed == false else {
				return
			}
			guard storage.count + chunk.count <= limit else {
				didExceed = true
				return
			}

			storage.append(chunk)
		}
	}

	var exceeded: Bool {
		lock.withLock { didExceed }
	}

	var data: Data {
		lock.withLock { storage }
	}
}

private protocol ResetCardStableDocument {
	var schema: String { get }
	var command: String { get }
}

private struct ResetCardResultDocument<Result: Decodable>: Decodable, ResetCardStableDocument {
	let schema: String
	let command: String
	let outcome: String
	let authority: ResetCardAuthorityWire
	let result: Result
}

private struct ResetCardAuthorityWire: Decodable {
	let profileName: String
	let serverID: String

	enum CodingKeys: String, CodingKey {
		case profileName = "profile_name"
		case serverID = "server_id"
	}

	var authority: ResetCardAuthority {
		get throws {
			let value = ResetCardAuthority(
				profileName: profileName,
				serverID: serverID
			)
			guard ResetCardCLIClient.isValidAuthority(value) else {
				throw ResetCardClientError.invalidResponse
			}

			return value
		}
	}
}

private struct ResetCardAccountWire: Decodable, Sendable {
	let accountID: String
	let displayLabel: String
	let accountRevision: UInt64
	let admissionState: ResetCardAdmissionState

	enum CodingKeys: String, CodingKey {
		case accountID = "account_id"
		case displayLabel = "display_label"
		case accountRevision = "account_revision"
		case admissionState = "admission_state"
	}

	func record(authority: ResetCardAuthority) throws -> ResetCardAccountRecord {
		guard ResetCardCLIClient.isValidAuthority(authority),
			ResetCardCLIClient.isCanonicalAccountID(accountID),
			displayLabel.isEmpty == false,
			displayLabel.utf8.count <= 4_096,
			accountRevision > 0
		else {
			throw ResetCardClientError.invalidResponse
		}

		return ResetCardAccountRecord(
			authority: authority,
			accountID: accountID,
			displayLabel: displayLabel,
			accountRevision: accountRevision,
			admissionState: admissionState
		)
	}
}

private enum ResetCardAccountsWireResult: Decodable, Sendable {
	case available([ResetCardAccountWire])
	case unavailable(ResetCardServiceError)

	init(from decoder: Decoder) throws {
		let raw = try ResetCardTaggedResult<ResetCardAccountsWireData>.init(from: decoder)
		switch raw.outcome {
		case "available":
			guard let data = raw.data,
				data.accounts.count <= resetCardCLIItemLimit,
				Set(data.accounts.map(\.accountID)).count == data.accounts.count
			else {
				throw ResetCardClientError.invalidResponse
			}
			self = .available(data.accounts)
		case "unavailable":
			guard let error = raw.data?.error else {
				throw ResetCardClientError.invalidResponse
			}
			self = .unavailable(error)
		default:
			throw ResetCardClientError.invalidResponse
		}
	}
}

private struct ResetCardAccountsWireData: Decodable, Sendable {
	let accounts: [ResetCardAccountWire]
	let error: ResetCardServiceError?

	init(from decoder: Decoder) throws {
		let container = try decoder.container(keyedBy: CodingKeys.self)
		accounts = try container.decodeIfPresent([ResetCardAccountWire].self, forKey: .accounts) ?? []
		error = try container.decodeIfPresent(ResetCardServiceError.self, forKey: .error)
	}

	private enum CodingKeys: String, CodingKey {
		case accounts
		case error
	}
}

private struct ResetCardDescriptorWire: Decodable, Sendable {
	let grantedAtUnixSeconds: Int64
	let expiresAtUnixSeconds: Int64

	enum CodingKeys: String, CodingKey {
		case grantedAtUnixSeconds = "granted_at_unix_seconds"
		case expiresAtUnixSeconds = "expires_at_unix_seconds"
	}

	var descriptor: ResetCardDescriptor {
		get throws {
			try ResetCardDescriptor(
				grantedAtUnixSeconds: grantedAtUnixSeconds,
				expiresAtUnixSeconds: expiresAtUnixSeconds
			)
		}
	}
}

private struct ResetCardObservationWire: Decodable, Sendable {
	let descriptor: ResetCardDescriptorWire
}

private struct ResetCardInventoryWireData: Decodable, Sendable {
	let accountID: String?
	let accountRevision: UInt64?
	let availableCount: UInt16?
	let cards: [ResetCardObservationWire]
	let error: ResetCardServiceError?

	enum CodingKeys: String, CodingKey {
		case accountID = "account_id"
		case accountRevision = "account_revision"
		case availableCount = "available_count"
		case cards
		case error
	}

	init(from decoder: Decoder) throws {
		let container = try decoder.container(keyedBy: CodingKeys.self)
		accountID = try container.decodeIfPresent(String.self, forKey: .accountID)
		accountRevision = try container.decodeIfPresent(UInt64.self, forKey: .accountRevision)
		availableCount = try container.decodeIfPresent(UInt16.self, forKey: .availableCount)
		cards = try container.decodeIfPresent([ResetCardObservationWire].self, forKey: .cards) ?? []
		error = try container.decodeIfPresent(ResetCardServiceError.self, forKey: .error)
	}

	func inventory(authority: ResetCardAuthority) throws -> ResetCardInventory {
		guard ResetCardCLIClient.isValidAuthority(authority),
			let accountID,
			let accountRevision,
			let availableCount,
			ResetCardCLIClient.isCanonicalAccountID(accountID),
			accountRevision > 0,
			cards.count == Int(availableCount),
			cards.count <= resetCardCLIItemLimit
		else {
			throw ResetCardClientError.invalidResponse
		}

		let descriptors = try cards.map { try $0.descriptor.descriptor }
		guard Set(descriptors).count == descriptors.count else {
			throw ResetCardClientError.invalidResponse
		}

		return ResetCardInventory(
			authority: authority,
			accountID: accountID,
			accountRevision: accountRevision,
			cards: descriptors
		)
	}
}

private enum ResetCardInventoryWireResult: Decodable, Sendable {
	case available(ResetCardInventoryWireData)
	case unavailable(ResetCardServiceError)

	init(from decoder: Decoder) throws {
		let raw = try ResetCardTaggedResult<ResetCardInventoryWireData>.init(from: decoder)
		switch raw.outcome {
		case "available":
			guard let data = raw.data else {
				throw ResetCardClientError.invalidResponse
			}
			self = .available(data)
		case "unavailable":
			guard let error = raw.data?.error else {
				throw ResetCardClientError.invalidResponse
			}
			self = .unavailable(error)
		default:
			throw ResetCardClientError.invalidResponse
		}
	}
}

private enum ResetCardOperationWireResult: Decodable, Sendable {
	case value(ResetCardOperationState)

	init(from decoder: Decoder) throws {
		let container = try decoder.container(keyedBy: CodingKeys.self)
		let state = try container.decode(String.self, forKey: .state)
		switch state {
		case "not_found":
			self = .value(.notFound)
		case "prepared":
			self = .value(.prepared)
		case "effect_ambiguous":
			self = .value(.effectAmbiguous)
		case "completed":
			let data = try container.decode(CompletedData.self, forKey: .data)
			self = .value(.completed(data.outcome))
		case "failed_before_effect":
			let data = try container.decode(FailedData.self, forKey: .data)
			self = .value(.failedBeforeEffect(data.error))
		case "unavailable":
			let data = try container.decode(FailedData.self, forKey: .data)
			self = .value(.unavailable(data.error))
		default:
			throw ResetCardClientError.invalidResponse
		}
	}

	var outerOutcome: String {
		switch self {
		case .value(.notFound):
			return "not_found"
		case .value(.prepared):
			return "prepared"
		case .value(.effectAmbiguous):
			return "effect_ambiguous"
		case .value(.completed):
			return "completed"
		case .value(.failedBeforeEffect):
			return "failed_before_effect"
		case .value(.unavailable):
			return "unavailable"
		}
	}

	var expectedExitCode: Int32 {
		switch self {
		case .value(.completed):
			return 0
		case .value(.unavailable):
			return 2
		case .value(.notFound), .value(.prepared), .value(.effectAmbiguous),
			.value(.failedBeforeEffect):
			return 1
		}
	}

	var state: ResetCardOperationState {
		switch self {
		case .value(let state):
			return state
		}
	}

	private enum CodingKeys: String, CodingKey {
		case state
		case data
	}

	private struct CompletedData: Decodable {
		let outcome: ResetCardUseOutcome
	}

	private struct FailedData: Decodable {
		let error: ResetCardServiceError
	}
}

private struct ResetCardUseDocument: Decodable, ResetCardStableDocument {
	let schema: String
	let command: String
	let outcome: String
	let idempotencyKey: String?
	let dispatchState: ResetCardUseDispatchState?
	let accountID: String?
	let descriptor: ResetCardDescriptorWire?
	let accountRevision: UInt64?
	let state: ResetCardOperationWireResult?
	let error: ResetCardCommandErrorWire?
	let failure: ResetCardClientFailureWire?

	enum CodingKeys: String, CodingKey {
		case schema
		case command
		case outcome
		case idempotencyKey = "idempotency_key"
		case dispatchState = "dispatch_state"
		case accountID = "account_id"
		case descriptor
		case accountRevision = "account_revision"
		case state
		case error
		case failure
	}
}

private enum ResetCardClientFailureWire: String, Decodable {
	case configurationMissing = "configuration_missing"
	case configurationMalformed = "configuration_malformed"
	case configurationVersion = "configuration_version"
	case profileMissing = "profile_missing"
	case unsafeHostPath = "unsafe_host_path"
	case serverIdentityUnavailable = "server_identity_unavailable"
	case remoteMutationUnsupported = "remote_mutation_unsupported"
	case protocolDisconnected = "protocol_disconnected"
	case protocolTimeout = "protocol_timeout"
	case protocolMajorMismatch = "protocol_major_mismatch"
	case protocolMinorMismatch = "protocol_minor_mismatch"
	case serverIdentityMismatch = "server_identity_mismatch"
	case protocolMalformed = "protocol_malformed"
	case protocolViolation = "protocol_violation"
	case protocolBackpressure = "protocol_backpressure"
	case applicationAcceptanceUnknown = "application_acceptance_unknown"
}

private enum ResetCardCommandErrorWire: Decodable {
	case expectedRevisionMismatch
	case idempotencyConflict
	case idempotencyCapacityExceeded
	case applicationUnavailable

	init(from decoder: Decoder) throws {
		let container = try decoder.container(keyedBy: CodingKeys.self)
		switch try container.decode(String.self, forKey: .reason) {
		case "expected_revision_mismatch":
			guard try container.decode(UInt64.self, forKey: .expected) > 0,
				try container.decode(UInt64.self, forKey: .actual) > 0
			else {
				throw ResetCardClientError.invalidResponse
			}
			self = .expectedRevisionMismatch
		case "idempotency_conflict":
			self = .idempotencyConflict
		case "idempotency_capacity_exceeded":
			guard try container.decode(UInt64.self, forKey: .capacity) > 0 else {
				throw ResetCardClientError.invalidResponse
			}
			self = .idempotencyCapacityExceeded
		case "application_unavailable":
			let message = try container.decode(String.self, forKey: .message)
			guard message.isEmpty == false, message.utf8.count <= 4_096 else {
				throw ResetCardClientError.invalidResponse
			}
			self = .applicationUnavailable
		default:
			throw ResetCardClientError.invalidResponse
		}
	}

	private enum CodingKeys: String, CodingKey {
		case reason
		case expected
		case actual
		case capacity
		case message
	}
}

private struct ResetCardStatusDocument: Decodable, ResetCardStableDocument {
	let schema: String
	let command: String
	let outcome: String
	let idempotencyKey: String
	let state: ResetCardOperationWireResult

	enum CodingKeys: String, CodingKey {
		case schema
		case command
		case outcome
		case idempotencyKey = "idempotency_key"
		case state
	}
}

private struct ResetCardTaggedResult<Data: Decodable>: Decodable {
	let outcome: String
	let data: Data?
}
