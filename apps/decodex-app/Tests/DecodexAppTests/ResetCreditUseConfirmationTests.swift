@testable import DecodexApp
import XCTest

final class ResetCreditUseConfirmationTests: XCTestCase {
	func testFirstTapArmsImmediatelyAndSecondTapSubmitsTheSameAttempt() throws {
		let target = makeTarget(accountID: "account-a", expiresAt: 200)
		var confirmation = ResetCreditUseConfirmation()

		XCTAssertNil(
			confirmation.tap(target, makeIdempotencyKey: { "attempt-1" })
		)
		XCTAssertTrue(confirmation.isArmed(target))
		XCTAssertFalse(confirmation.isSubmitting)
		XCTAssertEqual(confirmation.armedAttempt?.idempotencyKey, "attempt-1")
		XCTAssertNil(confirmation.armedAttempt?.creditID)

		let attempt = try XCTUnwrap(
			confirmation.tap(target, makeIdempotencyKey: { "unexpected-key" })
		)

		XCTAssertEqual(attempt.target, target)
		XCTAssertEqual(attempt.idempotencyKey, "attempt-1")
		XCTAssertNil(attempt.creditID)
		XCTAssertTrue(confirmation.isSubmitting(target))
	}

	func testUnresolvedAttemptRetainsResolvedCreditAndIdempotencyKey() throws {
		let target = makeTarget(accountID: "account-a", expiresAt: 200)
		var confirmation = ResetCreditUseConfirmation()
		XCTAssertNil(
			confirmation.tap(target, makeIdempotencyKey: { "attempt-1" })
		)
		let firstAttempt = try XCTUnwrap(confirmation.tap(target))

		confirmation.finish(
			firstAttempt,
			completion: ResetCreditUseCompletion(resolved: false, creditID: "credit-1")
		)
		let retry = try XCTUnwrap(confirmation.tap(target))

		XCTAssertEqual(retry.creditID, "credit-1")
		XCTAssertEqual(retry.idempotencyKey, "attempt-1")
	}

	func testFailureBeforeResolutionRetainsTheOriginalAttempt() throws {
		let target = makeTarget(accountID: "account-a", expiresAt: 200)
		var confirmation = ResetCreditUseConfirmation()
		XCTAssertNil(
			confirmation.tap(target, makeIdempotencyKey: { "attempt-1" })
		)
		let firstAttempt = try XCTUnwrap(confirmation.tap(target))

		confirmation.finish(
			firstAttempt,
			completion: ResetCreditUseCompletion(resolved: false, creditID: nil)
		)
		let retry = try XCTUnwrap(confirmation.tap(target))

		XCTAssertNil(retry.creditID)
		XCTAssertEqual(retry.idempotencyKey, "attempt-1")
	}

	func testResolvedAttemptDisarmsTheCard() throws {
		let target = makeTarget(accountID: "account-a", expiresAt: 200)
		var confirmation = ResetCreditUseConfirmation()
		XCTAssertNil(confirmation.tap(target))
		let attempt = try XCTUnwrap(confirmation.tap(target))

		confirmation.finish(
			attempt,
			completion: ResetCreditUseCompletion(resolved: true, creditID: "credit-1")
		)

		XCTAssertFalse(confirmation.isArmed(target))
		XCTAssertFalse(confirmation.isSubmitting(target))
	}

	func testConfirmationWindowCanDisarmTheExactAttempt() throws {
		let target = makeTarget(accountID: "account-a", expiresAt: 200)
		var confirmation = ResetCreditUseConfirmation()
		XCTAssertNil(confirmation.tap(target))
		let attempt = try XCTUnwrap(confirmation.armedAttempt)

		XCTAssertTrue(confirmation.disarm(attempt))
		XCTAssertFalse(confirmation.isArmed(target))
	}

	func testStaleConfirmationWindowCannotDisarmANewerAttempt() throws {
		let first = makeTarget(accountID: "account-a", expiresAt: 200)
		let second = makeTarget(accountID: "account-a", expiresAt: 300)
		var confirmation = ResetCreditUseConfirmation()
		XCTAssertNil(
			confirmation.tap(first, makeIdempotencyKey: { "attempt-1" })
		)
		let staleAttempt = try XCTUnwrap(confirmation.armedAttempt)
		XCTAssertNil(
			confirmation.tap(second, makeIdempotencyKey: { "attempt-2" })
		)
		let currentAttempt = try XCTUnwrap(confirmation.armedAttempt)

		XCTAssertFalse(confirmation.disarm(staleAttempt))
		XCTAssertTrue(confirmation.isArmed(currentAttempt))
	}

	func testClosingThePanelCancelsConfirmation() {
		let target = makeTarget(accountID: "account-a", expiresAt: 200)
		var confirmation = ResetCreditUseConfirmation()
		XCTAssertNil(confirmation.tap(target))

		confirmation.cancelPendingConfirmation()

		XCTAssertFalse(confirmation.isArmed(target))
		XCTAssertFalse(confirmation.isSubmitting)
	}

	func testTappingAnotherCardImmediatelyMovesConfirmation() throws {
		let first = makeTarget(accountID: "account-a", expiresAt: 200)
		let second = makeTarget(accountID: "account-a", expiresAt: 300)
		var confirmation = ResetCreditUseConfirmation()
		XCTAssertNil(
			confirmation.tap(first, makeIdempotencyKey: { "attempt-1" })
		)

		XCTAssertNil(
			confirmation.tap(second, makeIdempotencyKey: { "attempt-2" })
		)

		XCTAssertFalse(confirmation.isArmed(first))
		XCTAssertTrue(confirmation.isArmed(second))
		XCTAssertEqual(confirmation.armedAttempt?.idempotencyKey, "attempt-2")
	}

	func testSubmittingAttemptIgnoresOtherCardTaps() throws {
		let first = makeTarget(accountID: "account-a", expiresAt: 200)
		let second = makeTarget(accountID: "account-a", expiresAt: 300)
		var confirmation = ResetCreditUseConfirmation()
		XCTAssertNil(confirmation.tap(first))
		let attempt = try XCTUnwrap(confirmation.tap(first))

		XCTAssertNil(confirmation.tap(second))
		XCTAssertTrue(confirmation.isSubmitting(first))
		XCTAssertTrue(confirmation.isArmed(attempt))
		XCTAssertFalse(confirmation.isArmed(second))
	}

	func testRemovedCardClearsConfirmation() {
		let target = makeTarget(accountID: "account-a", expiresAt: 200)
		var confirmation = ResetCreditUseConfirmation()
		XCTAssertNil(confirmation.tap(target))

		confirmation.retainOnly([])

		XCTAssertFalse(confirmation.isArmed(target))
		XCTAssertFalse(confirmation.isSubmitting)
	}

	func testSubmittingAttemptSurvivesTargetRemovalUntilItFinishes() throws {
		let target = makeTarget(accountID: "account-a", expiresAt: 200)
		var confirmation = ResetCreditUseConfirmation()
		XCTAssertNil(confirmation.tap(target))
		let attempt = try XCTUnwrap(confirmation.tap(target))

		confirmation.retainOnly([])

		XCTAssertTrue(confirmation.isArmed(attempt))
		XCTAssertTrue(confirmation.isSubmitting(target))

		confirmation.finish(
			attempt,
			completion: ResetCreditUseCompletion(resolved: true, creditID: "credit-1")
		)

		XCTAssertFalse(confirmation.isArmed(target))
		XCTAssertFalse(confirmation.isSubmitting)
	}

	func testTargetsDistinguishDuplicateCardsByOccurrence() {
		let duplicate = AccountResetCredit(
			grantedAtUnixEpoch: 100,
			expiresAtUnixEpoch: 200,
			status: "available"
		)
		let targets = ResetCreditUseTarget.makeTargets(
			accountID: "account-a",
			reportedAvailableCount: 2,
			credits: [duplicate, duplicate]
		)

		XCTAssertEqual(targets.map(\.occurrence), [0, 1])
		XCTAssertEqual(targets.map(\.descriptorMultiplicity), [2, 2])
		XCTAssertTrue(targets.allSatisfy(\.detailsComplete))
		XCTAssertEqual(Set(targets).count, 2)
	}

	private func makeTarget(
		accountID: String,
		expiresAt: Int
	) -> ResetCreditUseTarget {
		ResetCreditUseTarget(
			accountID: accountID,
			descriptor: ResetCreditDescriptor(
				credit: AccountResetCredit(
					grantedAtUnixEpoch: 100,
					expiresAtUnixEpoch: expiresAt,
					status: "available"
				)
			),
			occurrence: 0,
			descriptorMultiplicity: 1,
			detailsComplete: true
		)
	}
}
