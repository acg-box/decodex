import Foundation

enum AccountCredentialProvider: String, Equatable, Sendable {
	case chatGPT = "chatgpt"
}

struct AccountCredentialBinding: Equatable, Sendable {
	let schemaVersion: UInt16
	let version: UInt64
	let fingerprintSHA256: String
	let provider: AccountCredentialProvider
	let providerAccountID: String
}

enum AccountOperationKind: String, Equatable, Sendable {
	case enroll
	case `import`
	case refresh
	case logout
}

enum AccountOperationPhase: String, Equatable, Sendable {
	case prepared
	case providerEffectPending = "provider_effect_pending"
	case storeApplied = "store_applied"
	case recoveryRequired = "recovery_required"
}

struct AccountUnsettledOperation: Equatable, Sendable {
	let operationID: String
	let kind: AccountOperationKind
	let phase: AccountOperationPhase
	let recoveryCode: String?
}

enum AccountRoutingMode: Equatable, Sendable {
	case balanced
	case fixed(accountID: String)
}

struct AccountRoutingControl: Equatable, Sendable {
	let revision: UInt64
	let mode: AccountRoutingMode
	let order: [String]
}

struct AccountControlSnapshot: Equatable, Sendable {
	let authority: ResetCardAuthority?
	let accounts: [ResetCardAccountRecord]
	let routing: AccountRoutingControl
}

enum AccountControlResult: Equatable, Sendable {
	case accountChanged(ResetCardAccountRecord)
	case accountLoggedOut(accountID: String, tombstoneRevision: UInt64)
	case routingChanged(AccountRoutingControl)
}

enum AccountControlRejection: String, Decodable, Equatable, Sendable {
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

	var presentation: String {
		switch self {
		case .invalidRequest:
			return "The account request was invalid."
		case .accountNotFound:
			return "The account no longer exists."
		case .staleAccount:
			return "The account changed. Refresh and try again."
		case .staleRoutingControl:
			return "Account routing changed. Refresh and try again."
		case .accountInUse:
			return "The account is in use and cannot be logged out."
		case .operationUnsettled:
			return "Another account operation is still pending."
		case .operationNotFound:
			return "The account operation no longer exists."
		case .credentialAbsent:
			return "The account has no credentials."
		case .credentialStoreUnavailable:
			return "The credential store is unavailable."
		case .providerMismatch:
			return "The provider account does not match."
		case .lifecycleUnready:
			return "The account is not ready for this action."
		case .routingOrderInvalid:
			return "The account routing order is invalid."
		case .manualRecoveryRequired:
			return "The account needs manual recovery."
		}
	}
}

enum AccountControlError: Error, Equatable, LocalizedError, Sendable {
	case invalidInput
	case invalidResponse
	case client(ResetCardClientError)
	case expectedRevisionMismatch(expected: UInt64, actual: UInt64)
	case idempotencyConflict
	case idempotencyCapacityExceeded
	case applicationUnavailable
	case acceptanceUnknown
	case rejected(AccountControlRejection, actualRevision: UInt64?)
	case potentiallyDispatched

	var errorDescription: String? {
		switch self {
		case .invalidInput:
			return "The account action contains invalid input."
		case .invalidResponse:
			return "The Decodex CLI returned an invalid account response."
		case .client(let error):
			switch error {
			case .executableMissing:
				return "The bundled Decodex CLI is unavailable."
			case .launchFailed:
				return "The Decodex CLI could not start."
			case .timedOut:
				return "The account request timed out."
			case .outputTooLarge:
				return "The Decodex CLI returned too much account data."
			case .commandRejected, .useDefinitelyNotDispatched,
				.usePotentiallyDispatched, .commandFailed, .invalidResponse,
				.service:
				return "The Decodex account service is unavailable."
			}
		case .expectedRevisionMismatch:
			return "The account changed. Refresh and try again."
		case .idempotencyConflict:
			return "The account action key conflicts with another request."
		case .idempotencyCapacityExceeded:
			return "The account service cannot accept another action yet."
		case .applicationUnavailable:
			return "The account service is unavailable."
		case .acceptanceUnknown, .potentiallyDispatched:
			return "The account action may have been accepted. Refresh authoritative state before trying again."
		case .rejected(let rejection, _):
			return rejection.presentation
		}
	}
}

protocol AccountControlClient: ResetCardClient {
	func accountSnapshot(
		authority: ResetCardAuthority?
	) async throws -> AccountControlSnapshot

	func enrollFromSharedCodex(
		authority: ResetCardAuthority?,
		operationID: String,
		accountID: String,
		displayLabel: String,
		enabled: Bool,
		idempotencyKey: String
	) async throws -> AccountControlResult

	func renameAccount(
		authority: ResetCardAuthority?,
		accountID: String,
		displayLabel: String,
		expectedRevision: UInt64,
		idempotencyKey: String
	) async throws -> AccountControlResult

	func setAccountEnabled(
		authority: ResetCardAuthority?,
		accountID: String,
		enabled: Bool,
		expectedRevision: UInt64,
		idempotencyKey: String
	) async throws -> AccountControlResult

	func logoutAccount(
		authority: ResetCardAuthority?,
		operationID: String,
		accountID: String,
		expectedRevision: UInt64,
		idempotencyKey: String
	) async throws -> AccountControlResult

	func setFixedSelection(
		authority: ResetCardAuthority?,
		accountID: String,
		expectedAccountRevision: UInt64,
		expectedRoutingRevision: UInt64,
		idempotencyKey: String
	) async throws -> AccountControlResult

	func setBalancedSelection(
		authority: ResetCardAuthority?,
		expectedRoutingRevision: UInt64,
		idempotencyKey: String
	) async throws -> AccountControlResult

	func refreshAccountCredentials(
		authority: ResetCardAuthority?,
		operationID: String,
		accountID: String,
		expectedRevision: UInt64,
		idempotencyKey: String
	) async throws -> AccountControlResult
}

extension ResetCardCLIClient: AccountControlClient {
	func enrollFromSharedCodex(
		authority: ResetCardAuthority?,
		operationID: String,
		accountID: String,
		displayLabel: String,
		enabled: Bool,
		idempotencyKey: String
	) async throws -> AccountControlResult {
		let arguments = try Self.enrollFromSharedCodexArguments(
			authority: authority,
			operationID: operationID,
			accountID: accountID,
			displayLabel: displayLabel,
			enabled: enabled,
			idempotencyKey: idempotencyKey
		)
		return try await executeAccountControl(
			arguments: arguments,
			authority: authority,
			expected: .accountChanged(
				accountID: accountID,
				displayLabel: displayLabel,
				enabled: enabled
			)
		)
	}

	func renameAccount(
		authority: ResetCardAuthority?,
		accountID: String,
		displayLabel: String,
		expectedRevision: UInt64,
		idempotencyKey: String
	) async throws -> AccountControlResult {
		let arguments = try Self.renameAccountArguments(
			authority: authority,
			accountID: accountID,
			displayLabel: displayLabel,
			expectedRevision: expectedRevision,
			idempotencyKey: idempotencyKey
		)
		return try await executeAccountControl(
			arguments: arguments,
			authority: authority,
			expected: .accountChanged(
				accountID: accountID,
				displayLabel: displayLabel,
				enabled: nil
			)
		)
	}

	func setAccountEnabled(
		authority: ResetCardAuthority?,
		accountID: String,
		enabled: Bool,
		expectedRevision: UInt64,
		idempotencyKey: String
	) async throws -> AccountControlResult {
		let arguments = try Self.setAccountEnabledArguments(
			authority: authority,
			accountID: accountID,
			enabled: enabled,
			expectedRevision: expectedRevision,
			idempotencyKey: idempotencyKey
		)
		return try await executeAccountControl(
			arguments: arguments,
			authority: authority,
			expected: .accountChanged(
				accountID: accountID,
				displayLabel: nil,
				enabled: enabled
			)
		)
	}

	func logoutAccount(
		authority: ResetCardAuthority?,
		operationID: String,
		accountID: String,
		expectedRevision: UInt64,
		idempotencyKey: String
	) async throws -> AccountControlResult {
		let arguments = try Self.logoutAccountArguments(
			authority: authority,
			operationID: operationID,
			accountID: accountID,
			expectedRevision: expectedRevision,
			idempotencyKey: idempotencyKey
		)
		return try await executeAccountControl(
			arguments: arguments,
			authority: authority,
			expected: .accountLoggedOut(accountID: accountID)
		)
	}

	func setFixedSelection(
		authority: ResetCardAuthority?,
		accountID: String,
		expectedAccountRevision: UInt64,
		expectedRoutingRevision: UInt64,
		idempotencyKey: String
	) async throws -> AccountControlResult {
		let arguments = try Self.setFixedSelectionArguments(
			authority: authority,
			accountID: accountID,
			expectedAccountRevision: expectedAccountRevision,
			expectedRoutingRevision: expectedRoutingRevision,
			idempotencyKey: idempotencyKey
		)
		return try await executeAccountControl(
			arguments: arguments,
			authority: authority,
			expected: .routing(mode: .fixed(accountID: accountID))
		)
	}

	func setBalancedSelection(
		authority: ResetCardAuthority?,
		expectedRoutingRevision: UInt64,
		idempotencyKey: String
	) async throws -> AccountControlResult {
		let arguments = try Self.setBalancedSelectionArguments(
			authority: authority,
			expectedRoutingRevision: expectedRoutingRevision,
			idempotencyKey: idempotencyKey
		)
		return try await executeAccountControl(
			arguments: arguments,
			authority: authority,
			expected: .routing(mode: .balanced)
		)
	}

	func refreshAccountCredentials(
		authority: ResetCardAuthority?,
		operationID: String,
		accountID: String,
		expectedRevision: UInt64,
		idempotencyKey: String
	) async throws -> AccountControlResult {
		let arguments = try Self.refreshAccountCredentialsArguments(
			authority: authority,
			operationID: operationID,
			accountID: accountID,
			expectedRevision: expectedRevision,
			idempotencyKey: idempotencyKey
		)
		return try await executeAccountControl(
			arguments: arguments,
			authority: authority,
			expected: .accountChanged(
				accountID: accountID,
				displayLabel: nil,
				enabled: nil
			)
		)
	}
}

extension ResetCardCLIClient {
	static func enrollFromSharedCodexArguments(
		authority: ResetCardAuthority?,
		operationID: String,
		accountID: String,
		displayLabel: String,
		enabled: Bool,
		idempotencyKey: String
	) throws -> [String] {
		try validateAccountControlInput(
			authority: authority,
			accountID: accountID,
			operationID: operationID,
			displayLabel: displayLabel,
			expectedRevision: nil,
			idempotencyKey: idempotencyKey
		)
		return accountControlPrefix(authority: authority) + [
			"enroll",
			"--operation-id", operationID,
			"--account-id", accountID,
			"--label", displayLabel,
			"--enabled", enabled ? "true" : "false",
			"--idempotency-key", idempotencyKey,
		]
	}

	static func renameAccountArguments(
		authority: ResetCardAuthority?,
		accountID: String,
		displayLabel: String,
		expectedRevision: UInt64,
		idempotencyKey: String
	) throws -> [String] {
		try validateAccountControlInput(
			authority: authority,
			accountID: accountID,
			operationID: nil,
			displayLabel: displayLabel,
			expectedRevision: expectedRevision,
			idempotencyKey: idempotencyKey
		)
		return accountControlPrefix(authority: authority) + [
			"rename",
			"--account-id", accountID,
			"--label", displayLabel,
			"--expected-revision", String(expectedRevision),
			"--idempotency-key", idempotencyKey,
		]
	}

	static func setAccountEnabledArguments(
		authority: ResetCardAuthority?,
		accountID: String,
		enabled: Bool,
		expectedRevision: UInt64,
		idempotencyKey: String
	) throws -> [String] {
		try validateAccountControlInput(
			authority: authority,
			accountID: accountID,
			operationID: nil,
			displayLabel: nil,
			expectedRevision: expectedRevision,
			idempotencyKey: idempotencyKey
		)
		return accountControlPrefix(authority: authority) + [
			enabled ? "enable" : "disable",
			"--account-id", accountID,
			"--expected-revision", String(expectedRevision),
			"--idempotency-key", idempotencyKey,
		]
	}

	static func logoutAccountArguments(
		authority: ResetCardAuthority?,
		operationID: String,
		accountID: String,
		expectedRevision: UInt64,
		idempotencyKey: String
	) throws -> [String] {
		try validateAccountControlInput(
			authority: authority,
			accountID: accountID,
			operationID: operationID,
			displayLabel: nil,
			expectedRevision: expectedRevision,
			idempotencyKey: idempotencyKey
		)
		return accountControlPrefix(authority: authority) + [
			"logout",
			"--operation-id", operationID,
			"--account-id", accountID,
			"--expected-revision", String(expectedRevision),
			"--idempotency-key", idempotencyKey,
		]
	}

	static func setFixedSelectionArguments(
		authority: ResetCardAuthority?,
		accountID: String,
		expectedAccountRevision: UInt64,
		expectedRoutingRevision: UInt64,
		idempotencyKey: String
	) throws -> [String] {
		try validateAccountControlInput(
			authority: authority,
			accountID: accountID,
			operationID: nil,
			displayLabel: nil,
			expectedRevision: expectedAccountRevision,
			idempotencyKey: idempotencyKey
		)
		guard expectedRoutingRevision > 0 else {
			throw AccountControlError.invalidInput
		}
		return accountControlPrefix(authority: authority) + [
			"set-fixed-selection",
			"--account-id", accountID,
			"--expected-account-revision", String(expectedAccountRevision),
			"--expected-revision", String(expectedRoutingRevision),
			"--idempotency-key", idempotencyKey,
		]
	}

	static func setBalancedSelectionArguments(
		authority: ResetCardAuthority?,
		expectedRoutingRevision: UInt64,
		idempotencyKey: String
	) throws -> [String] {
		guard authority.map(isValidAuthority) ?? true,
			expectedRoutingRevision > 0,
			isCanonicalUUID(idempotencyKey)
		else {
			throw AccountControlError.invalidInput
		}
		return accountControlPrefix(authority: authority) + [
			"set-balanced-selection",
			"--expected-revision", String(expectedRoutingRevision),
			"--idempotency-key", idempotencyKey,
		]
	}

	static func refreshAccountCredentialsArguments(
		authority: ResetCardAuthority?,
		operationID: String,
		accountID: String,
		expectedRevision: UInt64,
		idempotencyKey: String
	) throws -> [String] {
		try validateAccountControlInput(
			authority: authority,
			accountID: accountID,
			operationID: operationID,
			displayLabel: nil,
			expectedRevision: expectedRevision,
			idempotencyKey: idempotencyKey
		)
		return accountControlPrefix(authority: authority) + [
			"refresh",
			"--operation-id", operationID,
			"--account-id", accountID,
			"--expected-revision", String(expectedRevision),
			"--idempotency-key", idempotencyKey,
		]
	}

	private static func accountControlPrefix(
		authority: ResetCardAuthority?
	) -> [String] {
		(authority.map(authorityArguments) ?? []) + ["--output", "json", "account"]
	}

	private static func validateAccountControlInput(
		authority: ResetCardAuthority?,
		accountID: String,
		operationID: String?,
		displayLabel: String?,
		expectedRevision: UInt64?,
		idempotencyKey: String
	) throws {
		guard authority.map(isValidAuthority) ?? true,
			isCanonicalAccountID(accountID),
			operationID.map(isCanonicalUUID) ?? true,
			displayLabel.map({
				isBoundedWireText($0, maximumBytes: 128)
			}) ?? true,
			expectedRevision.map({ $0 > 0 }) ?? true,
			isCanonicalUUID(idempotencyKey)
		else {
			throw AccountControlError.invalidInput
		}
	}

	private func executeAccountControl(
		arguments: [String],
		authority: ResetCardAuthority?,
		expected: AccountControlExpectedResult
	) async throws -> AccountControlResult {
		let processResult: ResetCardProcessResult
		do {
			processResult = try await run(arguments: arguments)
		} catch let error as ResetCardClientError {
			if error == .invalidResponse {
				throw AccountControlError.invalidResponse
			}
			throw AccountControlError.client(error)
		} catch {
			throw AccountControlError.invalidResponse
		}

		let document: AccountControlDocument
		do {
			document = try decode(
				AccountControlDocument.self,
				from: processResult,
				schema: accountCLISchema,
				command: "command"
			)
		} catch let error as ResetCardClientError {
			if error == .invalidResponse {
				throw AccountControlError.invalidResponse
			}
			throw AccountControlError.client(error)
		} catch {
			throw AccountControlError.invalidResponse
		}

		switch document.result {
		case .applied(let entityRevision, let payload):
			guard document.outcome == "applied", processResult.exitCode == 0 else {
				throw AccountControlError.invalidResponse
			}
			return try payload.result(
				entityRevision: entityRevision,
				authority: authority,
				expected: expected
			)
		case .rejected(let error):
			guard document.outcome == "rejected", processResult.exitCode == 1 else {
				throw AccountControlError.invalidResponse
			}
			throw error.error
		case .potentiallyDispatched:
			guard document.outcome == "potentially_dispatched",
				processResult.exitCode == 2
			else {
				throw AccountControlError.invalidResponse
			}
			throw AccountControlError.potentiallyDispatched
		}
	}
}

private enum AccountControlExpectedResult {
	case accountChanged(accountID: String, displayLabel: String?, enabled: Bool?)
	case accountLoggedOut(accountID: String)
	case routing(mode: AccountRoutingMode)
}

private struct AccountControlDocument: Decodable, ResetCardStableDocument {
	let schema: String
	let command: String
	let outcome: String
	let result: AccountControlWireResponse

	init(from decoder: Decoder) throws {
		try requireExactFields(
			in: decoder,
			expected: ["schema", "command", "outcome", "result"]
		)
		let container = try decoder.container(keyedBy: CodingKeys.self)
		schema = try container.decode(String.self, forKey: .schema)
		command = try container.decode(String.self, forKey: .command)
		outcome = try container.decode(String.self, forKey: .outcome)
		result = try container.decode(AccountControlWireResponse.self, forKey: .result)
	}

	private enum CodingKeys: String, CodingKey {
		case schema
		case command
		case outcome
		case result
	}
}

private enum AccountControlWireResponse: Decodable {
	case applied(entityRevision: UInt64, payload: AccountControlResultPayloadWire)
	case rejected(error: AccountControlCommandErrorWire)
	case potentiallyDispatched

	init(from decoder: Decoder) throws {
		let container = try decoder.container(keyedBy: CodingKeys.self)
		switch try container.decode(String.self, forKey: .outcome) {
		case "applied":
			try requireExactFields(in: decoder, expected: ["outcome", "data"])
			let data = try container.decode(AppliedData.self, forKey: .data)
			guard data.entityRevision > 0 else {
				throw AccountControlError.invalidResponse
			}
			self = .applied(
				entityRevision: data.entityRevision,
				payload: data.result
			)
		case "rejected":
			try requireExactFields(in: decoder, expected: ["outcome", "data"])
			let data = try container.decode(RejectedData.self, forKey: .data)
			self = .rejected(error: data.error)
		case "potentially_dispatched":
			try requireExactFields(in: decoder, expected: ["outcome", "data"])
			_ = try container.decode(PotentiallyDispatchedData.self, forKey: .data)
			self = .potentiallyDispatched
		default:
			throw AccountControlError.invalidResponse
		}
	}

	private struct AppliedData: Decodable {
		let entityRevision: UInt64
		let result: AccountControlResultPayloadWire

		init(from decoder: Decoder) throws {
			try requireExactFields(
				in: decoder,
				expected: ["entity_revision", "result"]
			)
			let container = try decoder.container(keyedBy: CodingKeys.self)
			entityRevision = try container.decode(UInt64.self, forKey: .entityRevision)
			result = try container.decode(
				AccountControlResultPayloadWire.self,
				forKey: .result
			)
		}

		private enum CodingKeys: String, CodingKey {
			case entityRevision = "entity_revision"
			case result
		}
	}

	private struct RejectedData: Decodable {
		let error: AccountControlCommandErrorWire

		init(from decoder: Decoder) throws {
			try requireExactFields(in: decoder, expected: ["error"])
			let container = try decoder.container(keyedBy: CodingKeys.self)
			error = try container.decode(AccountControlCommandErrorWire.self, forKey: .error)
		}

		private enum CodingKeys: String, CodingKey {
			case error
		}
	}

	private struct PotentiallyDispatchedData: Decodable {
		init(from decoder: Decoder) throws {
			try requireExactFields(in: decoder, expected: ["failure"])
			let container = try decoder.container(keyedBy: CodingKeys.self)
			_ = try container.decode(
				AccountControlClientFailureWire.self,
				forKey: .failure
			)
		}

		private enum CodingKeys: String, CodingKey {
			case failure
		}
	}

	private enum CodingKeys: String, CodingKey {
		case outcome
		case data
	}
}

private enum AccountControlResultPayloadWire: Decodable {
	case accountChanged(ResetCardAccountWire)
	case accountLoggedOut(accountID: String, tombstoneRevision: UInt64)
	case routingChanged(AccountRoutingWire)

	init(from decoder: Decoder) throws {
		let container = try decoder.container(keyedBy: CodingKeys.self)
		switch try container.decode(String.self, forKey: .name) {
		case "account_changed":
			try requireExactFields(in: decoder, expected: ["name", "data"])
			let data = try container.decode(AccountChangedData.self, forKey: .data)
			self = .accountChanged(data.account)
		case "account_logged_out":
			try requireExactFields(in: decoder, expected: ["name", "data"])
			let data = try container.decode(AccountLoggedOutData.self, forKey: .data)
			guard ResetCardCLIClient.isCanonicalAccountID(data.accountID),
				data.tombstoneRevision > 0
			else {
				throw AccountControlError.invalidResponse
			}
			self = .accountLoggedOut(
				accountID: data.accountID,
				tombstoneRevision: data.tombstoneRevision
			)
		case "account_routing_changed":
			try requireExactFields(in: decoder, expected: ["name", "data"])
			let data = try container.decode(AccountRoutingChangedData.self, forKey: .data)
			self = .routingChanged(data.routing)
		default:
			throw AccountControlError.invalidResponse
		}
	}

	func result(
		entityRevision: UInt64,
		authority: ResetCardAuthority?,
		expected: AccountControlExpectedResult
	) throws -> AccountControlResult {
		switch (self, expected) {
		case (
			.accountChanged(let wire),
			.accountChanged(let expectedID, let expectedLabel, let expectedEnabled)
		):
			let decoded = try wire.record()
			guard decoded.accountID == expectedID,
				decoded.accountRevision == entityRevision,
				expectedLabel.map({ $0 == decoded.displayLabel }) ?? true,
				expectedEnabled.map({ $0 == decoded.enabled }) ?? true
			else {
				throw AccountControlError.invalidResponse
			}
			return .accountChanged(
				ResetCardAccountRecord(
					authority: authority,
					accountID: decoded.accountID,
					displayLabel: decoded.displayLabel,
					accountRevision: decoded.accountRevision,
					enabled: decoded.enabled,
					observedState: decoded.observedState,
					lifecycleReadiness: decoded.lifecycleReadiness,
					credentialBinding: decoded.credentialBinding,
					unsettledOperation: decoded.unsettledOperation,
					fiveHourQuota: decoded.fiveHourQuota,
					sevenDayQuota: decoded.sevenDayQuota
				)
			)
		case (
			.accountLoggedOut(let accountID, let tombstoneRevision),
			.accountLoggedOut(let expectedID)
		):
			guard accountID == expectedID, tombstoneRevision == entityRevision else {
				throw AccountControlError.invalidResponse
			}
			return .accountLoggedOut(
				accountID: accountID,
				tombstoneRevision: tombstoneRevision
			)
		case (.routingChanged(let wire), .routing(let expectedMode)):
			let routing = wire.routing
			guard routing.revision == entityRevision, routing.mode == expectedMode else {
				throw AccountControlError.invalidResponse
			}
			return .routingChanged(routing)
		default:
			throw AccountControlError.invalidResponse
		}
	}

	private struct AccountChangedData: Decodable {
		let account: ResetCardAccountWire

		init(from decoder: Decoder) throws {
			try requireExactFields(in: decoder, expected: ["account"])
			let container = try decoder.container(keyedBy: CodingKeys.self)
			account = try container.decode(ResetCardAccountWire.self, forKey: .account)
		}

		private enum CodingKeys: String, CodingKey {
			case account
		}
	}

	private struct AccountLoggedOutData: Decodable {
		let accountID: String
		let tombstoneRevision: UInt64

		init(from decoder: Decoder) throws {
			try requireExactFields(
				in: decoder,
				expected: ["account_id", "tombstone_revision"]
			)
			let container = try decoder.container(keyedBy: CodingKeys.self)
			accountID = try container.decode(String.self, forKey: .accountID)
			tombstoneRevision = try container.decode(
				UInt64.self,
				forKey: .tombstoneRevision
			)
		}

		private enum CodingKeys: String, CodingKey {
			case accountID = "account_id"
			case tombstoneRevision = "tombstone_revision"
		}
	}

	private struct AccountRoutingChangedData: Decodable {
		let routing: AccountRoutingWire

		init(from decoder: Decoder) throws {
			try requireExactFields(in: decoder, expected: ["routing"])
			let container = try decoder.container(keyedBy: CodingKeys.self)
			routing = try container.decode(AccountRoutingWire.self, forKey: .routing)
		}

		private enum CodingKeys: String, CodingKey {
			case routing
		}
	}

	private enum CodingKeys: String, CodingKey {
		case name
		case data
	}
}

private enum AccountControlCommandErrorWire: Decodable {
	case expectedRevisionMismatch(expected: UInt64, actual: UInt64)
	case idempotencyConflict
	case idempotencyCapacityExceeded
	case applicationUnavailable
	case acceptanceUnknown
	case accountRejected(AccountControlRejection, actualRevision: UInt64?)

	init(from decoder: Decoder) throws {
		let container = try decoder.container(keyedBy: CodingKeys.self)
		switch try container.decode(String.self, forKey: .reason) {
		case "expected_revision_mismatch":
			try requireExactFields(
				in: decoder,
				expected: ["reason", "expected", "actual"]
			)
			let expected = try container.decode(UInt64.self, forKey: .expected)
			let actual = try container.decode(UInt64.self, forKey: .actual)
			guard expected > 0, actual > 0 else {
				throw AccountControlError.invalidResponse
			}
			self = .expectedRevisionMismatch(expected: expected, actual: actual)
		case "idempotency_conflict":
			try requireExactFields(in: decoder, expected: ["reason"])
			self = .idempotencyConflict
		case "idempotency_capacity_exceeded":
			try requireExactFields(in: decoder, expected: ["reason", "capacity"])
			let capacity = try container.decode(UInt64.self, forKey: .capacity)
			guard capacity > 0 else {
				throw AccountControlError.invalidResponse
			}
			self = .idempotencyCapacityExceeded
		case "application_unavailable":
			try requireExactFields(in: decoder, expected: ["reason", "message"])
			let message = try container.decode(String.self, forKey: .message)
			guard isBoundedWireText(message, maximumBytes: 512) else {
				throw AccountControlError.invalidResponse
			}
			self = .applicationUnavailable
		case "acceptance_unknown":
			try requireExactFields(in: decoder, expected: ["reason"])
			self = .acceptanceUnknown
		case "account_command_rejected":
			try rejectUnknownFields(
				in: decoder,
				allowed: ["reason", "rejection", "actual_revision"]
			)
			let rejection = try container.decode(
				AccountControlRejection.self,
				forKey: .rejection
			)
			let actual = try container.decodeIfPresent(
				UInt64.self,
				forKey: .actualRevision
			)
			guard actual.map({ $0 > 0 }) ?? true else {
				throw AccountControlError.invalidResponse
			}
			self = .accountRejected(rejection, actualRevision: actual)
		default:
			throw AccountControlError.invalidResponse
		}
	}

	var error: AccountControlError {
		switch self {
		case .expectedRevisionMismatch(let expected, let actual):
			return .expectedRevisionMismatch(expected: expected, actual: actual)
		case .idempotencyConflict:
			return .idempotencyConflict
		case .idempotencyCapacityExceeded:
			return .idempotencyCapacityExceeded
		case .applicationUnavailable:
			return .applicationUnavailable
		case .acceptanceUnknown:
			return .acceptanceUnknown
		case .accountRejected(let rejection, let actualRevision):
			return .rejected(rejection, actualRevision: actualRevision)
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

private enum AccountControlClientFailureWire: String, Decodable {
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
