@testable import DecodexApp
import XCTest

func dashboardEvent(
	type: String,
	payload: String
) throws -> OperatorDashboardSocketEvent {
	let data = """
	{
	  "type": "\(type)",
	  "payload": \(payload)
	}
	""".data(using: .utf8)!

	return try JSONDecoder().decode(OperatorDashboardSocketEvent.self, from: data)
}

func makeAccount(
	status: String,
	email: String = "copy@example.com",
	accountFingerprint: String = "...123456",
	recoveryAction: String? = nil,
	refreshStatus: String? = nil,
	planType: String? = nil,
	checkedAtUnixEpoch: Int? = nil,
	primaryRemainingPercent: Int? = nil,
	profilePeakDailyTokens: Int? = nil,
	profileDailyUsage: [AccountProfileDailyUsage]? = nil
) -> CodexAccount {
	CodexAccount(
		accountFingerprint: accountFingerprint,
		email: email,
		selector: email,
		randomName: nil,
		randomNameKey: nil,
		randomNameOffset: nil,
		status: status,
		selected: false,
		codexActive: false,
		disabled: false,
		refreshTokenPresent: true,
		accessTokenExpiresAtUnixEpoch: nil,
		lastSelectedAtUnixEpoch: nil,
		cooldownUntilUnixEpoch: nil,
		note: nil,
		planType: planType,
		capacityMultiplier: nil,
		recoveryAction: recoveryAction,
		refreshStatus: refreshStatus,
		checkedAtUnixEpoch: checkedAtUnixEpoch,
		primaryWindowSeconds: nil,
		primaryRemainingPercent: primaryRemainingPercent,
		primaryResetsAtUnixEpoch: nil,
		secondaryWindowSeconds: nil,
		secondaryRemainingPercent: nil,
		secondaryResetsAtUnixEpoch: nil,
		creditsHasCredits: nil,
		creditsUnlimited: nil,
		creditsBalance: nil,
		rateLimitReachedType: nil,
		profileDisplayName: nil,
		profileUsername: nil,
		profileCheckedAtUnixEpoch: nil,
		profileLifetimeTokens: nil,
		profilePeakDailyTokens: profilePeakDailyTokens,
		profileLongestTaskSeconds: nil,
		profileCurrentStreakDays: nil,
		profileLongestStreakDays: nil,
		profileDailyUsage: profileDailyUsage,
		sevenDayUsedPercent: nil,
		sevenDayDailyAveragePercent: nil,
		usageRecords: nil
	)
}

func makeAccountList(_ accounts: [CodexAccount]) -> AccountListResponse {
	AccountListResponse(
		accountsPath: "/tmp/accounts.json",
		globalConfigPath: "/tmp/config.toml",
		codexAuthPath: "/tmp/auth.json",
		codexAuth: nil,
		control: AccountControl(mode: "balanced", accountSelector: nil),
		accounts: accounts,
		usageEstimate: nil,
		usageProbeError: nil
	)
}