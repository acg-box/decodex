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
	let pendingRoute: AccountRoutePending?
}

struct AccountRoutePending: Decodable, Equatable, Sendable {
	let operationID: String
	let accountID: String
	let routingRevision: UInt64
	let waitReason: AccountRouteWaitReason

	init(
		operationID: String,
		accountID: String,
		routingRevision: UInt64,
		waitReason: AccountRouteWaitReason
	) {
		self.operationID = operationID
		self.accountID = accountID
		self.routingRevision = routingRevision
		self.waitReason = waitReason
	}

	init(from decoder: Decoder) throws {
		try requireExactFields(
			in: decoder,
			expected: ["operation_id", "account_id", "routing_revision", "wait_reason"]
		)
		let container = try decoder.container(keyedBy: CodingKeys.self)
		operationID = try container.decode(String.self, forKey: .operationID)
		accountID = try container.decode(String.self, forKey: .accountID)
		routingRevision = try container.decode(UInt64.self, forKey: .routingRevision)
		waitReason = try container.decode(AccountRouteWaitReason.self, forKey: .waitReason)
		guard DecodexNativeClient.isCanonicalAccountID(operationID),
			DecodexNativeClient.isCanonicalAccountID(accountID),
			routingRevision > 0
		else {
			throw AccountControlError.invalidResponse
		}
	}

	private enum CodingKeys: String, CodingKey {
		case operationID = "operation_id"
		case accountID = "account_id"
		case routingRevision = "routing_revision"
		case waitReason = "wait_reason"
	}
}

enum AccountRouteBlockingProcess: String, Decodable, Equatable, Sendable {
	case chatgpt
	case codex
}

enum AccountRouteAuthHome: String, Decodable, Equatable, Sendable {
	case shared
	case unknown
}

struct AccountRouteProcessBlocker: Decodable, Equatable, Sendable {
	let pid: UInt32
	let process: AccountRouteBlockingProcess
	let authHome: AccountRouteAuthHome

	init(
		pid: UInt32,
		process: AccountRouteBlockingProcess,
		authHome: AccountRouteAuthHome
	) {
		self.pid = pid
		self.process = process
		self.authHome = authHome
	}

	init(from decoder: Decoder) throws {
		try requireExactFields(in: decoder, expected: ["pid", "process", "auth_home"])
		let container = try decoder.container(keyedBy: CodingKeys.self)
		pid = try container.decode(UInt32.self, forKey: .pid)
		process = try container.decode(AccountRouteBlockingProcess.self, forKey: .process)
		authHome = try container.decode(AccountRouteAuthHome.self, forKey: .authHome)
		guard pid > 0 else {
			throw AccountControlError.invalidResponse
		}
	}

	private enum CodingKeys: String, CodingKey {
		case pid
		case process
		case authHome = "auth_home"
	}
}

enum AccountRouteWaitReason: Decodable, Equatable, Sendable {
	case externalCodex(blockers: [AccountRouteProcessBlocker], omitted: UInt16)
	case codexObservationUnavailable
	case accountReadiness(ResetCardLifecycleReadiness)
	case sharedAuthStabilizing
	case sharedAuthUnavailable
	case projectionReadback

	init(from decoder: Decoder) throws {
		let container = try decoder.container(keyedBy: CodingKeys.self)
		switch try container.decode(String.self, forKey: .reason) {
		case "external_codex":
			try requireExactFields(in: decoder, expected: ["reason", "data"])
			let data = try container.decode(ExternalCodexData.self, forKey: .data)
			guard data.blockers.isEmpty == false,
				data.blockers.count <= 8,
				data.omitted == 0 || data.blockers.count == 8,
				Set(data.blockers.map(\.pid)).count == data.blockers.count
			else {
				throw AccountControlError.invalidResponse
			}
			self = .externalCodex(blockers: data.blockers, omitted: data.omitted)
		case "codex_observation_unavailable":
			try requireExactFields(in: decoder, expected: ["reason"])
			self = .codexObservationUnavailable
		case "account_readiness":
			try requireExactFields(in: decoder, expected: ["reason", "data"])
			let data = try container.decode(AccountReadinessData.self, forKey: .data)
			guard [
				ResetCardLifecycleReadiness.storeUnavailable,
				.storeMismatch,
				.operationUnsettled,
				.callbackCapabilityUnready,
			].contains(data.readiness) else {
				throw AccountControlError.invalidResponse
			}
			self = .accountReadiness(data.readiness)
		case "shared_auth_stabilizing":
			try requireExactFields(in: decoder, expected: ["reason"])
			self = .sharedAuthStabilizing
		case "shared_auth_unavailable":
			try requireExactFields(in: decoder, expected: ["reason"])
			self = .sharedAuthUnavailable
		case "projection_readback":
			try requireExactFields(in: decoder, expected: ["reason"])
			self = .projectionReadback
		default:
			throw AccountControlError.invalidResponse
		}
	}

	private struct ExternalCodexData: Decodable {
		let blockers: [AccountRouteProcessBlocker]
		let omitted: UInt16

		init(from decoder: Decoder) throws {
			try requireExactFields(in: decoder, expected: ["blockers", "omitted"])
			let container = try decoder.container(keyedBy: CodingKeys.self)
			blockers = try container.decode([AccountRouteProcessBlocker].self, forKey: .blockers)
			omitted = try container.decode(UInt16.self, forKey: .omitted)
		}

		private enum CodingKeys: String, CodingKey {
			case blockers
			case omitted
		}
	}

	private struct AccountReadinessData: Decodable {
		let readiness: ResetCardLifecycleReadiness

		init(from decoder: Decoder) throws {
			try requireExactFields(in: decoder, expected: ["readiness"])
			let container = try decoder.container(keyedBy: CodingKeys.self)
			readiness = try container.decode(ResetCardLifecycleReadiness.self, forKey: .readiness)
		}

		private enum CodingKeys: String, CodingKey {
			case readiness
		}
	}

	private enum CodingKeys: String, CodingKey {
		case reason
		case data
	}
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
	case routed(
		account: ResetCardAccountRecord,
		routing: AccountRoutingControl,
		projectionDigest: String
	)
	case routePending(AccountRoutePending)
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
	case providerAlreadyEnrolled = "provider_already_enrolled"
	case providerMismatch = "provider_mismatch"
	case lifecycleUnready = "lifecycle_unready"
	case sharedAuthOwnerBusy = "shared_auth_owner_busy"
	case routeSuperseded = "route_superseded"
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
		case .providerAlreadyEnrolled:
			return "This Codex login is already added. Choose a different account on the login page, then try again."
		case .providerMismatch:
			return "The provider account does not match."
		case .lifecycleUnready:
			return "The account is not ready for this action."
		case .sharedAuthOwnerBusy:
			return "Codex is still using this login. Close Codex and try again."
		case .routeSuperseded:
			return "A newer account choice replaced this pending route."
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
			case .nativeClientUnavailable, .timedOut, .transportDisconnected:
				return "Restart Decodex."
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
	case requestingCode = "requesting_code"
	case openingBrowser = "opening_browser"
	case waitingForBrowser = "waiting_for_browser"
	case installing
	case completed
	case failed
	case cancelled
}

enum AccountLoginMethod: String, Encodable, Equatable, Sendable {
	case browserRedirect = "browser_redirect"
	case deviceCode = "device_code"
}

enum AccountReauthenticationFailure: String, Decodable, Equatable, Sendable {
	case loginFailed = "login_failed"
	case loginTimedOut = "login_timed_out"
	case deviceAuthorizationRejected = "device_authorization_rejected"
	case accountMismatch = "account_mismatch"
	case accountChanged = "account_changed"
	case accountUnavailable = "account_unavailable"
	case recoveryChanged = "recovery_changed"
	case providerAlreadyEnrolled = "provider_already_enrolled"
	case credentialStoreUnavailable = "credential_store_unavailable"
	case serviceUnavailable = "service_unavailable"
	case outcomeUnknown = "outcome_unknown"
	case sessionNotFound = "session_not_found"
	case busy

	var presentation: String {
		switch self {
		case .loginFailed:
			return "Codex could not complete the login."
		case .loginTimedOut:
			return "The login timed out. Try again."
		case .deviceAuthorizationRejected:
			return "Device-code approval failed. Check ChatGPT Security, then try again."
		case .accountMismatch:
			return "This login belongs to a different account."
		case .accountChanged:
			return "The account changed. Close this login and try again."
		case .accountUnavailable:
			return "This account is not available for login."
		case .recoveryChanged:
			return "This login recovery changed. Refresh the account and try again."
		case .providerAlreadyEnrolled:
			return "This Codex login is already added. Choose a different account on the login page, then try again."
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

struct AccountReauthenticationPrompt: Equatable, Sendable {
	static let verificationURL = URL(string: "https://auth.openai.com/codex/device")!

	let verificationURL: URL
	let userCode: String
}

struct AccountReauthenticationStatus: Equatable, Sendable {
	let sessionID: String
	let state: AccountReauthenticationState
	let prompt: AccountReauthenticationPrompt?
	let authorizationURL: URL?
	let failure: AccountReauthenticationFailure?
	let resolvedAccountID: String?

	init(
		sessionID: String,
		state: AccountReauthenticationState,
		prompt: AccountReauthenticationPrompt?,
		authorizationURL: URL?,
		failure: AccountReauthenticationFailure?,
		resolvedAccountID: String? = nil
	) {
		self.sessionID = sessionID
		self.state = state
		self.prompt = prompt
		self.authorizationURL = authorizationURL
		self.failure = failure
		self.resolvedAccountID = resolvedAccountID
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
		enabled: Bool,
		idempotencyKey: String
	) async throws -> AccountControlResult

	func codexAuthProjection(
		authority: ResetCardAuthority?
	) async throws -> CodexAuthProjection

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

	func routeAccount(
		authority: ResetCardAuthority?,
		operationID: String,
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
		loginMethod: AccountLoginMethod
	) async throws -> AccountReauthenticationStatus

	func startAccountEnrollment(
		authority: ResetCardAuthority?,
		sessionID: String,
		operationID: String,
		accountID: String,
		enabled: Bool,
		idempotencyKey: String,
		loginMethod: AccountLoginMethod
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
	func routeAccount(
		authority _: ResetCardAuthority?,
		operationID _: String,
		accountID _: String,
		expectedAccountRevision _: UInt64,
		expectedRoutingRevision _: UInt64,
		idempotencyKey _: String
	) async throws -> AccountControlResult {
		throw AccountControlError.applicationUnavailable
	}

	func startAccountEnrollment(
		authority _: ResetCardAuthority?,
		sessionID _: String,
		operationID _: String,
		accountID _: String,
		enabled _: Bool,
		idempotencyKey _: String,
		loginMethod _: AccountLoginMethod
	) async throws -> AccountReauthenticationStatus {
		throw AccountControlError.applicationUnavailable
	}

	func startAccountReauthentication(
		authority _: ResetCardAuthority?,
		sessionID _: String,
		operationID _: String,
		accountID _: String,
		expectedRevision _: UInt64,
		recoveryOperationID _: String?,
		idempotencyKey _: String,
		loginMethod _: AccountLoginMethod
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
			expected: .accountEnrollment(
				requestedAccountID: accountID,
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

	func routeAccount(
		authority: ResetCardAuthority?,
		operationID: String,
		accountID: String,
		expectedAccountRevision: UInt64,
		expectedRoutingRevision: UInt64,
		idempotencyKey: String
	) async throws -> AccountControlResult {
		try Self.validateAccountControlInput(
			authority: authority,
			accountID: accountID,
			operationID: operationID,
			expectedRevision: expectedAccountRevision,
			idempotencyKey: idempotencyKey
		)
		guard expectedRoutingRevision > 0 else {
			throw AccountControlError.invalidInput
		}
		return try await executeAccountControl(
			request: DecodexNativeRequest(
				operation: "route_account",
				accountID: accountID,
				expectedAccountRevision: expectedAccountRevision,
				expectedRoutingRevision: expectedRoutingRevision,
				idempotencyKey: idempotencyKey,
				operationID: operationID
			),
			authority: authority,
			expected: .routed(accountID: accountID)
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
		loginMethod: AccountLoginMethod
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
			recoveryOperationID != operationID
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
				loginMethod: loginMethod
			),
			authority: authority,
			expectedSessionID: sessionID
		)
	}

	func startAccountEnrollment(
		authority: ResetCardAuthority?,
		sessionID: String,
		operationID: String,
		accountID: String,
		enabled: Bool,
		idempotencyKey: String,
		loginMethod: AccountLoginMethod
	) async throws -> AccountReauthenticationStatus {
		try Self.validateAccountControlInput(
			authority: authority,
			accountID: accountID,
			operationID: operationID,
			expectedRevision: nil,
			idempotencyKey: idempotencyKey
		)
		guard Self.isCanonicalUUID(sessionID) else {
			throw AccountControlError.invalidInput
		}
		return try await executeAccountReauthentication(
			request: DecodexNativeRequest(
				operation: "start_account_enrollment",
				accountID: accountID,
				idempotencyKey: idempotencyKey,
				operationID: operationID,
				sessionID: sessionID,
				loginMethod: loginMethod,
				enabled: enabled
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
			allowed: [
				"session_id", "state", "prompt", "authorization_url", "failure",
				"resolved_account_id",
			]
		)
		let container = try decoder.container(keyedBy: CodingKeys.self)
		let sessionID = try container.decode(String.self, forKey: .sessionID)
		let state = try container.decode(
			AccountReauthenticationState.self,
			forKey: .state
		)
		let prompt = try container.decodeIfPresent(
			PromptWire.self,
			forKey: .prompt
		)?.value
		let authorizationURLText = try container.decodeIfPresent(
			String.self,
			forKey: .authorizationURL
		)
		let authorizationURL = authorizationURLText.flatMap(URL.init(string:))
		let failure = try container.decodeIfPresent(
			AccountReauthenticationFailure.self,
			forKey: .failure
		)
		let resolvedAccountID = try container.decodeIfPresent(
			String.self,
			forKey: .resolvedAccountID
		)
		guard DecodexNativeClient.isCanonicalUUID(sessionID),
			authorizationURLText == nil || authorizationURL != nil,
			resolvedAccountID.map(DecodexNativeClient.isCanonicalUUID) ?? true,
			Self.hasValidShape(
				state: state,
				prompt: prompt,
				authorizationURL: authorizationURL,
				failure: failure,
				resolvedAccountID: resolvedAccountID
			)
		else {
			throw AccountControlError.invalidResponse
		}
		value = AccountReauthenticationStatus(
			sessionID: sessionID,
			state: state,
			prompt: prompt,
			authorizationURL: authorizationURL,
			failure: failure,
			resolvedAccountID: resolvedAccountID
		)
	}

	private static func hasValidShape(
		state: AccountReauthenticationState,
		prompt: AccountReauthenticationPrompt?,
		authorizationURL: URL?,
		failure: AccountReauthenticationFailure?,
		resolvedAccountID: String?
	) -> Bool {
		switch state {
		case .completed:
			return prompt == nil && authorizationURL == nil && failure == nil
				&& resolvedAccountID != nil
		case .requestingCode, .openingBrowser, .installing, .cancelled:
			return prompt == nil && authorizationURL == nil && failure == nil
				&& resolvedAccountID == nil
		case .waitingForBrowser:
			return failure == nil
				&& (prompt != nil) != (authorizationURL != nil)
				&& (authorizationURL.map(Self.isValidAuthorizationURL) ?? true)
				&& resolvedAccountID == nil
		case .failed:
			return prompt == nil && authorizationURL == nil && failure != nil
				&& resolvedAccountID == nil
		}
	}

	private static func isValidAuthorizationURL(_ value: URL) -> Bool {
		guard value.absoluteString.utf8.count <= 8 * 1_024,
			let components = URLComponents(url: value, resolvingAgainstBaseURL: false)
		else {
			return false
		}
		return components.scheme == "https"
			&& components.host?.lowercased() == "auth.openai.com"
			&& components.port == nil
			&& components.user == nil
			&& components.password == nil
			&& components.path == "/oauth/authorize"
			&& components.fragment == nil
			&& components.queryItems?.isEmpty == false
	}

	private struct PromptWire: Decodable {
		let value: AccountReauthenticationPrompt

		init(from decoder: Decoder) throws {
			try requireExactFields(
				in: decoder,
				expected: ["verification_url", "user_code"]
			)
			let container = try decoder.container(keyedBy: CodingKeys.self)
			let verificationURLText = try container.decode(
				String.self,
				forKey: .verificationURL
			)
			let userCode = try container.decode(String.self, forKey: .userCode)
			guard let verificationURL = URL(string: verificationURLText),
				verificationURL == AccountReauthenticationPrompt.verificationURL,
				Self.isValidUserCode(userCode)
			else {
				throw AccountControlError.invalidResponse
			}
			value = AccountReauthenticationPrompt(
				verificationURL: verificationURL,
				userCode: userCode
			)
		}

		private static func isValidUserCode(_ value: String) -> Bool {
			let bytes = Array(value.utf8)
			guard (9 ... 10).contains(bytes.count), bytes[4] == 0x2d else {
				return false
			}
			return bytes.enumerated().allSatisfy { index, byte in
				if index == 4 {
					return true
				}
				return (byte >= 0x30 && byte <= 0x39)
					|| (byte >= 0x41 && byte <= 0x5a)
			}
		}

		private enum CodingKeys: String, CodingKey {
			case verificationURL = "verification_url"
			case userCode = "user_code"
		}
	}

	private enum CodingKeys: String, CodingKey {
		case sessionID = "session_id"
		case state
		case prompt
		case authorizationURL = "authorization_url"
		case failure
		case resolvedAccountID = "resolved_account_id"
	}
}
private enum AccountControlExpectedResult {
	case accountChanged(accountID: String, enabled: Bool?)
	case accountEnrollment(requestedAccountID: String, enabled: Bool)
	case accountLoggedOut(accountID: String)
	case routing(mode: AccountRoutingMode)
	case routingOrder([String])
	case routed(accountID: String)
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
	case accountRestored(requestedAccountID: String, account: ResetCardAccountWire)
	case accountLoggedOut(accountID: String, tombstoneRevision: UInt64)
	case routingChanged(AccountRoutingWire)
	case routed(account: ResetCardAccountWire, routing: AccountRoutingWire, projectionDigest: String)
	case routePending(AccountRoutePending)

	init(from decoder: Decoder) throws {
		let container = try decoder.container(keyedBy: CodingKeys.self)
		switch try container.decode(String.self, forKey: .name) {
		case "account_changed":
			try requireExactFields(in: decoder, expected: ["name", "data"])
			let data = try container.decode(AccountChangedData.self, forKey: .data)
			self = .accountChanged(data.account)
		case "account_restored":
			try requireExactFields(in: decoder, expected: ["name", "data"])
			let data = try container.decode(AccountRestoredData.self, forKey: .data)
			guard DecodexNativeClient.isCanonicalAccountID(data.requestedAccountID) else {
				throw AccountControlError.invalidResponse
			}
			self = .accountRestored(
				requestedAccountID: data.requestedAccountID,
				account: data.account
			)
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
		case "account_routed":
			try requireExactFields(in: decoder, expected: ["name", "data"])
			let data = try container.decode(AccountRoutedData.self, forKey: .data)
			guard CodexAuthProjectionWire.isSHA256(data.projectionDigest) else {
				throw AccountControlError.invalidResponse
			}
			self = .routed(
				account: data.account,
				routing: data.routing,
				projectionDigest: data.projectionDigest
			)
		case "account_route_pending":
			try requireExactFields(in: decoder, expected: ["name", "data"])
			let data = try container.decode(AccountRoutePendingData.self, forKey: .data)
			self = .routePending(data.pending)
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
			.accountChanged(let wire),
			.accountEnrollment(let requestedAccountID, let expectedEnabled)
		):
			let decoded = try wire.record()
			guard decoded.accountID == requestedAccountID,
				decoded.accountRevision == entityRevision,
				decoded.enabled == expectedEnabled
			else {
				throw AccountControlError.invalidResponse
			}
			return .accountChanged(decoded.withAuthority(authority))
		case (
			.accountRestored(let requestedAccountID, let wire),
			.accountEnrollment(let expectedRequestedID, let expectedEnabled)
		):
			let decoded = try wire.record()
			guard requestedAccountID == expectedRequestedID,
				decoded.accountID != requestedAccountID,
				decoded.accountRevision == entityRevision,
				decoded.enabled == expectedEnabled
			else {
				throw AccountControlError.invalidResponse
			}
			return .accountChanged(decoded.withAuthority(authority))
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
			.routed(let accountWire, let routingWire, let projectionDigest),
			.routed(let expectedID)
		):
			let account = try accountWire.record()
			let routing = routingWire.routing
			guard account.accountID == expectedID,
				account.accountRevision > 0,
				routing.revision == entityRevision,
				routing.mode == .fixed(accountID: expectedID),
				CodexAuthProjectionWire.isSHA256(projectionDigest)
			else {
				throw AccountControlError.invalidResponse
			}
			return .routed(
				account: account.withAuthority(authority),
				routing: routing,
				projectionDigest: projectionDigest
			)
		case (.routePending(let pending), .routed(let expectedID)):
			guard pending.accountID == expectedID,
				pending.routingRevision == entityRevision
			else {
				throw AccountControlError.invalidResponse
			}
			return .routePending(pending)
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

	private struct AccountRestoredData: Decodable {
		let requestedAccountID: String
		let account: ResetCardAccountWire

		init(from decoder: Decoder) throws {
			try requireExactFields(
				in: decoder,
				expected: ["requested_account_id", "account"]
			)
			let container = try decoder.container(keyedBy: CodingKeys.self)
			requestedAccountID = try container.decode(String.self, forKey: .requestedAccountID)
			account = try container.decode(ResetCardAccountWire.self, forKey: .account)
		}

		private enum CodingKeys: String, CodingKey {
			case requestedAccountID = "requested_account_id"
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

	private struct AccountRoutedData: Decodable {
		let account: ResetCardAccountWire
		let routing: AccountRoutingWire
		let projectionDigest: String

		init(from decoder: Decoder) throws {
			try requireExactFields(
				in: decoder,
				expected: ["account", "routing", "projection_digest"]
			)
			let container = try decoder.container(keyedBy: CodingKeys.self)
			account = try container.decode(ResetCardAccountWire.self, forKey: .account)
			routing = try container.decode(AccountRoutingWire.self, forKey: .routing)
			projectionDigest = try container.decode(String.self, forKey: .projectionDigest)
		}

		private enum CodingKeys: String, CodingKey {
			case account
			case routing
			case projectionDigest = "projection_digest"
		}
	}

	private struct AccountRoutePendingData: Decodable {
		let pending: AccountRoutePending

		init(from decoder: Decoder) throws {
			try requireExactFields(in: decoder, expected: ["pending"])
			let container = try decoder.container(keyedBy: CodingKeys.self)
			pending = try container.decode(AccountRoutePending.self, forKey: .pending)
		}

		private enum CodingKeys: String, CodingKey {
			case pending
		}
	}

	private enum CodingKeys: String, CodingKey {
		case name
		case data
	}
}

private extension ResetCardAccountRecord {
	func withAuthority(_ authority: ResetCardAuthority?) -> ResetCardAccountRecord {
		ResetCardAccountRecord(
			authority: authority,
			accountID: accountID,
			alias: alias,
			accountRevision: accountRevision,
			enabled: enabled,
			observedState: observedState,
			lifecycleReadiness: lifecycleReadiness,
			credentialBinding: credentialBinding,
			unsettledOperation: unsettledOperation,
			fiveHourQuota: fiveHourQuota,
			sevenDayQuota: sevenDayQuota
		)
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
