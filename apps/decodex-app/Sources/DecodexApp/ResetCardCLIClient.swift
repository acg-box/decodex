import Foundation

private let resetCardNativeItemLimit = 64
private let accountNativeItemLimit = 512

enum ResetCardServiceError: String, Decodable, Equatable, Sendable {
	case invalidRequest = "invalid_request"
	case accountNotFound = "account_not_found"
	case accountStateRejected = "account_state_rejected"
	case vaultUnavailable = "vault_unavailable"
	case schemaUnsupported = "schema_unsupported"
	case providerUnavailable = "provider_unavailable"
	case inventoryIncomplete = "inventory_incomplete"
	case inventoryChanged = "inventory_changed"
	case requestTimedOut = "request_timed_out"
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
		case .requestTimedOut:
			return "The reset-card provider did not respond in time."
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
	case error(ResetCardQuotaError)
}

struct ResetCardQuotaWindow: Equatable, Sendable {
	let durationMinutes: UInt32
	let observedAtUnixMicros: Int64?
	let state: ResetCardQuotaState

	var usedPercent: UInt8? {
		switch state {
		case .current(let usedPercent, _):
			return usedPercent
		case .unknown, .error:
			return nil
		}
	}

	var resetDate: Date? {
		let micros: Int64
		switch state {
		case .current(_, let resetsAtUnixMicros):
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
		case .current:
			return ""
		}
	}

	var accessibilityValue: String {
		switch state {
		case .unknown:
			return "Unknown, no observation"
		case .current(let usedPercent, _):
			return "Current, \(usedPercent) percent used, resets \(resetDate?.formatted() ?? "unknown")"
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

struct ResetCardAccountRecord: Identifiable, Equatable, Sendable {
	let authority: ResetCardAuthority?
	let accountID: String
	let alias: String
	let accountRevision: UInt64
	let enabled: Bool
	let observedState: ResetCardObservedState
	let lifecycleReadiness: ResetCardLifecycleReadiness
	let credentialBinding: AccountCredentialBinding?
	let unsettledOperation: AccountUnsettledOperation?
	let fiveHourQuota: ResetCardQuotaWindow
	let sevenDayQuota: ResetCardQuotaWindow

	init(
		authority: ResetCardAuthority?,
		accountID: String,
		alias: String,
		accountRevision: UInt64,
		enabled: Bool,
		observedState: ResetCardObservedState,
		lifecycleReadiness: ResetCardLifecycleReadiness,
		credentialBinding: AccountCredentialBinding? = nil,
		unsettledOperation: AccountUnsettledOperation? = nil,
		fiveHourQuota: ResetCardQuotaWindow,
		sevenDayQuota: ResetCardQuotaWindow
	) {
		self.authority = authority
		self.accountID = accountID
		self.alias = alias
		self.accountRevision = accountRevision
		self.enabled = enabled
		self.observedState = observedState
		self.lifecycleReadiness = lifecycleReadiness
		self.credentialBinding = credentialBinding
		self.unsettledOperation = unsettledOperation
		self.fiveHourQuota = fiveHourQuota
		self.sevenDayQuota = sevenDayQuota
	}

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
	let reportedAvailableCount: UInt64?
	let detailsComplete: Bool
	let cards: [ResetCardDescriptor]
	let fiveHourQuota: ResetCardQuotaWindow
	let sevenDayQuota: ResetCardQuotaWindow
	let observationError: ResetCardServiceError?

	init(
		authority: ResetCardAuthority,
		accountID: String,
		accountRevision: UInt64,
		reportedAvailableCount: UInt64? = nil,
		detailsComplete: Bool = true,
		cards: [ResetCardDescriptor],
		fiveHourQuota: ResetCardQuotaWindow,
		sevenDayQuota: ResetCardQuotaWindow,
		observationError: ResetCardServiceError?
	) {
		self.authority = authority
		self.accountID = accountID
		self.accountRevision = accountRevision
		self.reportedAvailableCount = reportedAvailableCount
			?? (detailsComplete ? UInt64(cards.count) : nil)
		self.detailsComplete = detailsComplete
		self.cards = cards
		self.fiveHourQuota = fiveHourQuota
		self.sevenDayQuota = sevenDayQuota
		self.observationError = observationError
	}
}

enum ResetCardClientError: Error, Equatable, LocalizedError, Sendable, CustomDebugStringConvertible {
	case nativeClientUnavailable
	case timedOut
	case transportDisconnected
	case transportBackpressured
	case outputTooLarge
	case commandRejected
	case useDefinitelyNotDispatched
	case usePotentiallyDispatched
	case invalidResponse
	case service(ResetCardServiceError)

	var errorDescription: String? {
		switch self {
		case .nativeClientUnavailable:
			return "The native Decodex client is unavailable."
		case .timedOut:
			return "The Decodex request timed out."
		case .transportDisconnected:
			return "The Decodex daemon is not reachable."
		case .transportBackpressured:
			return "The Decodex service is busy."
		case .outputTooLarge:
			return "The Decodex service returned too much data."
		case .commandRejected:
			return "The reset-card request was rejected. Refresh and try again."
		case .useDefinitelyNotDispatched:
			return "The reset-card request was not dispatched."
		case .usePotentiallyDispatched:
			return "The reset-card request may have been dispatched."
		case .invalidResponse:
			return "The Decodex service returned an invalid reset-card response."
		case .service(let error):
			return error.presentation
		}
	}

	var debugDescription: String {
		switch self {
		case .nativeClientUnavailable:
			return "ResetCardClientError.nativeClientUnavailable"
		case .timedOut:
			return "ResetCardClientError.timedOut"
		case .transportDisconnected:
			return "ResetCardClientError.transportDisconnected"
		case .transportBackpressured:
			return "ResetCardClientError.transportBackpressured"
		case .outputTooLarge:
			return "ResetCardClientError.outputTooLarge"
		case .commandRejected:
			return "ResetCardClientError.commandRejected"
		case .useDefinitelyNotDispatched:
			return "ResetCardClientError.useDefinitelyNotDispatched"
		case .usePotentiallyDispatched:
			return "ResetCardClientError.usePotentiallyDispatched"
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

struct AccountObservationSignal: Decodable, Equatable, Sendable {
	let generation: UInt64

	init(generation: UInt64) {
		self.generation = generation
	}

	init(from decoder: Decoder) throws {
		try requireExactFields(in: decoder, expected: ["generation"])
		let container = try decoder.container(keyedBy: CodingKeys.self)
		generation = try container.decode(UInt64.self, forKey: .generation)
	}

	private enum CodingKeys: String, CodingKey {
		case generation
	}
}

protocol AccountObservationClient: Sendable {
	func waitForAccountObservation(
		afterGeneration: UInt64
	) async throws -> AccountObservationSignal
	func requestAccountObservationRefresh(
		afterGeneration: UInt64
	) async throws -> AccountObservationSignal
}

extension AccountObservationClient {
	func requestAccountObservationRefresh(
		afterGeneration: UInt64
	) async throws -> AccountObservationSignal {
		try await waitForAccountObservation(afterGeneration: afterGeneration)
	}
}

extension ResetCardClient {
	func accounts() async throws -> [ResetCardAccountRecord] {
		try await accounts(authority: nil)
	}
}

extension DecodexNativeClient: ResetCardClient {
	func accounts(
		authority: ResetCardAuthority?
	) async throws -> [ResetCardAccountRecord] {
		try await accountSnapshot(authority: authority).accounts
	}

	func accountSnapshot(
		authority: ResetCardAuthority?
	) async throws -> AccountControlSnapshot {
		let response: (
			authority: ResetCardAuthority,
			data: AccountListWireResult
		) = try await perform(
			DecodexNativeRequest(operation: "list_accounts"),
			authority: authority
		)
		switch response.data {
		case .available(let data):
			return try data.snapshot(authority: response.authority)
		case .unavailable:
			throw ResetCardClientError.service(.productStateUnavailable)
		}
	}

	func inventory(for account: ResetCardAccountRecord) async throws -> ResetCardInventory {
		guard Self.isCanonicalAccountID(account.accountID),
			let authority = account.authority,
			Self.isValidAuthority(authority)
		else {
			throw ResetCardClientError.invalidResponse
		}
		let response: (
			authority: ResetCardAuthority,
			data: ResetCardInventoryWireResult
		) = try await perform(
			DecodexNativeRequest(
				operation: "get_reset_cards",
				accountID: account.accountID
			),
			authority: authority
		)

		let inventory: ResetCardInventory
		switch response.data {
		case .available(let value):
			inventory = try value.inventory(authority: response.authority)
		case .observationFailed(let value):
			inventory = try value.inventory(authority: response.authority)
		case .unavailable(let error):
			throw ResetCardClientError.service(error)
		}
		guard inventory.accountID == account.accountID else {
			throw ResetCardClientError.invalidResponse
		}
		return inventory
	}

	func use(_ attempt: ResetCardUseAttempt) async throws -> ResetCardOperationState {
		guard Self.isCanonicalAccountID(attempt.target.accountID),
			attempt.target.expectedRevision > 0,
			Self.isCanonicalUUID(attempt.idempotencyKey),
			Self.isValidAuthority(attempt.target.authority)
		else {
			throw ResetCardClientError.invalidResponse
		}
		let response: (
			authority: ResetCardAuthority,
			data: DecodexNativeResetCardConsumeResult
		) = try await perform(
			DecodexNativeRequest(
				operation: "use_reset_card",
				accountID: attempt.target.accountID,
				grantedAtUnixSeconds: attempt.target.descriptor.grantedAtUnixSeconds,
				expiresAtUnixSeconds: attempt.target.descriptor.expiresAtUnixSeconds,
				expectedRevision: attempt.target.expectedRevision,
				idempotencyKey: attempt.idempotencyKey
			),
			authority: attempt.target.authority
		)

		switch response.data {
		case .accepted(
			let accountID,
			let descriptor,
			let state,
			let entityRevision
		):
			guard accountID == attempt.target.accountID,
				try descriptor.descriptor == attempt.target.descriptor,
				entityRevision == attempt.target.expectedRevision
			else {
				throw ResetCardClientError.invalidResponse
			}
			return state.state
		case .rejected:
			throw ResetCardClientError.commandRejected
		case .potentiallyDispatched:
			throw ResetCardClientError.usePotentiallyDispatched
		}
	}

	func status(for attempt: ResetCardUseAttempt) async throws -> ResetCardOperationState {
		guard Self.isCanonicalUUID(attempt.idempotencyKey),
			Self.isValidAuthority(attempt.target.authority)
		else {
			throw ResetCardClientError.invalidResponse
		}
		let response: (
			authority: ResetCardAuthority,
			data: ResetCardOperationWireResult
		) = try await perform(
			DecodexNativeRequest(
				operation: "reset_card_status",
				idempotencyKey: attempt.idempotencyKey
			),
			authority: attempt.target.authority
		)
		return response.data.state
	}
}

extension DecodexNativeClient: AccountObservationClient {
	func waitForAccountObservation(
		afterGeneration: UInt64
	) async throws -> AccountObservationSignal {
		try await waitForAccountObservation(
			afterGeneration: afterGeneration,
			requestRefresh: false
		)
	}

	func requestAccountObservationRefresh(
		afterGeneration: UInt64
	) async throws -> AccountObservationSignal {
		try await waitForAccountObservation(
			afterGeneration: afterGeneration,
			requestRefresh: true
		)
	}

	private func waitForAccountObservation(
		afterGeneration: UInt64,
		requestRefresh: Bool
	) async throws -> AccountObservationSignal {
		let response: (
			authority: ResetCardAuthority,
			data: AccountObservationSignal
		) = try await perform(
			DecodexNativeRequest(
				operation: "wait_for_account_observation",
				afterGeneration: afterGeneration,
				requestRefresh: requestRefresh ? true : nil
			),
			authority: nil
		)
		return response.data
	}
}
struct ResetCardAnyCodingKey: CodingKey {
	let stringValue: String
	let intValue: Int? = nil

	init?(stringValue: String) {
		self.stringValue = stringValue
	}

	init?(intValue: Int) {
		return nil
	}
}

func rejectUnknownFields(
	in decoder: Decoder,
	allowed: Set<String>
) throws {
	let container = try decoder.container(keyedBy: ResetCardAnyCodingKey.self)
	guard container.allKeys.allSatisfy({ allowed.contains($0.stringValue) }) else {
		throw ResetCardClientError.invalidResponse
	}
}

func requireExactFields(
	in decoder: Decoder,
	expected: Set<String>
) throws {
	let container = try decoder.container(keyedBy: ResetCardAnyCodingKey.self)
	guard Set(container.allKeys.map(\.stringValue)) == expected else {
		throw ResetCardClientError.invalidResponse
	}
}

func isBoundedWireText(
	_ value: String,
	maximumBytes: Int
) -> Bool {
	value.isEmpty == false
		&& value.utf8.count <= maximumBytes
		&& value.unicodeScalars.contains {
			$0.properties.generalCategory == .control
		} == false
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

struct AccountListWireData: Decodable {
	let accounts: [ResetCardAccountWire]
	let routing: AccountRoutingWire

	init(from decoder: Decoder) throws {
		try rejectUnknownFields(in: decoder, allowed: ["accounts", "routing"])
		let container = try decoder.container(keyedBy: CodingKeys.self)
		accounts = try container.decode([ResetCardAccountWire].self, forKey: .accounts)
		routing = try container.decode(AccountRoutingWire.self, forKey: .routing)
	}

	func snapshot(
		authority: ResetCardAuthority?
	) throws -> AccountControlSnapshot {
		guard accounts.count <= accountNativeItemLimit else {
			throw ResetCardClientError.invalidResponse
		}
		let records = try accounts.map {
			let record = try $0.record()
			return ResetCardAccountRecord(
				authority: authority,
				accountID: record.accountID,
				alias: record.alias,
				accountRevision: record.accountRevision,
				enabled: record.enabled,
				observedState: record.observedState,
				lifecycleReadiness: record.lifecycleReadiness,
				credentialBinding: record.credentialBinding,
				unsettledOperation: record.unsettledOperation,
				fiveHourQuota: record.fiveHourQuota,
				sevenDayQuota: record.sevenDayQuota
			)
		}
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

		let ordered = try routing.order.map {
			guard let account = byID[$0] else {
				throw ResetCardClientError.invalidResponse
			}
			return account
		}
		return AccountControlSnapshot(
			authority: authority,
			accounts: ordered,
			routing: routing.routing
		)
	}

	func orderedAccounts() throws -> [ResetCardAccountRecord] {
		try snapshot(authority: nil).accounts
	}

	private enum CodingKeys: String, CodingKey {
		case accounts
		case routing
	}
}

struct AccountRoutingWire: Decodable {
	let revision: UInt64
	let mode: AccountRoutingModeWire
	let order: [String]

	init(from decoder: Decoder) throws {
		try rejectUnknownFields(in: decoder, allowed: ["revision", "mode", "order"])
		let container = try decoder.container(keyedBy: CodingKeys.self)
		revision = try container.decode(UInt64.self, forKey: .revision)
		mode = try container.decode(AccountRoutingModeWire.self, forKey: .mode)
		order = try container.decode([String].self, forKey: .order)
		guard revision > 0,
			order.count <= accountNativeItemLimit,
			Set(order).count == order.count,
			order.allSatisfy(DecodexNativeClient.isCanonicalAccountID)
		else {
			throw ResetCardClientError.invalidResponse
		}
		if case .fixed(let accountID) = mode,
			order.contains(accountID) == false
		{
			throw ResetCardClientError.invalidResponse
		}
	}

	var routing: AccountRoutingControl {
		AccountRoutingControl(
			revision: revision,
			mode: mode.mode,
			order: order
		)
	}

	private enum CodingKeys: String, CodingKey {
		case revision
		case mode
		case order
	}
}

enum AccountRoutingModeWire: Decodable {
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
			guard DecodexNativeClient.isCanonicalAccountID(accountID) else {
				throw ResetCardClientError.invalidResponse
			}
			self = .fixed(accountID)
		default:
			throw ResetCardClientError.invalidResponse
		}
	}

	var mode: AccountRoutingMode {
		switch self {
		case .balanced:
			return .balanced
		case .fixed(let accountID):
			return .fixed(accountID: accountID)
		}
	}

	private enum CodingKeys: String, CodingKey {
		case mode
		case accountID = "account_id"
	}
}

struct ResetCardAccountWire: Decodable, Sendable {
	let accountID: String
	let alias: String
	let enabled: Bool
	let accountRevision: UInt64
	let observedState: ResetCardObservedState
	let lifecycleReadiness: ResetCardLifecycleReadiness
	let credentialBinding: AccountCredentialBindingWire?
	let unsettledOperation: AccountUnsettledOperationWire?
	private let fiveHourQuota: ResetCardQuotaWindowWire
	private let sevenDayQuota: ResetCardQuotaWindowWire

	enum CodingKeys: String, CodingKey {
		case accountID = "account_id"
		case alias
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
				"alias",
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
		alias = try container.decode(String.self, forKey: .alias)
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
		guard DecodexNativeClient.isCanonicalAccountID(accountID),
			Self.isCanonicalAlias(alias),
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
			alias: alias,
			accountRevision: accountRevision,
			enabled: enabled,
			observedState: observedState,
			lifecycleReadiness: lifecycleReadiness,
			credentialBinding: credentialBinding?.binding,
			unsettledOperation: unsettledOperation?.operation,
			fiveHourQuota: fiveHourQuota,
			sevenDayQuota: sevenDayQuota
		)
	}

	private static func isCanonicalAlias(_ value: String) -> Bool {
		let bytes = Array(value.utf8)
		guard (2 ... 16).contains(bytes.count),
			let first = bytes.first,
			(65 ... 90).contains(first)
		else {
			return false
		}
		return bytes.dropFirst().allSatisfy { (97 ... 122).contains($0) }
	}
}

struct AccountCredentialBindingWire: Decodable, Sendable {
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

	var binding: AccountCredentialBinding {
		AccountCredentialBinding(
			schemaVersion: schemaVersion,
			version: version,
			fingerprintSHA256: fingerprintSHA256,
			provider: .chatGPT,
			providerAccountID: providerAccountID
		)
	}

	private enum CodingKeys: String, CodingKey {
		case schemaVersion = "schema_version"
		case version
		case fingerprintSHA256 = "fingerprint_sha256"
		case provider
		case providerAccountID = "provider_account_id"
	}
}

struct AccountUnsettledOperationWire: Decodable, Sendable {
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
		guard DecodexNativeClient.isCanonicalUUID(operationID),
			(recoveryCode.map {
				isBoundedWireText($0, maximumBytes: 128)
			} ?? true),
			(phase == .recoveryRequired) == (recoveryCode != nil)
		else {
			throw ResetCardClientError.invalidResponse
		}
	}

	var operation: AccountUnsettledOperation {
		AccountUnsettledOperation(
			operationID: operationID,
			kind: kind.kind,
			phase: phase.phase,
			recoveryCode: recoveryCode
		)
	}

	private enum CodingKeys: String, CodingKey {
		case operationID = "operation_id"
		case kind
		case phase
		case recoveryCode = "recovery_code"
	}
}

enum AccountOperationKindWire: String, Decodable, Sendable {
	case enroll
	case `import`
	case refresh
	case logout

	var kind: AccountOperationKind {
		switch self {
		case .enroll:
			return .enroll
		case .import:
			return .import
		case .refresh:
			return .refresh
		case .logout:
			return .logout
		}
	}
}

enum AccountOperationPhaseWire: String, Decodable, Sendable {
	case prepared
	case providerEffectPending = "provider_effect_pending"
	case storeApplied = "store_applied"
	case recoveryRequired = "recovery_required"

	var phase: AccountOperationPhase {
		switch self {
		case .prepared:
			return .prepared
		case .providerEffectPending:
			return .providerEffectPending
		case .storeApplied:
			return .storeApplied
		case .recoveryRequired:
			return .recoveryRequired
		}
	}
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
	let reportedAvailableCount: UInt64?
	let detailsComplete: Bool
	let cards: [ResetCardObservationWire]
	let fiveHourQuota: ResetCardQuotaWindowWire
	let sevenDayQuota: ResetCardQuotaWindowWire

	enum CodingKeys: String, CodingKey {
		case accountID = "account_id"
		case accountRevision = "account_revision"
		case reportedAvailableCount = "reported_available_count"
		case detailsComplete = "details_complete"
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
				"reported_available_count",
				"details_complete",
				"cards",
				"five_hour_quota",
				"seven_day_quota",
			]
		)
		let container = try decoder.container(keyedBy: CodingKeys.self)
		accountID = try container.decode(String.self, forKey: .accountID)
		accountRevision = try container.decode(UInt64.self, forKey: .accountRevision)
		reportedAvailableCount = try container.decodeIfPresent(
			UInt64.self,
			forKey: .reportedAvailableCount
		)
		detailsComplete = try container.decode(Bool.self, forKey: .detailsComplete)
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
		guard DecodexNativeClient.isValidAuthority(authority),
			DecodexNativeClient.isCanonicalAccountID(accountID),
			accountRevision > 0,
			cards.count <= resetCardNativeItemLimit
		else {
			throw ResetCardClientError.invalidResponse
		}
		if detailsComplete {
			guard reportedAvailableCount == UInt64(cards.count) else {
				throw ResetCardClientError.invalidResponse
			}
		} else {
			guard cards.isEmpty, reportedAvailableCount != 0 else {
				throw ResetCardClientError.invalidResponse
			}
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
			reportedAvailableCount: reportedAvailableCount,
			detailsComplete: detailsComplete,
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
		guard DecodexNativeClient.isValidAuthority(authority),
			DecodexNativeClient.isCanonicalAccountID(accountID),
			accountRevision > 0
		else {
			throw ResetCardClientError.invalidResponse
		}
		return ResetCardInventory(
			authority: authority,
			accountID: accountID,
			accountRevision: accountRevision,
			reportedAvailableCount: nil,
			detailsComplete: false,
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

private enum DecodexNativeResetCardConsumeResult: Decodable, Sendable {
	case accepted(
		accountID: String,
		descriptor: ResetCardDescriptorWire,
		state: ResetCardOperationWireResult,
		entityRevision: UInt64
	)
	case rejected(ResetCardCommandErrorWire)
	case potentiallyDispatched(DecodexNativeFailure)

	init(from decoder: Decoder) throws {
		let container = try decoder.container(keyedBy: CodingKeys.self)
		switch try container.decode(String.self, forKey: .outcome) {
		case "accepted":
			try requireExactFields(in: decoder, expected: ["outcome", "data"])
			let data = try container.decode(AcceptedData.self, forKey: .data)
			self = .accepted(
				accountID: data.accountID,
				descriptor: data.descriptor,
				state: data.state,
				entityRevision: data.entityRevision
			)
		case "rejected":
			try requireExactFields(in: decoder, expected: ["outcome", "data"])
			let data = try container.decode(RejectedData.self, forKey: .data)
			self = .rejected(data.error)
		case "potentially_dispatched":
			try requireExactFields(in: decoder, expected: ["outcome", "data"])
			let data = try container.decode(PotentiallyDispatchedData.self, forKey: .data)
			self = .potentiallyDispatched(data.failure)
		default:
			throw ResetCardClientError.invalidResponse
		}
	}

	private enum CodingKeys: String, CodingKey {
		case outcome
		case data
	}

	private struct AcceptedData: Decodable, Sendable {
		let accountID: String
		let descriptor: ResetCardDescriptorWire
		let state: ResetCardOperationWireResult
		let entityRevision: UInt64

		init(from decoder: Decoder) throws {
			try requireExactFields(
				in: decoder,
				expected: ["account_id", "descriptor", "state", "entity_revision"]
			)
			let container = try decoder.container(keyedBy: CodingKeys.self)
			accountID = try container.decode(String.self, forKey: .accountID)
			descriptor = try container.decode(
				ResetCardDescriptorWire.self,
				forKey: .descriptor
			)
			state = try container.decode(
				ResetCardOperationWireResult.self,
				forKey: .state
			)
			entityRevision = try container.decode(UInt64.self, forKey: .entityRevision)
		}

		private enum CodingKeys: String, CodingKey {
			case accountID = "account_id"
			case descriptor
			case state
			case entityRevision = "entity_revision"
		}
	}

	private struct RejectedData: Decodable, Sendable {
		let error: ResetCardCommandErrorWire

		init(from decoder: Decoder) throws {
			try requireExactFields(in: decoder, expected: ["error"])
			let container = try decoder.container(keyedBy: CodingKeys.self)
			error = try container.decode(ResetCardCommandErrorWire.self, forKey: .error)
		}

		private enum CodingKeys: String, CodingKey {
			case error
		}
	}

	private struct PotentiallyDispatchedData: Decodable, Sendable {
		let failure: DecodexNativeFailure

		init(from decoder: Decoder) throws {
			try requireExactFields(in: decoder, expected: ["failure"])
			let container = try decoder.container(keyedBy: CodingKeys.self)
			failure = try container.decode(DecodexNativeFailure.self, forKey: .failure)
		}

		private enum CodingKeys: String, CodingKey {
			case failure
		}
	}
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
