@testable import DecodexApp
import XCTest

final class AccountNoticePresentationTests: XCTestCase {
	func testResetCreditOutcomesUseConciseSemanticNotices() {
		let reset = AccountNotice.resetCreditOutcome(.reset)
		XCTAssertEqual(reset.tone, .success)
		XCTAssertEqual(reset.summary, "Usage restored")
		XCTAssertNil(reset.details)

		let alreadyUsed = AccountNotice.resetCreditOutcome(.alreadyRedeemed)
		XCTAssertEqual(alreadyUsed.tone, .information)
		XCTAssertEqual(alreadyUsed.summary, "Card already used")

		let nothingToReset = AccountNotice.resetCreditOutcome(.nothingToReset)
		XCTAssertEqual(nothingToReset.tone, .information)
		XCTAssertEqual(nothingToReset.summary, "Nothing to reset")

		let noCredit = AccountNotice.resetCreditOutcome(.noCredit)
		XCTAssertEqual(noCredit.tone, .information)
		XCTAssertEqual(noCredit.summary, "No reset card available")
	}

	func testResetSuccessWithRefreshFailurePreservesBothFacts() {
		let notice = AccountNotice.resetCreditOutcome(
			.reset,
			refreshError: "The account service did not respond."
		)

		XCTAssertEqual(notice.tone, .error)
		XCTAssertEqual(notice.summary, "Usage restored; refresh failed")
		XCTAssertEqual(notice.details, "The account service did not respond.")
		XCTAssertEqual(notice.copyText, "The account service did not respond.")
	}

	func testErrorSeparatesReadableSummaryFromCopyableDetails() {
		let notice = AccountNotice.error(
			"Couldn’t refresh accounts",
			details: "Server request failed (503): unavailable"
		)

		XCTAssertEqual(notice.summary, "Couldn’t refresh accounts")
		XCTAssertEqual(notice.copyText, "Server request failed (503): unavailable")
		XCTAssertNil(notice.automaticDismissalDelay)
	}

	@MainActor
	func testLoginStatusTreatsOnlySignInErrorsAsFailure() {
		let store = AccountStore()

		store.presentNotice(.information("Nothing to reset"))
		XCTAssertEqual(store.loginStatusLabel, "Ready")
		XCTAssertEqual(store.notice?.summary, "Nothing to reset")

		store.presentNotice(.error(
			"Couldn’t refresh fast mode",
			details: "Network unavailable"
		))
		XCTAssertEqual(store.loginStatusLabel, "Ready")
		XCTAssertNil(store.loginNotice)
		XCTAssertEqual(store.notice?.summary, "Couldn’t refresh fast mode")

		store.presentNotice(.error(
			"Sign-in failed",
			details: "Network unavailable",
			scope: .signIn
		))
		XCTAssertEqual(store.loginStatusLabel, "Login failed")
		XCTAssertEqual(store.loginNotice?.summary, "Sign-in failed")
		XCTAssertEqual(store.notice?.summary, "Couldn’t refresh fast mode")

		store.clearNotice()
		XCTAssertEqual(store.loginStatusLabel, "Login failed")
		store.clearLoginNotice()
	}

	@MainActor
	func testSourceSpecificClearingPreservesUnrelatedNotice() {
		let store = AccountStore()
		store.presentNotice(.resetCreditOutcome(.reset))

		store.clearNotice(source: .accountRefresh)
		XCTAssertEqual(store.notice?.summary, "Usage restored")

		store.clearNotice(source: .resetCredit)
		XCTAssertNil(store.notice)
	}

	@MainActor
	func testRepeatedIdenticalErrorsKeepTheVisibleNoticeIdentity() {
		let store = AccountStore()
		store.presentNotice(.error(
			"Couldn’t refresh fast mode",
			details: "Service unavailable",
			source: .fastMode
		))
		let firstID = store.notice?.id

		store.presentNotice(.error(
			"Couldn’t refresh fast mode",
			details: "Service unavailable",
			source: .fastMode
		))

		XCTAssertEqual(store.notice?.id, firstID)
	}
}
