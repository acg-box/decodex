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
	let routing: AccountRoutingControl?
}

enum CodexAuthProjection: Equatable, Sendable {
	case current(accountID: String, accountRevision: UInt64, projectionDigest: String)
	case unmanaged
	case unavailable
}

enum AccountControlResult: Equatable, Sendable {
	case accountChanged(ResetCardAccountRecord)
	case accountLoggedOut(accountID: String, tombstoneRevision: UInt64)
	case routingChanged(AccountRoutingControl)
	case codexAuthProjected(accountID: String, accountRevision: UInt64, projectionDigest: String)
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
			return "The Decodex service returned an invalid account response."
		case .client(let error):
			switch error {
			case .nativeClientUnavailable:
				return "The native Decodex client is unavailable."
			case .timedOut:
				return "The account request timed out."
			case .transportDisconnected:
				return "The Decodex daemon is not reachable."
			case .transportBackpressured:
				return "The Decodex account service is busy."
			case .outputTooLarge:
				return "The Decodex service returned too much account data."
			case .commandRejected, .useDefinitelyNotDispatched,
				.usePotentiallyDispatched, .invalidResponse,
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

enum AccountReauthenticationState: String, Decodable, Equatable, Sendable {
	case openingBrowser = "opening_browser"
	case waitingForBrowser = "waiting_for_browser"
	case installing
	case completed
	case failed
	case cancelled
}

enum AccountReauthenticationFailure: String, Decodable, Equatable, Sendable {
	case codexUnavailable = "codex_unavailable"
	case loginFailed = "login_failed"
	case loginTimedOut = "login_timed_out"
	case accountMismatch = "account_mismatch"
	case accountChanged = "account_changed"
	case accountUnavailable = "account_unavailable"
	case recoveryChanged = "recovery_changed"
	case credentialStoreUnavailable = "credential_store_unavailable"
	case serviceUnavailable = "service_unavailable"
	case outcomeUnknown = "outcome_unknown"
	case sessionNotFound = "session_not_found"
	case busy

	var presentation: String {
		switch self {
		case .codexUnavailable:
			return "The Codex login tool is unavailable."
		case .loginFailed:
			return "Codex could not complete the login."
		case .loginTimedOut:
			return "The browser login timed out. Try again."
		case .accountMismatch:
			return "This login belongs to a different account."
		case .accountChanged:
			return "The account changed. Close this login and try again."
		case .accountUnavailable:
			return "This account is not available for login."
		case .recoveryChanged:
			return "This login recovery changed. Refresh the account and try again."
		case .credentialStoreUnavailable:
			return "The credential store is unavailable."
		case .serviceUnavailable:
			return "The Decodex account service is unavailable."
		case .outcomeUnknown:
			return "The login may have completed. Refresh the account before trying again."
		case .sessionNotFound:
			return "This login session is no longer available."
		case .busy:
			return "Another account login is already in progress."
		}
	}
}

struct AccountReauthenticationStatus: Equatable, Sendable {
	let sessionID: String
	let state: AccountReauthenticationState
	let failure: AccountReauthenticationFailure?
}

protocol AccountControlClient: ResetCardClient {
	func accountSnapshot(
		authority: ResetCardAuthority?
	) async throws -> AccountControlSnapshot

	func enrollFromSharedCodex(
		authority: ResetCardAuthority?,
		operationID: String,
		accountID: String,
		enabled: Bool,
		idempotencyKey: String
	) async throws -> AccountControlResult

	func codexAuthProjection(
		authority: ResetCardAuthority?
	) async throws -> CodexAuthProjection

	func useAccountInCodex(
		authority: ResetCardAuthority?,
		accountID: String,
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

	func setAccountOrder(
		authority: ResetCardAuthority?,
		order: [String],
		expectedRoutingRevision: UInt64,
		idempotencyKey: String
	) async throws -> AccountControlResult

	func startAccountReauthentication(
		authority: ResetCardAuthority?,
		sessionID: String,
		operationID: String,
		accountID: String,
		expectedRevision: UInt64,
		recoveryOperationID: String?,
		idempotencyKey: String,
		codexBin: String
	) async throws -> AccountReauthenticationStatus

	func pollAccountReauthentication(
		authority: ResetCardAuthority?,
		sessionID: String
	) async throws -> AccountReauthenticationStatus

	func cancelAccountReauthentication(
		authority: ResetCardAuthority?,
		sessionID: String
	) async throws -> AccountReauthenticationStatus
}

extension AccountControlClient {
	func startAccountReauthentication(
		authority _: ResetCardAuthority?,
		sessionID _: String,
		operationID _: String,
		accountID _: String,
		expectedRevision _: UInt64,
		recoveryOperationID _: String?,
		idempotencyKey _: String,
		codexBin _: String
	) async throws -> AccountReauthenticationStatus {
		throw AccountControlError.applicationUnavailable
	}

	func pollAccountReauthentication(
		authority _: ResetCardAuthority?,
		sessionID _: String
	) async throws -> AccountReauthenticationStatus {
		throw AccountControlError.applicationUnavailable
	}

	func cancelAccountReauthentication(
		authority _: ResetCardAuthority?,
		sessionID _: String
	) async throws -> AccountReauthenticationStatus {
		throw AccountControlError.applicationUnavailable
	}
}

extension DecodexNativeClient: AccountControlClient {
	func enrollFromSharedCodex(
		authority: ResetCardAuthority?,
		operationID: String,
		accountID: String,
		enabled: Bool,
		idempotencyKey: String
	) async throws -> AccountControlResult {
		try Self.validateAccountControlInput(
			authority: authority,
			accountID: accountID,
			operationID: operationID,
			expectedRevision: nil,
			idempotencyKey: idempotencyKey
		)
		return try await executeAccountControl(
			request: DecodexNativeRequest(
				operation: "enroll_account",
				accountID: accountID,
				idempotencyKey: idempotencyKey,
				operationID: operationID,
				enabled: enabled
			),
			authority: authority,
			expected: .accountChanged(
				accountID: accountID,
				enabled: enabled
			)
		)
	}

	func codexAuthProjection(
		authority: ResetCardAuthority?
	) async throws -> CodexAuthProjection {
		guard authority.map(Self.isValidAuthority) ?? true else {
			throw AccountControlError.invalidInput
		}
		let response: (
			authority: ResetCardAuthority,
			data: CodexAuthProjectionWire
		)
		do {
			response = try await perform(
				DecodexNativeRequest(operation: "get_codex_auth_projection"),
				authority: authority
			)
		} catch let error as ResetCardClientError {
			if error == .invalidResponse {
				throw AccountControlError.invalidResponse
			}
			throw AccountControlError.client(error)
		} catch {
			throw AccountControlError.invalidResponse
		}
		return response.data.projection
	}

	func useAccountInCodex(
		authority: ResetCardAuthority?,
		accountID: String,
		expectedRevision: UInt64,
		idempotencyKey: String
	) async throws -> AccountControlResult {
		try Self.validateAccountControlInput(
			authority: authority,
			accountID: accountID,
			operationID: nil,
			expectedRevision: expectedRevision,
			idempotencyKey: idempotencyKey
		)
		return try await executeAccountControl(
			request: DecodexNativeRequest(
				operation: "use_account_in_codex",
				accountID: accountID,
				expectedRevision: expectedRevision,
				idempotencyKey: idempotencyKey
			),
			authority: authority,
			expected: .codexAuthProjected(
				accountID: accountID,
				accountRevision: expectedRevision
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
		try Self.validateAccountControlInput(
			authority: authority,
			accountID: accountID,
			operationID: nil,
			expectedRevision: expectedRevision,
			idempotencyKey: idempotencyKey
		)
		return try await executeAccountControl(
			request: DecodexNativeRequest(
				operation: enabled ? "enable_account" : "disable_account",
				accountID: accountID,
				expectedRevision: expectedRevision,
				idempotencyKey: idempotencyKey
			),
			authority: authority,
			expected: .accountChanged(
				accountID: accountID,
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
		try Self.validateAccountControlInput(
			authority: authority,
			accountID: accountID,
			operationID: operationID,
			expectedRevision: expectedRevision,
			idempotencyKey: idempotencyKey
		)
		return try await executeAccountControl(
			request: DecodexNativeRequest(
				operation: "logout_account",
				accountID: accountID,
				expectedRevision: expectedRevision,
				idempotencyKey: idempotencyKey,
				operationID: operationID
			),
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
		try Self.validateAccountControlInput(
			authority: authority,
			accountID: accountID,
			operationID: nil,
			expectedRevision: expectedAccountRevision,
			idempotencyKey: idempotencyKey
		)
		guard expectedRoutingRevision > 0 else {
			throw AccountControlError.invalidInput
		}
		return try await executeAccountControl(
			request: DecodexNativeRequest(
				operation: "set_fixed_selection",
				accountID: accountID,
				expectedAccountRevision: expectedAccountRevision,
				expectedRoutingRevision: expectedRoutingRevision,
				idempotencyKey: idempotencyKey
			),
			authority: authority,
			expected: .routing(mode: .fixed(accountID: accountID))
		)
	}

	func setBalancedSelection(
		authority: ResetCardAuthority?,
		expectedRoutingRevision: UInt64,
		idempotencyKey: String
	) async throws -> AccountControlResult {
		guard authority.map(Self.isValidAuthority) ?? true,
			expectedRoutingRevision > 0,
			Self.isCanonicalUUID(idempotencyKey)
		else {
			throw AccountControlError.invalidInput
		}
		return try await executeAccountControl(
			request: DecodexNativeRequest(
				operation: "set_balanced_selection",
				expectedRoutingRevision: expectedRoutingRevision,
				idempotencyKey: idempotencyKey
			),
			authority: authority,
			expected: .routing(mode: .balanced)
		)
	}

	func setAccountOrder(
		authority: ResetCardAuthority?,
		order: [String],
		expectedRoutingRevision: UInt64,
		idempotencyKey: String
	) async throws -> AccountControlResult {
		guard authority.map(Self.isValidAuthority) ?? true,
			expectedRoutingRevision > 0,
			order.count <= 512,
			order.allSatisfy(Self.isCanonicalAccountID),
			Set(order).count == order.count,
			Self.isCanonicalUUID(idempotencyKey)
		else {
			throw AccountControlError.invalidInput
		}
		return try await executeAccountControl(
			request: DecodexNativeRequest(
				operation: "set_account_order",
				expectedRoutingRevision: expectedRoutingRevision,
				order: order,
				idempotencyKey: idempotencyKey
			),
			authority: authority,
			expected: .routingOrder(order)
		)
	}

	func startAccountReauthentication(
		authority: ResetCardAuthority?,
		sessionID: String,
		operationID: String,
		accountID: String,
		expectedRevision: UInt64,
		recoveryOperationID: String?,
		idempotencyKey: String,
		codexBin: String
	) async throws -> AccountReauthenticationStatus {
		try Self.validateAccountControlInput(
			authority: authority,
			accountID: accountID,
			operationID: operationID,
			expectedRevision: expectedRevision,
			idempotencyKey: idempotencyKey
		)
		guard Self.isCanonicalUUID(sessionID),
			recoveryOperationID.map(Self.isCanonicalUUID) ?? true,
			recoveryOperationID != operationID,
			Self.isValidCodexExecutablePath(codexBin)
		else {
			throw AccountControlError.invalidInput
		}
		return try await executeAccountReauthentication(
			request: DecodexNativeRequest(
				operation: "start_account_reauthentication",
				accountID: accountID,
				expectedRevision: expectedRevision,
				idempotencyKey: idempotencyKey,
				operationID: operationID,
				recoveryOperationID: recoveryOperationID,
				sessionID: sessionID,
				codexBin: codexBin
			),
			authority: authority,
			expectedSessionID: sessionID
		)
	}

	func pollAccountReauthentication(
		authority: ResetCardAuthority?,
		sessionID: String
	) async throws -> AccountReauthenticationStatus {
		try await accountReauthenticationSessionRequest(
			operation: "poll_account_reauthentication",
			authority: authority,
			sessionID: sessionID
		)
	}

	func cancelAccountReauthentication(
		authority: ResetCardAuthority?,
		sessionID: String
	) async throws -> AccountReauthenticationStatus {
		try await accountReauthenticationSessionRequest(
			operation: "cancel_account_reauthentication",
			authority: authority,
			sessionID: sessionID
		)
	}

	private static func validateAccountControlInput(
		authority: ResetCardAuthority?,
		accountID: String,
		operationID: String?,
		expectedRevision: UInt64?,
		idempotencyKey: String
	) throws {
		guard authority.map(isValidAuthority) ?? true,
			isCanonicalAccountID(accountID),
			operationID.map(isCanonicalUUID) ?? true,
			expectedRevision.map({ $0 > 0 }) ?? true,
			isCanonicalUUID(idempotencyKey)
		else {
			throw AccountControlError.invalidInput
		}
	}

	private static func isValidCodexExecutablePath(_ value: String) -> Bool {
		guard value.utf8.count <= 4_096,
			value.utf8.contains(0) == false
		else {
			return false
		}
		let url = URL(fileURLWithPath: value)
		return url.path.hasPrefix("/")
			&& url.standardizedFileURL.path == value
	}

	private func accountReauthenticationSessionRequest(
		operation: String,
		authority: ResetCardAuthority?,
		sessionID: String
	) async throws -> AccountReauthenticationStatus {
		guard authority.map(Self.isValidAuthority) ?? true,
			Self.isCanonicalUUID(sessionID)
		else {
			throw AccountControlError.invalidInput
		}
		return try await executeAccountReauthentication(
			request: DecodexNativeRequest(
				operation: operation,
				sessionID: sessionID
			),
			authority: authority,
			expectedSessionID: sessionID
		)
	}

	private func executeAccountReauthentication(
		request: DecodexNativeRequest,
		authority: ResetCardAuthority?,
		expectedSessionID: String
	) async throws -> AccountReauthenticationStatus {
		let response: (
			authority: ResetCardAuthority,
			data: AccountReauthenticationWire
		)
		do {
			response = try await perform(request, authority: authority)
		} catch let error as ResetCardClientError {
			if error == .invalidResponse {
				throw AccountControlError.invalidResponse
			}
			throw AccountControlError.client(error)
		} catch {
			throw AccountControlError.invalidResponse
		}
		guard response.data.value.sessionID == expectedSessionID else {
			throw AccountControlError.invalidResponse
		}
		return response.data.value
	}

	private func executeAccountControl(
		request: DecodexNativeRequest,
		authority: ResetCardAuthority?,
		expected: AccountControlExpectedResult
	) async throws -> AccountControlResult {
		let response: (
			authority: ResetCardAuthority,
			data: AccountControlWireResponse
		)
		do {
			response = try await perform(request, authority: authority)
		} catch let error as ResetCardClientError {
			if error == .invalidResponse {
				throw AccountControlError.invalidResponse
			}
			throw AccountControlError.client(error)
		} catch {
			throw AccountControlError.invalidResponse
		}

		switch response.data {
		case .applied(let entityRevision, let payload):
			return try payload.result(
				entityRevision: entityRevision,
				authority: response.authority,
				expected: expected
			)
		case .rejected(let error):
			throw error.error
		case .potentiallyDispatched:
			throw AccountControlError.potentiallyDispatched
		}
	}
}

private struct AccountReauthenticationWire: Decodable {
	let value: AccountReauthenticationStatus

	init(from decoder: Decoder) throws {
		try rejectUnknownFields(
			in: decoder,
			allowed: ["session_id", "state", "failure"]
		)
		let container = try decoder.container(keyedBy: CodingKeys.self)
		let sessionID = try container.decode(String.self, forKey: .sessionID)
		let state = try container.decode(
			AccountReauthenticationState.self,
			forKey: .state
		)
		let failure = try container.decodeIfPresent(
			AccountReauthenticationFailure.self,
			forKey: .failure
		)
		guard DecodexNativeClient.isCanonicalUUID(sessionID),
			Self.hasValidShape(state: state, failure: failure)
		else {
			throw AccountControlError.invalidResponse
		}
		value = AccountReauthenticationStatus(
			sessionID: sessionID,
			state: state,
			failure: failure
		)
	}

	private static func hasValidShape(
		state: AccountReauthenticationState,
		failure: AccountReauthenticationFailure?
	) -> Bool {
		switch state {
		case .openingBrowser, .waitingForBrowser, .installing,
			.completed, .cancelled:
			return failure == nil
		case .failed:
			return failure != nil
		}
	}

	private enum CodingKeys: String, CodingKey {
		case sessionID = "session_id"
		case state
		case failure
	}
}
private enum AccountControlExpectedResult {
	case accountChanged(accountID: String, enabled: Bool?)
	case accountLoggedOut(accountID: String)
	case routing(mode: AccountRoutingMode)
	case routingOrder([String])
	case codexAuthProjected(accountID: String, accountRevision: UInt64)
}

private enum CodexAuthProjectionWire: Decodable {
	case current(accountID: String, accountRevision: UInt64, projectionDigest: String)
	case unmanaged
	case unavailable

	init(from decoder: Decoder) throws {
		let container = try decoder.container(keyedBy: CodingKeys.self)
		switch try container.decode(String.self, forKey: .outcome) {
		case "current":
			try requireExactFields(in: decoder, expected: ["outcome", "data"])
			let data = try container.decode(CurrentData.self, forKey: .data)
			guard DecodexNativeClient.isCanonicalAccountID(data.accountID),
				data.accountRevision > 0,
				Self.isSHA256(data.projectionDigest)
			else {
				throw AccountControlError.invalidResponse
			}
			self = .current(
				accountID: data.accountID,
				accountRevision: data.accountRevision,
				projectionDigest: data.projectionDigest
			)
		case "unmanaged":
			try requireExactFields(in: decoder, expected: ["outcome"])
			self = .unmanaged
		case "unavailable":
			try requireExactFields(in: decoder, expected: ["outcome"])
			self = .unavailable
		default:
			throw AccountControlError.invalidResponse
		}
	}

	var projection: CodexAuthProjection {
		switch self {
		case .current(let accountID, let accountRevision, let projectionDigest):
			return .current(
				accountID: accountID,
				accountRevision: accountRevision,
				projectionDigest: projectionDigest
			)
		case .unmanaged:
			return .unmanaged
		case .unavailable:
			return .unavailable
		}
	}

	static func isSHA256(_ value: String) -> Bool {
		value.utf8.count == 64
			&& value.utf8.allSatisfy {
				(48...57).contains($0) || (97...102).contains($0)
			}
	}

	private struct CurrentData: Decodable {
		let accountID: String
		let accountRevision: UInt64
		let projectionDigest: String

		init(from decoder: Decoder) throws {
			try requireExactFields(
				in: decoder,
				expected: ["account_id", "account_revision", "projection_digest"]
			)
			let container = try decoder.container(keyedBy: CodingKeys.self)
			accountID = try container.decode(String.self, forKey: .accountID)
			accountRevision = try container.decode(UInt64.self, forKey: .accountRevision)
			projectionDigest = try container.decode(String.self, forKey: .projectionDigest)
		}

		private enum CodingKeys: String, CodingKey {
			case accountID = "account_id"
			case accountRevision = "account_revision"
			case projectionDigest = "projection_digest"
		}
	}

	private enum CodingKeys: String, CodingKey {
		case outcome
		case data
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
	case codexAuthProjected(accountID: String, accountRevision: UInt64, projectionDigest: String)

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
			guard DecodexNativeClient.isCanonicalAccountID(data.accountID),
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
		case "codex_auth_projected":
			try requireExactFields(in: decoder, expected: ["name", "data"])
			let data = try container.decode(CodexAuthProjectedData.self, forKey: .data)
			guard DecodexNativeClient.isCanonicalAccountID(data.accountID),
				data.accountRevision > 0,
				CodexAuthProjectionWire.isSHA256(data.projectionDigest)
			else {
				throw AccountControlError.invalidResponse
			}
			self = .codexAuthProjected(
				accountID: data.accountID,
				accountRevision: data.accountRevision,
				projectionDigest: data.projectionDigest
			)
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
			.accountChanged(let expectedID, let expectedEnabled)
		):
			let decoded = try wire.record()
			guard decoded.accountID == expectedID,
				decoded.accountRevision == entityRevision,
				expectedEnabled.map({ $0 == decoded.enabled }) ?? true
			else {
				throw AccountControlError.invalidResponse
			}
			return .accountChanged(
				ResetCardAccountRecord(
					authority: authority,
					accountID: decoded.accountID,
					alias: decoded.alias,
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
		case (.routingChanged(let wire), .routingOrder(let expectedOrder)):
			let routing = wire.routing
			guard routing.revision == entityRevision, routing.order == expectedOrder else {
				throw AccountControlError.invalidResponse
			}
			return .routingChanged(routing)
		case (
			.codexAuthProjected(let accountID, let accountRevision, let projectionDigest),
			.codexAuthProjected(let expectedID, let expectedRevision)
		):
			guard accountID == expectedID,
				accountRevision == expectedRevision,
				entityRevision == expectedRevision
			else {
				throw AccountControlError.invalidResponse
			}
			return .codexAuthProjected(
				accountID: accountID,
				accountRevision: accountRevision,
				projectionDigest: projectionDigest
			)
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

	private struct CodexAuthProjectedData: Decodable {
		let accountID: String
		let accountRevision: UInt64
		let projectionDigest: String

		init(from decoder: Decoder) throws {
			try requireExactFields(
				in: decoder,
				expected: ["account_id", "account_revision", "projection_digest"]
			)
			let container = try decoder.container(keyedBy: CodingKeys.self)
			accountID = try container.decode(String.self, forKey: .accountID)
			accountRevision = try container.decode(UInt64.self, forKey: .accountRevision)
			projectionDigest = try container.decode(String.self, forKey: .projectionDigest)
		}

		private enum CodingKeys: String, CodingKey {
			case accountID = "account_id"
			case accountRevision = "account_revision"
			case projectionDigest = "projection_digest"
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
