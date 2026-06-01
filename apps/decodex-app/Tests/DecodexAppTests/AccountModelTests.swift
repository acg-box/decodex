@testable import DecodexApp
import XCTest

final class AccountModelTests: XCTestCase {
	func testRefresh401RequiresReloginAndHidesCapacity() {
		let account = makeAccount(
			status: "unusable",
			recoveryAction: "login",
			refreshStatus: "failed",
			checkedAtUnixEpoch: 1_800_000_000,
			primaryRemainingPercent: 89
		)

		XCTAssertTrue(account.needsLogin)
		XCTAssertFalse(account.canRouteRuns)
		XCTAssertEqual(account.statusLabel, "Re-login required")
		XCTAssertNil(account.currentCapacityLabel)
	}

	func testExpiredAccountCanRefreshButDoesNotShowCapacity() {
		let account = makeAccount(
			status: "expired",
			recoveryAction: "refresh",
			checkedAtUnixEpoch: 1_800_000_000,
			primaryRemainingPercent: 89
		)

		XCTAssertFalse(account.needsLogin)
		XCTAssertTrue(account.canRouteRuns)
		XCTAssertEqual(account.statusLabel, "Refresh needed")
		XCTAssertNil(account.currentCapacityLabel)
	}

	func testAvailableMeasuredAccountShowsCapacity() {
		let account = makeAccount(
			status: "available",
			planType: "pro",
			checkedAtUnixEpoch: 1_800_000_000,
			primaryRemainingPercent: 89
		)

		XCTAssertEqual(account.currentCapacityLabel, "20x")
	}

	func testCompactEmailKeepsDottedLocalSuffixesConsistent() {
		XCTAssertEqual(AccountDisplay.compactEmail("aurevoirxavier@gmail.com"), "aur...ier@gmail.com")
		XCTAssertEqual(AccountDisplay.compactEmail("aurevoirxavier.us@gmail.com"), "aur...us@gmail.com")
		XCTAssertEqual(AccountDisplay.compactEmail("aurevoirxavier.jp@gmail.com"), "aur...jp@gmail.com")
		XCTAssertEqual(AccountDisplay.compactEmail("aurevoirxavier.hk@gmail.com"), "aur...hk@gmail.com")
		XCTAssertEqual(AccountDisplay.compactEmail("xavier.lau@helixbox.ai"), "xav...lau@helixbox.ai")
	}

	private func makeAccount(
		status: String,
		recoveryAction: String? = nil,
		refreshStatus: String? = nil,
		planType: String? = nil,
		checkedAtUnixEpoch: Int? = nil,
		primaryRemainingPercent: Int? = nil
	) -> CodexAccount {
		CodexAccount(
			accountFingerprint: "...123456",
			email: "copy@example.com",
			selector: "copy@example.com",
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
			sevenDayUsedPercent: nil,
			sevenDayDailyAveragePercent: nil,
			usageRecords: nil
		)
	}
}
