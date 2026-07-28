import Darwin
import Foundation

private let resetCardCLISchema = "decodex/reset-card-cli/1"
private let accountCLISchema = "decodex/cli-account/1"
private let resetCardCLIItemLimit = 64
private let accountCLIItemLimit = 512

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
			return "The account no longer exists."
		case .accountStateRejected:
			return "The account cannot use a Reset Card in its current state."
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

enum ResetCardObservedState: String, Decodable, Equatable, Sendable {
	case unavailable
	case unknown
	case available
	case depleted
	case authFailed = "auth_failed"
	case pluginUnready = "plugin_unready"
}

enum ResetCardLifecycleReadiness: String, Decodable, Equatable, Sendable {
	case ready
	case credentialAbsent = "credential_absent"
	case storeUnavailable = "store_unavailable"
	case storeMismatch = "store_mismatch"
	case providerMismatch = "provider_mismatch"
	case operationUnsettled = "operation_unsettled"
	case callbackCapabilityUnready = "callback_capability_unready"
	case tombstoned
}

enum ResetCardQuotaError: String, Decodable, Equatable, Sendable {
	case providerUnavailable = "provider_unavailable"
	case protocolUnavailable = "protocol_unavailable"
	case accountMismatch = "account_mismatch"
	case unsupportedWindow = "unsupported_window"

	var presentation: String {
		switch self {
		case .providerUnavailable:
			return "Provider unavailable"
		case .protocolUnavailable:
			return "Invalid provider response"
		case .accountMismatch:
			return "Account mismatch"
		case .unsupportedWindow:
			return "Unsupported quota window"
		}
	}
}

enum ResetCardQuotaState: Equatable, Sendable {
	case unknown
	case current(usedPercent: UInt8, resetsAtUnixMicros: Int64)
	case stale(usedPercent: UInt8, resetsAtUnixMicros: Int64)
	case error(ResetCardQuotaError)
}

struct ResetCardQuotaWindow: Equatable, Sendable {
	let durationMinutes: UInt32
	let observedAtUnixMicros: Int64?
	let state: ResetCardQuotaState

	var usedPercent: UInt8? {
		switch state {
		case .current(let usedPercent, _), .stale(let usedPercent, _):
			return usedPercent
		case .unknown, .error:
			return nil
		}
	}

	var resetDate: Date? {
		let micros: Int64
		switch state {
		case .current(_, let resetsAtUnixMicros), .stale(_, let resetsAtUnixMicros):
			micros = resetsAtUnixMicros
		case .unknown, .error:
			return nil
		}

		return Date(timeIntervalSince1970: TimeInterval(micros) / 1_000_000)
	}

	var stateLabel: String {
		switch state {
		case .unknown:
			return "Unknown"
		case .current:
			return "Current"
		case .stale:
			return "Stale"
		case .error:
			return "Error"
		}
	}

	var detailLabel: String {
		switch state {
		case .unknown:
			return "No observation"
		case .error(let error):
			return error.presentation
		case .current, .stale:
			return ""
		}
	}

	var accessibilityValue: String {
		switch state {
		case .unknown:
			return "Unknown, no observation"
		case .current(let usedPercent, _):
			return "Current, \(usedPercent) percent used, resets \(resetDate?.formatted() ?? "unknown")"
		case .stale(let usedPercent, _):
			return "Stale, \(usedPercent) percent used, resets \(resetDate?.formatted() ?? "unknown")"
		case .error(let error):
			return "Error, \(error.presentation)"
		}
	}

	static func unknown(durationMinutes: UInt32) -> Self {
		Self(
			durationMinutes: durationMinutes,
			observedAtUnixMicros: nil,
			state: .unknown
		)
	}
}

enum ResetCardUseDispatchState: String, Decodable, Equatable, Sendable {
	case definitelyNotDispatched = "definitely_not_dispatched"
	case potentiallyDispatched = "potentially_dispatched"
	case durablyAccepted = "durably_accepted"
	case rejectedBeforeAcceptance = "rejected_before_acceptance"
}

struct ResetCardAccountRecord: Identifiable, Equatable, Sendable {
	let authority: ResetCardAuthority?
	let accountID: String
	let displayLabel: String
	let accountRevision: UInt64
	let enabled: Bool
	let observedState: ResetCardObservedState
	let lifecycleReadiness: ResetCardLifecycleReadiness
	let fiveHourQuota: ResetCardQuotaWindow
	let sevenDayQuota: ResetCardQuotaWindow

	var id: String {
		accountID
	}

	var statusLabel: String {
		if enabled == false {
			return "Disabled"
		}
		switch lifecycleReadiness {
		case .credentialAbsent:
			return "No credentials"
		case .storeUnavailable:
			return "Store unavailable"
		case .storeMismatch:
			return "Store mismatch"
		case .providerMismatch:
			return "Provider mismatch"
		case .operationUnsettled:
			return "Operation pending"
		case .callbackCapabilityUnready:
			return "Update required"
		case .tombstoned:
			return "Logged out"
		case .ready:
			break
		}
		switch observedState {
		case .unavailable:
			return "Unavailable"
		case .unknown:
			return "Unknown"
		case .available:
			return "Available"
		case .depleted:
			return "Depleted"
		case .authFailed:
			return "Auth failed"
		case .pluginUnready:
			return "Plugin unready"
		}
	}
}

struct ResetCardInventory: Equatable, Sendable {
	let authority: ResetCardAuthority
	let accountID: String
	let accountRevision: UInt64
	let cards: [ResetCardDescriptor]
	let fiveHourQuota: ResetCardQuotaWindow
	let sevenDayQuota: ResetCardQuotaWindow
	let observationError: ResetCardServiceError?
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
	func accounts(
		authority: ResetCardAuthority?
	) async throws -> [ResetCardAccountRecord]
	func inventory(for account: ResetCardAccountRecord) async throws -> ResetCardInventory
	func use(_ attempt: ResetCardUseAttempt) async throws -> ResetCardOperationState
	func status(for attempt: ResetCardUseAttempt) async throws -> ResetCardOperationState
}

extension ResetCardClient {
	func accounts() async throws -> [ResetCardAccountRecord] {
		try await accounts(authority: nil)
	}
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

	func accounts(
		authority: ResetCardAuthority?
	) async throws -> [ResetCardAccountRecord] {
		guard authority.map(Self.isValidAuthority) ?? true else {
			throw ResetCardClientError.invalidResponse
		}
		let processResult = try await run(
			arguments: Self.accountsArguments(authority: authority)
		)
		let document = try decode(
			AccountListDocument.self,
			from: processResult,
			schema: accountCLISchema,
			command: "list"
		)

		switch document.result {
		case .available(let data):
			guard document.outcome == "success", processResult.exitCode == 0 else {
				throw ResetCardClientError.invalidResponse
			}
			return try data.orderedAccounts()
		case .unavailable:
			guard document.outcome == "success", processResult.exitCode == 0 else {
				throw ResetCardClientError.invalidResponse
			}
			throw ResetCardClientError.service(.productStateUnavailable)
		}
	}

	func inventory(for account: ResetCardAccountRecord) async throws -> ResetCardInventory {
		guard Self.isCanonicalAccountID(account.accountID),
			account.authority.map(Self.isValidAuthority) ?? true
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
			guard account.authority.map({ $0 == authority }) ?? true,
				value.accountID == account.accountID
			else {
				throw ResetCardClientError.invalidResponse
			}

			return value
		case .observationFailed(let inventory):
			guard document.outcome == "observation_failed", processResult.exitCode == 1 else {
				throw ResetCardClientError.invalidResponse
			}
			let authority = try document.authority.authority
			let value = try inventory.inventory(authority: authority)
			guard account.authority.map({ $0 == authority }) ?? true,
				value.accountID == account.accountID
			else {
				throw ResetCardClientError.invalidResponse
			}

			return value
		case .unavailable(let error):
			guard document.outcome == "unavailable",
				processResult.exitCode == 1,
				(try? document.authority.authority) != nil
			else {
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

	static func accountsArguments(
		authority: ResetCardAuthority? = nil
	) -> [String] {
		let authority = authority.map(authorityArguments) ?? []
		return authority + ["--output", "json", "account", "list"]
	}

	static func listArguments(
		accountID: String,
		authority: ResetCardAuthority?
	) -> [String] {
		let authority = authority.map(authorityArguments) ?? []
		return authority + [
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
		try decode(
			type,
			from: processResult,
			schema: resetCardCLISchema,
			command: command
		)
	}

	private func decode<Document: Decodable & ResetCardStableDocument>(
		_ type: Document.Type,
		from processResult: ResetCardProcessResult,
		schema: String,
		command: String
	) throws -> Document {
		do {
			let document = try JSONDecoder().decode(Document.self, from: processResult.standardOutput)
			guard document.schema == schema,
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

private struct ResetCardAnyCodingKey: CodingKey {
	let stringValue: String
	let intValue: Int? = nil

	init?(stringValue: String) {
		self.stringValue = stringValue
	}

	init?(intValue: Int) {
		return nil
	}
}

private func rejectUnknownFields(
	in decoder: Decoder,
	allowed: Set<String>
) throws {
	let container = try decoder.container(keyedBy: ResetCardAnyCodingKey.self)
	guard container.allKeys.allSatisfy({ allowed.contains($0.stringValue) }) else {
		throw ResetCardClientError.invalidResponse
	}
}

private func requireExactFields(
	in decoder: Decoder,
	expected: Set<String>
) throws {
	let container = try decoder.container(keyedBy: ResetCardAnyCodingKey.self)
	guard Set(container.allKeys.map(\.stringValue)) == expected else {
		throw ResetCardClientError.invalidResponse
	}
}

private func isBoundedWireText(
	_ value: String,
	maximumBytes: Int
) -> Bool {
	value.isEmpty == false
		&& value.utf8.count <= maximumBytes
		&& value.unicodeScalars.contains {
			$0.properties.generalCategory == .control
		} == false
}

private struct ResetCardResultDocument<Result: Decodable>: Decodable, ResetCardStableDocument {
	let schema: String
	let command: String
	let outcome: String
	let authority: ResetCardAuthorityWire
	let result: Result

	init(from decoder: Decoder) throws {
		try rejectUnknownFields(
			in: decoder,
			allowed: ["schema", "command", "outcome", "authority", "result"]
		)
		let container = try decoder.container(keyedBy: CodingKeys.self)
		schema = try container.decode(String.self, forKey: .schema)
		command = try container.decode(String.self, forKey: .command)
		outcome = try container.decode(String.self, forKey: .outcome)
		authority = try container.decode(ResetCardAuthorityWire.self, forKey: .authority)
		result = try container.decode(Result.self, forKey: .result)
	}

	private enum CodingKeys: String, CodingKey {
		case schema
		case command
		case outcome
		case authority
		case result
	}
}

private struct ResetCardAuthorityWire: Decodable {
	let profileName: String
	let serverID: String

	enum CodingKeys: String, CodingKey {
		case profileName = "profile_name"
		case serverID = "server_id"
	}

	init(from decoder: Decoder) throws {
		try rejectUnknownFields(in: decoder, allowed: ["profile_name", "server_id"])
		let container = try decoder.container(keyedBy: CodingKeys.self)
		profileName = try container.decode(String.self, forKey: .profileName)
		serverID = try container.decode(String.self, forKey: .serverID)
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

private struct AccountListDocument: Decodable, ResetCardStableDocument {
	let schema: String
	let command: String
	let outcome: String
	let result: AccountListWireResult

	init(from decoder: Decoder) throws {
		try rejectUnknownFields(
			in: decoder,
			allowed: ["schema", "command", "outcome", "result"]
		)
		let container = try decoder.container(keyedBy: CodingKeys.self)
		schema = try container.decode(String.self, forKey: .schema)
		command = try container.decode(String.self, forKey: .command)
		outcome = try container.decode(String.self, forKey: .outcome)
		result = try container.decode(AccountListWireResult.self, forKey: .result)
	}

	private enum CodingKeys: String, CodingKey {
		case schema
		case command
		case outcome
		case result
	}
}

private enum AccountListWireResult: Decodable {
	case available(AccountListWireData)
	case unavailable

	init(from decoder: Decoder) throws {
		let outcomeContainer = try decoder.container(keyedBy: OutcomeCodingKeys.self)
		let outcome = try outcomeContainer.decode(String.self, forKey: .outcome)
		switch outcome {
		case "available":
			try rejectUnknownFields(in: decoder, allowed: ["outcome", "data"])
			self = .available(
				try outcomeContainer.decode(AccountListWireData.self, forKey: .data)
			)
		case "unavailable":
			try rejectUnknownFields(in: decoder, allowed: ["outcome"])
			self = .unavailable
		default:
			throw ResetCardClientError.invalidResponse
		}
	}

	private enum OutcomeCodingKeys: String, CodingKey {
		case outcome
		case data
	}
}

private struct AccountListWireData: Decodable {
	let accounts: [ResetCardAccountWire]
	let routing: AccountRoutingWire

	init(from decoder: Decoder) throws {
		try rejectUnknownFields(in: decoder, allowed: ["accounts", "routing"])
		let container = try decoder.container(keyedBy: CodingKeys.self)
		accounts = try container.decode([ResetCardAccountWire].self, forKey: .accounts)
		routing = try container.decode(AccountRoutingWire.self, forKey: .routing)
	}

	func orderedAccounts() throws -> [ResetCardAccountRecord] {
		guard accounts.count <= accountCLIItemLimit else {
			throw ResetCardClientError.invalidResponse
		}
		let records = try accounts.map { try $0.record() }
		var byID = [String: ResetCardAccountRecord]()
		for record in records {
			guard byID.updateValue(record, forKey: record.accountID) == nil else {
				throw ResetCardClientError.invalidResponse
			}
		}
		guard routing.order.count == records.count,
			Set(routing.order) == Set(byID.keys)
		else {
			throw ResetCardClientError.invalidResponse
		}

		return try routing.order.map {
			guard let account = byID[$0] else {
				throw ResetCardClientError.invalidResponse
			}
			return account
		}
	}

	private enum CodingKeys: String, CodingKey {
		case accounts
		case routing
	}
}

private struct AccountRoutingWire: Decodable {
	let order: [String]

	init(from decoder: Decoder) throws {
		try rejectUnknownFields(in: decoder, allowed: ["revision", "mode", "order"])
		let container = try decoder.container(keyedBy: CodingKeys.self)
		let revision = try container.decode(UInt64.self, forKey: .revision)
		let mode = try container.decode(AccountRoutingModeWire.self, forKey: .mode)
		order = try container.decode([String].self, forKey: .order)
		guard revision > 0,
			order.count <= accountCLIItemLimit,
			Set(order).count == order.count,
			order.allSatisfy(ResetCardCLIClient.isCanonicalAccountID)
		else {
			throw ResetCardClientError.invalidResponse
		}
		if case .fixed(let accountID) = mode,
			order.contains(accountID) == false
		{
			throw ResetCardClientError.invalidResponse
		}
	}

	private enum CodingKeys: String, CodingKey {
		case revision
		case mode
		case order
	}
}

private enum AccountRoutingModeWire: Decodable {
	case balanced
	case fixed(String)

	init(from decoder: Decoder) throws {
		let container = try decoder.container(keyedBy: CodingKeys.self)
		switch try container.decode(String.self, forKey: .mode) {
		case "balanced":
			try rejectUnknownFields(in: decoder, allowed: ["mode"])
			self = .balanced
		case "fixed":
			try rejectUnknownFields(in: decoder, allowed: ["mode", "account_id"])
			let accountID = try container.decode(String.self, forKey: .accountID)
			guard ResetCardCLIClient.isCanonicalAccountID(accountID) else {
				throw ResetCardClientError.invalidResponse
			}
			self = .fixed(accountID)
		default:
			throw ResetCardClientError.invalidResponse
		}
	}

	private enum CodingKeys: String, CodingKey {
		case mode
		case accountID = "account_id"
	}
}

private struct ResetCardAccountWire: Decodable, Sendable {
	let accountID: String
	let displayLabel: String
	let enabled: Bool
	let accountRevision: UInt64
	let observedState: ResetCardObservedState
	let lifecycleReadiness: ResetCardLifecycleReadiness
	let credentialBinding: AccountCredentialBindingWire?
	let unsettledOperation: AccountUnsettledOperationWire?
	let fiveHourQuota: ResetCardQuotaWindowWire
	let sevenDayQuota: ResetCardQuotaWindowWire

	enum CodingKeys: String, CodingKey {
		case accountID = "account_id"
		case displayLabel = "display_label"
		case enabled
		case accountRevision = "account_revision"
		case observedState = "observed_state"
		case lifecycleReadiness = "lifecycle_readiness"
		case credentialBinding = "credential_binding"
		case unsettledOperation = "unsettled_operation"
		case fiveHourQuota = "five_hour_quota"
		case sevenDayQuota = "seven_day_quota"
	}

	init(from decoder: Decoder) throws {
		try rejectUnknownFields(
			in: decoder,
			allowed: [
				"account_id",
				"display_label",
				"enabled",
				"account_revision",
				"observed_state",
				"lifecycle_readiness",
				"credential_binding",
				"unsettled_operation",
				"five_hour_quota",
				"seven_day_quota",
			]
		)
		let container = try decoder.container(keyedBy: CodingKeys.self)
		accountID = try container.decode(String.self, forKey: .accountID)
		displayLabel = try container.decode(String.self, forKey: .displayLabel)
		enabled = try container.decode(Bool.self, forKey: .enabled)
		accountRevision = try container.decode(UInt64.self, forKey: .accountRevision)
		observedState = try container.decode(ResetCardObservedState.self, forKey: .observedState)
		lifecycleReadiness = try container.decode(
			ResetCardLifecycleReadiness.self,
			forKey: .lifecycleReadiness
		)
		credentialBinding = try container.decodeIfPresent(
			AccountCredentialBindingWire.self,
			forKey: .credentialBinding
		)
		unsettledOperation = try container.decodeIfPresent(
			AccountUnsettledOperationWire.self,
			forKey: .unsettledOperation
		)
		fiveHourQuota = try container.decode(
			ResetCardQuotaWindowWire.self,
			forKey: .fiveHourQuota
		)
		sevenDayQuota = try container.decode(
			ResetCardQuotaWindowWire.self,
			forKey: .sevenDayQuota
		)
	}

	func record() throws -> ResetCardAccountRecord {
		let fiveHourQuota = try fiveHourQuota.window(expectedDuration: 300)
		let sevenDayQuota = try sevenDayQuota.window(expectedDuration: 10_080)
		guard ResetCardCLIClient.isCanonicalAccountID(accountID),
			isBoundedWireText(displayLabel, maximumBytes: 128),
			accountRevision > 0,
			lifecycleReadiness != .tombstoned,
			lifecycleReadiness != .ready
				|| (credentialBinding != nil && unsettledOperation == nil),
			lifecycleReadiness != .credentialAbsent || credentialBinding == nil,
			(lifecycleReadiness == .operationUnsettled) == (unsettledOperation != nil)
		else {
			throw ResetCardClientError.invalidResponse
		}

		return ResetCardAccountRecord(
			authority: nil,
			accountID: accountID,
			displayLabel: displayLabel,
			accountRevision: accountRevision,
			enabled: enabled,
			observedState: observedState,
			lifecycleReadiness: lifecycleReadiness,
			fiveHourQuota: fiveHourQuota,
			sevenDayQuota: sevenDayQuota
		)
	}
}

private struct AccountCredentialBindingWire: Decodable, Sendable {
	let schemaVersion: UInt16
	let version: UInt64
	let fingerprintSHA256: String
	let provider: String
	let providerAccountID: String

	init(from decoder: Decoder) throws {
		try rejectUnknownFields(
			in: decoder,
			allowed: [
				"schema_version",
				"version",
				"fingerprint_sha256",
				"provider",
				"provider_account_id",
			]
		)
		let container = try decoder.container(keyedBy: CodingKeys.self)
		schemaVersion = try container.decode(UInt16.self, forKey: .schemaVersion)
		version = try container.decode(UInt64.self, forKey: .version)
		fingerprintSHA256 = try container.decode(String.self, forKey: .fingerprintSHA256)
		provider = try container.decode(String.self, forKey: .provider)
		providerAccountID = try container.decode(String.self, forKey: .providerAccountID)
		guard schemaVersion == 1,
			version > 0,
			fingerprintSHA256.utf8.count == 64,
			fingerprintSHA256.utf8.allSatisfy({
				(48...57).contains($0) || (97...102).contains($0)
			}),
			provider == "chatgpt",
			isBoundedWireText(providerAccountID, maximumBytes: 512)
		else {
			throw ResetCardClientError.invalidResponse
		}
	}

	private enum CodingKeys: String, CodingKey {
		case schemaVersion = "schema_version"
		case version
		case fingerprintSHA256 = "fingerprint_sha256"
		case provider
		case providerAccountID = "provider_account_id"
	}
}

private struct AccountUnsettledOperationWire: Decodable, Sendable {
	let operationID: String
	let kind: AccountOperationKindWire
	let phase: AccountOperationPhaseWire
	let recoveryCode: String?

	init(from decoder: Decoder) throws {
		try rejectUnknownFields(
			in: decoder,
			allowed: ["operation_id", "kind", "phase", "recovery_code"]
		)
		let container = try decoder.container(keyedBy: CodingKeys.self)
		operationID = try container.decode(String.self, forKey: .operationID)
		kind = try container.decode(AccountOperationKindWire.self, forKey: .kind)
		phase = try container.decode(AccountOperationPhaseWire.self, forKey: .phase)
		recoveryCode = try container.decodeIfPresent(String.self, forKey: .recoveryCode)
		guard ResetCardCLIClient.isCanonicalUUID(operationID),
			(recoveryCode.map {
				isBoundedWireText($0, maximumBytes: 128)
			} ?? true),
			(phase == .recoveryRequired) == (recoveryCode != nil)
		else {
			throw ResetCardClientError.invalidResponse
		}
	}

	private enum CodingKeys: String, CodingKey {
		case operationID = "operation_id"
		case kind
		case phase
		case recoveryCode = "recovery_code"
	}
}

private enum AccountOperationKindWire: String, Decodable, Sendable {
	case enroll
	case `import`
	case refresh
	case logout
}

private enum AccountOperationPhaseWire: String, Decodable, Sendable {
	case prepared
	case providerEffectPending = "provider_effect_pending"
	case storeApplied = "store_applied"
	case recoveryRequired = "recovery_required"
}

private struct ResetCardQuotaWindowWire: Decodable, Sendable {
	let durationMinutes: UInt32
	let observedAtUnixMicros: Int64?
	let result: ResetCardQuotaStateWire

	init(from decoder: Decoder) throws {
		try rejectUnknownFields(
			in: decoder,
			allowed: ["duration_minutes", "observed_at_unix_micros", "result"]
		)
		let container = try decoder.container(keyedBy: CodingKeys.self)
		durationMinutes = try container.decode(UInt32.self, forKey: .durationMinutes)
		guard container.contains(.observedAtUnixMicros) else {
			throw ResetCardClientError.invalidResponse
		}
		observedAtUnixMicros = try container.decodeIfPresent(
			Int64.self,
			forKey: .observedAtUnixMicros
		)
		result = try container.decode(ResetCardQuotaStateWire.self, forKey: .result)
	}

	func window(expectedDuration: UInt32) throws -> ResetCardQuotaWindow {
		guard durationMinutes == expectedDuration else {
			throw ResetCardClientError.invalidResponse
		}
		let state = try result.state(observedAtUnixMicros: observedAtUnixMicros)
		return ResetCardQuotaWindow(
			durationMinutes: durationMinutes,
			observedAtUnixMicros: observedAtUnixMicros,
			state: state
		)
	}

	private enum CodingKeys: String, CodingKey {
		case durationMinutes = "duration_minutes"
		case observedAtUnixMicros = "observed_at_unix_micros"
		case result
	}
}

private enum ResetCardQuotaStateWire: Decodable, Sendable {
	case unknown
	case current(ResetCardQuotaValueWire)
	case stale(ResetCardQuotaValueWire)
	case error(ResetCardQuotaError)

	init(from decoder: Decoder) throws {
		let container = try decoder.container(keyedBy: CodingKeys.self)
		switch try container.decode(String.self, forKey: .state) {
		case "unknown":
			try rejectUnknownFields(in: decoder, allowed: ["state"])
			self = .unknown
		case "current":
			try rejectUnknownFields(in: decoder, allowed: ["state", "data"])
			self = .current(
				try container.decode(ResetCardQuotaValueWire.self, forKey: .data)
			)
		case "stale":
			try rejectUnknownFields(in: decoder, allowed: ["state", "data"])
			self = .stale(
				try container.decode(ResetCardQuotaValueWire.self, forKey: .data)
			)
		case "error":
			try rejectUnknownFields(in: decoder, allowed: ["state", "data"])
			let data = try container.decode(ResetCardQuotaErrorWire.self, forKey: .data)
			self = .error(data.error)
		default:
			throw ResetCardClientError.invalidResponse
		}
	}

	func state(observedAtUnixMicros: Int64?) throws -> ResetCardQuotaState {
		switch self {
		case .unknown:
			guard observedAtUnixMicros == nil else {
				throw ResetCardClientError.invalidResponse
			}
			return .unknown
		case .current(let value):
			let observed = try validatedObservation(
				observedAtUnixMicros,
				value: value
			)
			guard value.resetsAtUnixMicros > observed else {
				throw ResetCardClientError.invalidResponse
			}
			return .current(
				usedPercent: value.usedPercent,
				resetsAtUnixMicros: value.resetsAtUnixMicros
			)
		case .stale(let value):
			let observed = try validatedObservation(
				observedAtUnixMicros,
				value: value
			)
			guard value.resetsAtUnixMicros > observed else {
				throw ResetCardClientError.invalidResponse
			}
			return .stale(
				usedPercent: value.usedPercent,
				resetsAtUnixMicros: value.resetsAtUnixMicros
			)
		case .error(let error):
			guard let observedAtUnixMicros, observedAtUnixMicros > 0 else {
				throw ResetCardClientError.invalidResponse
			}
			return .error(error)
		}
	}

	private func validatedObservation(
		_ observedAtUnixMicros: Int64?,
		value: ResetCardQuotaValueWire
	) throws -> Int64 {
		guard let observedAtUnixMicros,
			observedAtUnixMicros > 0,
			value.usedPercent <= 100
		else {
			throw ResetCardClientError.invalidResponse
		}
		return observedAtUnixMicros
	}

	private enum CodingKeys: String, CodingKey {
		case state
		case data
	}
}

private struct ResetCardQuotaValueWire: Decodable, Sendable {
	let usedPercent: UInt8
	let resetsAtUnixMicros: Int64

	init(from decoder: Decoder) throws {
		try rejectUnknownFields(
			in: decoder,
			allowed: ["used_percent", "resets_at_unix_micros"]
		)
		let container = try decoder.container(keyedBy: CodingKeys.self)
		usedPercent = try container.decode(UInt8.self, forKey: .usedPercent)
		resetsAtUnixMicros = try container.decode(Int64.self, forKey: .resetsAtUnixMicros)
	}

	private enum CodingKeys: String, CodingKey {
		case usedPercent = "used_percent"
		case resetsAtUnixMicros = "resets_at_unix_micros"
	}
}

private struct ResetCardQuotaErrorWire: Decodable, Sendable {
	let error: ResetCardQuotaError

	init(from decoder: Decoder) throws {
		try rejectUnknownFields(in: decoder, allowed: ["error"])
		let container = try decoder.container(keyedBy: CodingKeys.self)
		error = try container.decode(ResetCardQuotaError.self, forKey: .error)
	}

	private enum CodingKeys: String, CodingKey {
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

	init(from decoder: Decoder) throws {
		try rejectUnknownFields(
			in: decoder,
			allowed: ["granted_at_unix_seconds", "expires_at_unix_seconds"]
		)
		let container = try decoder.container(keyedBy: CodingKeys.self)
		grantedAtUnixSeconds = try container.decode(
			Int64.self,
			forKey: .grantedAtUnixSeconds
		)
		expiresAtUnixSeconds = try container.decode(
			Int64.self,
			forKey: .expiresAtUnixSeconds
		)
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

	init(from decoder: Decoder) throws {
		try rejectUnknownFields(in: decoder, allowed: ["descriptor"])
		let container = try decoder.container(keyedBy: CodingKeys.self)
		descriptor = try container.decode(ResetCardDescriptorWire.self, forKey: .descriptor)
	}

	private enum CodingKeys: String, CodingKey {
		case descriptor
	}
}

private struct ResetCardAvailableInventoryWireData: Decodable, Sendable {
	let accountID: String
	let accountRevision: UInt64
	let availableCount: UInt16
	let cards: [ResetCardObservationWire]
	let fiveHourQuota: ResetCardQuotaWindowWire
	let sevenDayQuota: ResetCardQuotaWindowWire

	enum CodingKeys: String, CodingKey {
		case accountID = "account_id"
		case accountRevision = "account_revision"
		case availableCount = "available_count"
		case cards
		case fiveHourQuota = "five_hour_quota"
		case sevenDayQuota = "seven_day_quota"
	}

	init(from decoder: Decoder) throws {
		try rejectUnknownFields(
			in: decoder,
			allowed: [
				"account_id",
				"account_revision",
				"available_count",
				"cards",
				"five_hour_quota",
				"seven_day_quota",
			]
		)
		let container = try decoder.container(keyedBy: CodingKeys.self)
		accountID = try container.decode(String.self, forKey: .accountID)
		accountRevision = try container.decode(UInt64.self, forKey: .accountRevision)
		availableCount = try container.decode(UInt16.self, forKey: .availableCount)
		cards = try container.decode([ResetCardObservationWire].self, forKey: .cards)
		fiveHourQuota = try container.decode(
			ResetCardQuotaWindowWire.self,
			forKey: .fiveHourQuota
		)
		sevenDayQuota = try container.decode(
			ResetCardQuotaWindowWire.self,
			forKey: .sevenDayQuota
		)
	}

	func inventory(authority: ResetCardAuthority) throws -> ResetCardInventory {
		guard ResetCardCLIClient.isValidAuthority(authority),
			ResetCardCLIClient.isCanonicalAccountID(accountID),
			accountRevision > 0,
			cards.count == Int(availableCount),
			cards.count <= resetCardCLIItemLimit
		else {
			throw ResetCardClientError.invalidResponse
		}

		let descriptors = try cards.map { try $0.descriptor.descriptor }
		let fiveHourQuota = try fiveHourQuota.window(expectedDuration: 300)
		let sevenDayQuota = try sevenDayQuota.window(expectedDuration: 10_080)
		guard Set(descriptors).count == descriptors.count else {
			throw ResetCardClientError.invalidResponse
		}

		return ResetCardInventory(
			authority: authority,
			accountID: accountID,
			accountRevision: accountRevision,
			cards: descriptors,
			fiveHourQuota: fiveHourQuota,
			sevenDayQuota: sevenDayQuota,
			observationError: nil
		)
	}
}

private struct ResetCardFailedInventoryWireData: Decodable, Sendable {
	let accountID: String
	let accountRevision: UInt64
	let fiveHourQuota: ResetCardQuotaWindowWire
	let sevenDayQuota: ResetCardQuotaWindowWire
	let error: ResetCardServiceError

	init(from decoder: Decoder) throws {
		try rejectUnknownFields(
			in: decoder,
			allowed: [
				"account_id",
				"account_revision",
				"five_hour_quota",
				"seven_day_quota",
				"error",
			]
		)
		let container = try decoder.container(keyedBy: CodingKeys.self)
		accountID = try container.decode(String.self, forKey: .accountID)
		accountRevision = try container.decode(UInt64.self, forKey: .accountRevision)
		fiveHourQuota = try container.decode(
			ResetCardQuotaWindowWire.self,
			forKey: .fiveHourQuota
		)
		sevenDayQuota = try container.decode(
			ResetCardQuotaWindowWire.self,
			forKey: .sevenDayQuota
		)
		error = try container.decode(ResetCardServiceError.self, forKey: .error)
	}

	func inventory(authority: ResetCardAuthority) throws -> ResetCardInventory {
		guard ResetCardCLIClient.isValidAuthority(authority),
			ResetCardCLIClient.isCanonicalAccountID(accountID),
			accountRevision > 0
		else {
			throw ResetCardClientError.invalidResponse
		}
		return ResetCardInventory(
			authority: authority,
			accountID: accountID,
			accountRevision: accountRevision,
			cards: [],
			fiveHourQuota: try fiveHourQuota.window(expectedDuration: 300),
			sevenDayQuota: try sevenDayQuota.window(expectedDuration: 10_080),
			observationError: error
		)
	}

	private enum CodingKeys: String, CodingKey {
		case accountID = "account_id"
		case accountRevision = "account_revision"
		case fiveHourQuota = "five_hour_quota"
		case sevenDayQuota = "seven_day_quota"
		case error
	}
}

private struct ResetCardUnavailableInventoryWireData: Decodable, Sendable {
	let error: ResetCardServiceError

	init(from decoder: Decoder) throws {
		try rejectUnknownFields(in: decoder, allowed: ["error"])
		let container = try decoder.container(keyedBy: CodingKeys.self)
		error = try container.decode(ResetCardServiceError.self, forKey: .error)
	}

	private enum CodingKeys: String, CodingKey {
		case error
	}
}

private enum ResetCardInventoryWireResult: Decodable, Sendable {
	case available(ResetCardAvailableInventoryWireData)
	case observationFailed(ResetCardFailedInventoryWireData)
	case unavailable(ResetCardServiceError)

	init(from decoder: Decoder) throws {
		let container = try decoder.container(keyedBy: CodingKeys.self)
		switch try container.decode(String.self, forKey: .outcome) {
		case "available":
			try rejectUnknownFields(in: decoder, allowed: ["outcome", "data"])
			self = .available(
				try container.decode(ResetCardAvailableInventoryWireData.self, forKey: .data)
			)
		case "observation_failed":
			try rejectUnknownFields(in: decoder, allowed: ["outcome", "data"])
			self = .observationFailed(
				try container.decode(ResetCardFailedInventoryWireData.self, forKey: .data)
			)
		case "unavailable":
			try rejectUnknownFields(in: decoder, allowed: ["outcome", "data"])
			let data = try container.decode(
				ResetCardUnavailableInventoryWireData.self,
				forKey: .data
			)
			self = .unavailable(data.error)
		default:
			throw ResetCardClientError.invalidResponse
		}
	}

	private enum CodingKeys: String, CodingKey {
		case outcome
		case data
	}
}

private enum ResetCardOperationWireResult: Decodable, Sendable {
	case value(ResetCardOperationState)

	init(from decoder: Decoder) throws {
		let container = try decoder.container(keyedBy: CodingKeys.self)
		let state = try container.decode(String.self, forKey: .state)
		switch state {
		case "not_found":
			try requireExactFields(in: decoder, expected: ["state"])
			self = .value(.notFound)
		case "prepared":
			try requireExactFields(in: decoder, expected: ["state"])
			self = .value(.prepared)
		case "effect_ambiguous":
			try requireExactFields(in: decoder, expected: ["state"])
			self = .value(.effectAmbiguous)
		case "completed":
			try requireExactFields(in: decoder, expected: ["state", "data"])
			let data = try container.decode(CompletedData.self, forKey: .data)
			self = .value(.completed(data.outcome))
		case "failed_before_effect":
			try requireExactFields(in: decoder, expected: ["state", "data"])
			let data = try container.decode(FailedData.self, forKey: .data)
			self = .value(.failedBeforeEffect(data.error))
		case "unavailable":
			try requireExactFields(in: decoder, expected: ["state", "data"])
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

		init(from decoder: Decoder) throws {
			try rejectUnknownFields(in: decoder, allowed: ["outcome"])
			let container = try decoder.container(keyedBy: CodingKeys.self)
			outcome = try container.decode(ResetCardUseOutcome.self, forKey: .outcome)
		}

		private enum CodingKeys: String, CodingKey {
			case outcome
		}
	}

	private struct FailedData: Decodable {
		let error: ResetCardServiceError

		init(from decoder: Decoder) throws {
			try rejectUnknownFields(in: decoder, allowed: ["error"])
			let container = try decoder.container(keyedBy: CodingKeys.self)
			error = try container.decode(ResetCardServiceError.self, forKey: .error)
		}

		private enum CodingKeys: String, CodingKey {
			case error
		}
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

	init(from decoder: Decoder) throws {
		let container = try decoder.container(keyedBy: CodingKeys.self)
		schema = try container.decode(String.self, forKey: .schema)
		command = try container.decode(String.self, forKey: .command)
		outcome = try container.decode(String.self, forKey: .outcome)
		idempotencyKey = try container.decode(String.self, forKey: .idempotencyKey)
		let decodedDispatchState = try container.decode(
			ResetCardUseDispatchState.self,
			forKey: .dispatchState
		)
		dispatchState = decodedDispatchState

		let baseFields: Set<String> = [
			"schema",
			"command",
			"outcome",
			"idempotency_key",
			"dispatch_state",
		]
		switch decodedDispatchState {
		case .durablyAccepted:
			try requireExactFields(
				in: decoder,
				expected: baseFields.union([
					"account_id",
					"descriptor",
					"account_revision",
					"state",
				])
			)
			accountID = try container.decode(String.self, forKey: .accountID)
			descriptor = try container.decode(
				ResetCardDescriptorWire.self,
				forKey: .descriptor
			)
			accountRevision = try container.decode(UInt64.self, forKey: .accountRevision)
			state = try container.decode(ResetCardOperationWireResult.self, forKey: .state)
			error = nil
			failure = nil
		case .rejectedBeforeAcceptance:
			try requireExactFields(
				in: decoder,
				expected: baseFields.union(["error"])
			)
			accountID = nil
			descriptor = nil
			accountRevision = nil
			state = nil
			error = try container.decode(ResetCardCommandErrorWire.self, forKey: .error)
			failure = nil
		case .definitelyNotDispatched, .potentiallyDispatched:
			try requireExactFields(
				in: decoder,
				expected: baseFields.union(["failure"])
			)
			accountID = nil
			descriptor = nil
			accountRevision = nil
			state = nil
			error = nil
			failure = try container.decode(ResetCardClientFailureWire.self, forKey: .failure)
		}
	}

	private enum CodingKeys: String, CodingKey {
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
	case localTransportDisabled = "local_transport_disabled"
	case remoteTransportDisabled = "remote_transport_disabled"
	case localTransportUnsupported = "local_transport_unsupported"
	case unsafeLocalEndpoint = "unsafe_local_endpoint"
	case localPeerIdentityUnavailable = "local_peer_identity_unavailable"
	case localPeerUIDMismatch = "local_peer_uid_mismatch"
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
	case acceptanceUnknown
	case accountCommandRejected

	init(from decoder: Decoder) throws {
		let container = try decoder.container(keyedBy: CodingKeys.self)
		switch try container.decode(String.self, forKey: .reason) {
		case "expected_revision_mismatch":
			try rejectUnknownFields(
				in: decoder,
				allowed: ["reason", "expected", "actual"]
			)
			guard try container.decode(UInt64.self, forKey: .expected) > 0,
				try container.decode(UInt64.self, forKey: .actual) > 0
			else {
				throw ResetCardClientError.invalidResponse
			}
			self = .expectedRevisionMismatch
		case "idempotency_conflict":
			try rejectUnknownFields(in: decoder, allowed: ["reason"])
			self = .idempotencyConflict
		case "idempotency_capacity_exceeded":
			try rejectUnknownFields(in: decoder, allowed: ["reason", "capacity"])
			guard try container.decode(UInt64.self, forKey: .capacity) > 0 else {
				throw ResetCardClientError.invalidResponse
			}
			self = .idempotencyCapacityExceeded
		case "application_unavailable":
			try rejectUnknownFields(in: decoder, allowed: ["reason", "message"])
			let message = try container.decode(String.self, forKey: .message)
			guard isBoundedWireText(message, maximumBytes: 4_096) else {
				throw ResetCardClientError.invalidResponse
			}
			self = .applicationUnavailable
		case "acceptance_unknown":
			try rejectUnknownFields(in: decoder, allowed: ["reason"])
			self = .acceptanceUnknown
		case "account_command_rejected":
			try rejectUnknownFields(
				in: decoder,
				allowed: ["reason", "rejection", "actual_revision"]
			)
			_ = try container.decode(
				ResetCardAccountCommandRejectionWire.self,
				forKey: .rejection
			)
			if let actual = try container.decodeIfPresent(
				UInt64.self,
				forKey: .actualRevision
			), actual == 0 {
				throw ResetCardClientError.invalidResponse
			}
			self = .accountCommandRejected
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
		case rejection
		case actualRevision = "actual_revision"
	}
}

private enum ResetCardAccountCommandRejectionWire: String, Decodable {
	case invalidRequest = "invalid_request"
	case accountNotFound = "account_not_found"
	case staleAccount = "stale_account"
	case staleRoutingControl = "stale_routing_control"
	case accountInUse = "account_in_use"
	case operationUnsettled = "operation_unsettled"
	case operationNotFound = "operation_not_found"
	case credentialAbsent = "credential_absent"
	case credentialStoreUnavailable = "credential_store_unavailable"
	case providerMismatch = "provider_mismatch"
	case lifecycleUnready = "lifecycle_unready"
	case routingOrderInvalid = "routing_order_invalid"
	case manualRecoveryRequired = "manual_recovery_required"
}

private struct ResetCardStatusDocument: Decodable, ResetCardStableDocument {
	let schema: String
	let command: String
	let outcome: String
	let idempotencyKey: String
	let state: ResetCardOperationWireResult

	init(from decoder: Decoder) throws {
		try rejectUnknownFields(
			in: decoder,
			allowed: ["schema", "command", "outcome", "idempotency_key", "state"]
		)
		let container = try decoder.container(keyedBy: CodingKeys.self)
		schema = try container.decode(String.self, forKey: .schema)
		command = try container.decode(String.self, forKey: .command)
		outcome = try container.decode(String.self, forKey: .outcome)
		idempotencyKey = try container.decode(String.self, forKey: .idempotencyKey)
		state = try container.decode(ResetCardOperationWireResult.self, forKey: .state)
	}

	private enum CodingKeys: String, CodingKey {
		case schema
		case command
		case outcome
		case idempotencyKey = "idempotency_key"
		case state
	}
}
