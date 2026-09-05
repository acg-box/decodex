import Foundation

enum AccountProfileObservationError: String, Decodable, Equatable, Sendable {
	case invalidRequest = "invalid_request"
	case accountUnavailable = "account_unavailable"
	case productStateUnavailable = "product_state_unavailable"
	case credentialUnavailable = "credential_unavailable"
	case credentialBusy = "credential_busy"
	case refreshRejected = "refresh_rejected"
	case refreshAmbiguous = "refresh_ambiguous"
	case accessRejectedAfterRefresh = "access_rejected_after_refresh"
	case unauthorized
	case providerUnavailable = "provider_unavailable"
	case protocolUnavailable = "protocol_unavailable"
	case accountChanged = "account_changed"

	var presentation: String {
		switch self {
		case .invalidRequest:
			return "The profile request was invalid."
		case .accountUnavailable:
			return "The account is unavailable."
		case .productStateUnavailable:
			return "Account profile state is unavailable."
		case .credentialUnavailable:
			return "The account login is unavailable."
		case .credentialBusy:
			return "Account credentials are busy with another active owner."
		case .refreshRejected:
			return "Credential refresh was rejected. Re-login is required."
		case .refreshAmbiguous:
			return "Credential refresh was uncertain. Re-login is required."
		case .accessRejectedAfterRefresh:
			return "Refreshed credentials are still unauthorized. Re-login is required."
		case .unauthorized:
			return "The account login needs to be refreshed."
		case .providerUnavailable:
			return "The account profile provider is unavailable."
		case .protocolUnavailable:
			return "The provider returned an unsupported profile."
		case .accountChanged:
			return "The account changed while its profile was loading."
		}
	}
}

enum AccountProfileFreshness: Equatable, Sendable {
	case current
	case cached(refreshError: AccountProfileObservationError)
}

struct AccountProfileObservation: Equatable, Sendable {
	let accountID: String
	let accountRevision: UInt64
	let observedAtUnixMicros: Int64
	let email: String?
	let planType: String?
	let displayName: String?
	let username: String?
	let snapshot: AccountProfileSnapshot
	let freshness: AccountProfileFreshness

	var isCached: Bool {
		if case .cached = freshness {
			return true
		}
		return false
	}

	var refreshError: AccountProfileObservationError? {
		if case .cached(let refreshError) = freshness {
			return refreshError
		}
		return nil
	}

	func redactingEmail() -> Self {
		replacingEmail(nil)
	}

	func replacingEmail(_ email: String?) -> Self {
		Self(
			accountID: accountID,
			accountRevision: accountRevision,
			observedAtUnixMicros: observedAtUnixMicros,
			email: email,
			planType: planType,
			displayName: displayName,
			username: username,
			snapshot: snapshot,
			freshness: freshness
		)
	}
}

struct AccountProfileClaims: Equatable, Sendable {
	let email: String?
	let planType: String?

	func redactingEmail() -> Self {
		replacingEmail(nil)
	}

	func replacingEmail(_ email: String?) -> Self {
		Self(email: email, planType: planType)
	}
}

struct AccountProfileUnavailable: Equatable, Sendable {
	let error: AccountProfileObservationError
	let claims: AccountProfileClaims

	func redactingEmail() -> Self {
		replacingEmail(nil)
	}

	func replacingEmail(_ email: String?) -> Self {
		Self(error: error, claims: claims.replacingEmail(email))
	}
}

enum AccountProfileRead: Equatable, Sendable {
	case available(AccountProfileObservation)
	case unavailable(AccountProfileUnavailable)
}

protocol AccountProfileClient: Sendable {
	func profile(
		for account: ResetCardAccountRecord,
		includeEmail: Bool
	) async throws -> AccountProfileRead
}

extension DecodexNativeClient: AccountProfileClient {
	func profile(
		for account: ResetCardAccountRecord,
		includeEmail: Bool
	) async throws -> AccountProfileRead {
		guard Self.isCanonicalAccountID(account.accountID),
			account.accountRevision > 0,
			let authority = account.authority,
			Self.isValidAuthority(authority)
		else {
			throw ResetCardClientError.invalidResponse
		}

		let response: (
			authority: ResetCardAuthority,
			data: AccountProfileWireResult
		) = try await perform(
			DecodexNativeRequest(
				operation: "get_account_profile",
				accountID: account.accountID,
				includeEmail: includeEmail
			),
			authority: authority
		)

		switch response.data {
		case .current(let profile):
			return .available(
				try profile.observation(
					expectedAccount: account,
					includeEmail: includeEmail,
					freshness: .current
				)
			)
		case .cached(let profile, let refreshError):
			return .available(
				try profile.observation(
					expectedAccount: account,
					includeEmail: includeEmail,
					freshness: .cached(refreshError: refreshError)
				)
			)
		case .unavailable(let unavailable):
			return .unavailable(
				try unavailable.value(includeEmail: includeEmail)
			)
		}
	}
}

private enum AccountProfileWireResult: Decodable {
	case current(AccountProfileWire)
	case cached(AccountProfileWire, AccountProfileObservationError)
	case unavailable(UnavailableData)

	init(from decoder: Decoder) throws {
		let container = try decoder.container(keyedBy: CodingKeys.self)
		switch try container.decode(String.self, forKey: .outcome) {
		case "current":
			try requireExactFields(in: decoder, expected: ["outcome", "data"])
			self = .current(
				try container.decode(AccountProfileWire.self, forKey: .data)
			)
		case "cached":
			try requireExactFields(in: decoder, expected: ["outcome", "data"])
			let data = try container.decode(CachedData.self, forKey: .data)
			self = .cached(data.profile, data.refreshError)
		case "unavailable":
			try requireExactFields(in: decoder, expected: ["outcome", "data"])
			let data = try container.decode(UnavailableData.self, forKey: .data)
			self = .unavailable(data)
		default:
			throw ResetCardClientError.invalidResponse
		}
	}

	private struct CachedData: Decodable {
		let profile: AccountProfileWire
		let refreshError: AccountProfileObservationError

		init(from decoder: Decoder) throws {
			try requireExactFields(
				in: decoder,
				expected: ["profile", "refresh_error"]
			)
			let container = try decoder.container(keyedBy: CodingKeys.self)
			profile = try container.decode(AccountProfileWire.self, forKey: .profile)
			refreshError = try container.decode(
				AccountProfileObservationError.self,
				forKey: .refreshError
			)
		}

		private enum CodingKeys: String, CodingKey {
			case profile
			case refreshError = "refresh_error"
		}
	}

	struct UnavailableData: Decodable {
		let error: AccountProfileObservationError
		let email: AccountProfileEmailWire
		let planType: String?

		init(from decoder: Decoder) throws {
			try rejectUnknownFields(
				in: decoder,
				allowed: ["error", "email", "plan_type"]
			)
			let container = try decoder.container(keyedBy: CodingKeys.self)
			error = try container.decode(AccountProfileObservationError.self, forKey: .error)
			email = try container.decode(AccountProfileEmailWire.self, forKey: .email)
			planType = try container.decodeIfPresent(String.self, forKey: .planType)
		}

		func value(includeEmail: Bool) throws -> AccountProfileUnavailable {
			guard planType.map({
				isBoundedWireText($0, maximumBytes: 128)
			}) ?? true else {
				throw ResetCardClientError.invalidResponse
			}
			let visibleEmail: String?
			switch email {
			case .redacted:
				visibleEmail = nil
			case .visible(let value):
				guard includeEmail,
					isBoundedWireText(value, maximumBytes: 320)
				else {
					throw ResetCardClientError.invalidResponse
				}
				visibleEmail = value
			}
			return AccountProfileUnavailable(
				error: error,
				claims: AccountProfileClaims(
					email: visibleEmail,
					planType: planType
				)
			)
		}

		private enum CodingKeys: String, CodingKey {
			case error
			case email
			case planType = "plan_type"
		}
	}

	private enum CodingKeys: String, CodingKey {
		case outcome
		case data
	}
}

private struct AccountProfileWire: Decodable {
	let accountID: String
	let accountRevision: UInt64
	let observedAtUnixMicros: Int64
	let email: AccountProfileEmailWire
	let planType: String?
	let displayName: String?
	let username: String?
	let lifetimeTokens: UInt64?
	let peakDailyTokens: UInt64?
	let longestTaskSeconds: UInt64?
	let currentStreakDays: UInt32?
	let longestStreakDays: UInt32?
	let dailyUsage: [AccountProfileDailyUsageWire]

	init(from decoder: Decoder) throws {
		try rejectUnknownFields(
			in: decoder,
			allowed: [
				"account_id",
				"account_revision",
				"observed_at_unix_micros",
				"email",
				"plan_type",
				"display_name",
				"username",
				"lifetime_tokens",
				"peak_daily_tokens",
				"longest_task_seconds",
				"current_streak_days",
				"longest_streak_days",
				"daily_usage",
			]
		)
		let container = try decoder.container(keyedBy: CodingKeys.self)
		accountID = try container.decode(String.self, forKey: .accountID)
		accountRevision = try container.decode(UInt64.self, forKey: .accountRevision)
		observedAtUnixMicros = try container.decode(Int64.self, forKey: .observedAtUnixMicros)
		email = try container.decode(AccountProfileEmailWire.self, forKey: .email)
		planType = try container.decodeIfPresent(String.self, forKey: .planType)
		displayName = try container.decodeIfPresent(String.self, forKey: .displayName)
		username = try container.decodeIfPresent(String.self, forKey: .username)
		lifetimeTokens = try container.decodeIfPresent(UInt64.self, forKey: .lifetimeTokens)
		peakDailyTokens = try container.decodeIfPresent(UInt64.self, forKey: .peakDailyTokens)
		longestTaskSeconds = try container.decodeIfPresent(
			UInt64.self,
			forKey: .longestTaskSeconds
		)
		currentStreakDays = try container.decodeIfPresent(
			UInt32.self,
			forKey: .currentStreakDays
		)
		longestStreakDays = try container.decodeIfPresent(
			UInt32.self,
			forKey: .longestStreakDays
		)
		dailyUsage = try container.decode(
			[AccountProfileDailyUsageWire].self,
			forKey: .dailyUsage
		)
	}

	func observation(
		expectedAccount: ResetCardAccountRecord,
		includeEmail: Bool,
		freshness: AccountProfileFreshness
	) throws -> AccountProfileObservation {
		guard accountID == expectedAccount.accountID,
			accountRevision == expectedAccount.accountRevision,
			accountRevision > 0,
			observedAtUnixMicros > 0,
			dailyUsage.count <= 36,
			Self.validOptionalText(planType, maximumBytes: 128),
			Self.validOptionalText(displayName, maximumBytes: 256),
			Self.validOptionalText(username, maximumBytes: 256)
		else {
			throw ResetCardClientError.invalidResponse
		}

		let visibleEmail: String?
		switch email {
		case .redacted:
			visibleEmail = nil
		case .visible(let value):
			guard includeEmail,
				isBoundedWireText(value, maximumBytes: 320)
			else {
				throw ResetCardClientError.invalidResponse
			}
			visibleEmail = value
		}

		let usage = dailyUsage.map(\.record)
		guard zip(usage, usage.dropFirst()).allSatisfy({ $0.date < $1.date }) else {
			throw ResetCardClientError.invalidResponse
		}

		return AccountProfileObservation(
			accountID: accountID,
			accountRevision: accountRevision,
			observedAtUnixMicros: observedAtUnixMicros,
			email: visibleEmail,
			planType: planType,
			displayName: displayName,
			username: username,
			snapshot: AccountProfileSnapshot(
				lifetimeTokens: lifetimeTokens,
				peakDailyTokens: peakDailyTokens,
				longestTaskSeconds: longestTaskSeconds,
				currentStreakDays: currentStreakDays,
				longestStreakDays: longestStreakDays,
				dailyUsage: usage
			),
			freshness: freshness
		)
	}

	private static func validOptionalText(
		_ value: String?,
		maximumBytes: Int
	) -> Bool {
		value.map {
			isBoundedWireText($0, maximumBytes: maximumBytes)
		} ?? true
	}

	private enum CodingKeys: String, CodingKey {
		case accountID = "account_id"
		case accountRevision = "account_revision"
		case observedAtUnixMicros = "observed_at_unix_micros"
		case email
		case planType = "plan_type"
		case displayName = "display_name"
		case username
		case lifetimeTokens = "lifetime_tokens"
		case peakDailyTokens = "peak_daily_tokens"
		case longestTaskSeconds = "longest_task_seconds"
		case currentStreakDays = "current_streak_days"
		case longestStreakDays = "longest_streak_days"
		case dailyUsage = "daily_usage"
	}
}

private enum AccountProfileEmailWire: Decodable {
	case redacted
	case visible(String)

	init(from decoder: Decoder) throws {
		let container = try decoder.container(keyedBy: CodingKeys.self)
		switch try container.decode(String.self, forKey: .visibility) {
		case "redacted":
			try requireExactFields(in: decoder, expected: ["visibility"])
			self = .redacted
		case "visible":
			try requireExactFields(in: decoder, expected: ["visibility", "value"])
			self = .visible(try container.decode(String.self, forKey: .value))
		default:
			throw ResetCardClientError.invalidResponse
		}
	}

	private enum CodingKeys: String, CodingKey {
		case visibility
		case value
	}
}

private struct AccountProfileDailyUsageWire: Decodable {
	let startDate: String
	let tokens: UInt64

	init(from decoder: Decoder) throws {
		try requireExactFields(in: decoder, expected: ["start_date", "tokens"])
		let container = try decoder.container(keyedBy: CodingKeys.self)
		startDate = try container.decode(String.self, forKey: .startDate)
		tokens = try container.decode(UInt64.self, forKey: .tokens)
		guard Self.isCanonicalDate(startDate) else {
			throw ResetCardClientError.invalidResponse
		}
	}

	var record: AccountProfileDailyUsage {
		AccountProfileDailyUsage(date: startDate, tokens: tokens)
	}

	private static func isCanonicalDate(_ value: String) -> Bool {
		let bytes = Array(value.utf8)
		guard bytes.count == 10,
			bytes[4] == 45,
			bytes[7] == 45,
			bytes.enumerated().allSatisfy({ index, byte in
				index == 4 || index == 7 || (byte >= 48 && byte <= 57)
			})
		else {
			return false
		}

		let formatter = DateFormatter()
		formatter.calendar = Calendar(identifier: .gregorian)
		formatter.locale = Locale(identifier: "en_US_POSIX")
		formatter.timeZone = TimeZone(secondsFromGMT: 0)
		formatter.dateFormat = "yyyy-MM-dd"
		formatter.isLenient = false
		guard let date = formatter.date(from: value) else {
			return false
		}
		return formatter.string(from: date) == value
	}

	private enum CodingKeys: String, CodingKey {
		case startDate = "start_date"
		case tokens
	}
}
