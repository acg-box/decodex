@testable import DecodexApp
import Foundation
import XCTest

final class AccountUsageMeterPresentationTests: XCTestCase {
	@MainActor
	func testProgressIsClampedToTheMeterRange() {
		XCTAssertEqual(AccountUsageMeterView.normalizedProgress(for: nil), 0)
		XCTAssertEqual(AccountUsageMeterView.normalizedProgress(for: -1), 0)
		XCTAssertEqual(AccountUsageMeterView.normalizedProgress(for: 24), 0.24)
		XCTAssertEqual(AccountUsageMeterView.normalizedProgress(for: 100), 1)
		XCTAssertEqual(AccountUsageMeterView.normalizedProgress(for: 101), 1)
	}

	@MainActor
	func testOnlyAnExplicitResetCardEventStartsTheRefillAnimation() {
		let refill = AccountUsageMeterRefillAnimation(
			eventID: UUID(),
			fromPercent: 24
		)

		XCTAssertTrue(AccountUsageMeterView.shouldAnimateRefill(refill, to: 100))
		XCTAssertTrue(AccountUsageMeterView.shouldAnimateRefill(refill, to: 101))
		XCTAssertFalse(AccountUsageMeterView.shouldAnimateRefill(nil, to: 100))
		XCTAssertFalse(AccountUsageMeterView.shouldAnimateRefill(refill, to: 99))
	}

	func testResetCardEventTargetsOnlyMetersThatActuallyReachedFull() {
		let eventID = UUID()
		let refill = AccountUsageRefillAnimation(
			id: eventID,
			accountID: "account-a",
			primaryFromPercent: 24,
			secondaryFromPercent: 100
		)

		XCTAssertEqual(
			refill.meterAnimation(for: .primary, currentPercent: 100),
			AccountUsageMeterRefillAnimation(eventID: eventID, fromPercent: 24)
		)
		XCTAssertNil(refill.meterAnimation(for: .primary, currentPercent: 99))
		XCTAssertNil(refill.meterAnimation(for: .secondary, currentPercent: 100))
	}

	@MainActor
	func testRefillEventSurvivesSuccessfulRefreshAndClearsAfterFailure() {
		let store = AccountStore()
		let refill = AccountUsageRefillAnimation(
			accountID: "account-a",
			primaryFromPercent: 24,
			secondaryFromPercent: nil
		)

		store.beginUsageRefillAnimation(refill)
		store.finishUsageRefillAnimation(refill, refreshSucceeded: true)
		XCTAssertEqual(store.usageRefillAnimations["account-a"], refill)

		store.finishUsageRefillAnimation(refill, refreshSucceeded: false)
		XCTAssertNil(store.usageRefillAnimations["account-a"])
		store.cancelUsageRefillAnimation(for: "account-a")
	}
}
