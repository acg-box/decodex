@testable import DecodexApp
import XCTest

final class ResetCreditUseConfirmationTests: XCTestCase {
	func testFirstTapPreparesAndSecondTapConsumesTheExactArmedCredit() throws {
		let target = makeTarget(accountID: "account-a", expiresAt: 200)
		var confirmation = ResetCreditUseConfirmation()

		let preparation = try unwrapPreparation(
			confirmation.tap(target, makeIdempotencyKey: { "attempt-1" })
		)
		XCTAssertTrue(confirmation.isPreparing(target))
		XCTAssertFalse(confirmation.isArmed(target))

		confirmation.finishPreparation(preparation, creditID: "credit-1")
		XCTAssertFalse(confirmation.isPreparing(target))
		XCTAssertTrue(confirmation.isArmed(target))

		let attempt = try unwrapAttempt(
			confirmation.tap(target, makeIdempotencyKey: { "unexpected-key" })
		)
		XCTAssertEqual(attempt.creditID, "credit-1")
		XCTAssertEqual(attempt.idempotencyKey, "attempt-1")
		XCTAssertTrue(confirmation.isSubmitting(target))
	}

	func testFailedPreparationDisarmsTheCard() throws {
		let target = makeTarget(accountID: "account-a", expiresAt: 200)
		var confirmation = ResetCreditUseConfirmation()

		let preparation = try unwrapPreparation(confirmation.tap(target))
		confirmation.finishPreparation(preparation, creditID: nil)

		XCTAssertFalse(confirmation.isPreparing(target))
		XCTAssertFalse(confirmation.isArmed(target))
		XCTAssertFalse(confirmation.isBusy)
	}

	func testUnresolvedConsumeRetainsExactCreditAndIdempotencyKey() throws {
		let target = makeTarget(accountID: "account-a", expiresAt: 200)
		var confirmation = ResetCreditUseConfirmation()
		let preparation = try unwrapPreparation(
			confirmation.tap(target, makeIdempotencyKey: { "attempt-1" })
		)
		confirmation.finishPreparation(preparation, creditID: "credit-1")

		let firstAttempt = try unwrapAttempt(confirmation.tap(target))
		confirmation.finish(firstAttempt, resolved: false)
		let retry = try unwrapAttempt(confirmation.tap(target))

		XCTAssertEqual(retry.creditID, "credit-1")
		XCTAssertEqual(retry.idempotencyKey, "attempt-1")
	}

	func testResolvedAttemptDisarmsTheCard() throws {
		let target = makeTarget(accountID: "account-a", expiresAt: 200)
		var confirmation = ResetCreditUseConfirmation()
		let preparation = try unwrapPreparation(confirmation.tap(target))
		confirmation.finishPreparation(preparation, creditID: "credit-1")

		let attempt = try unwrapAttempt(confirmation.tap(target))
		confirmation.finish(attempt, resolved: true)

		XCTAssertFalse(confirmation.isArmed(target))
		XCTAssertFalse(confirmation.isSubmitting(target))
	}

	func testConfirmationWindowCanDisarmTheExactAttempt() throws {
		let target = makeTarget(accountID: "account-a", expiresAt: 200)
		var confirmation = ResetCreditUseConfirmation()
		let preparation = try unwrapPreparation(confirmation.tap(target))
		let attempt = try XCTUnwrap(
			confirmation.finishPreparation(preparation, creditID: "credit-1")
		)

		XCTAssertTrue(confirmation.disarm(attempt))
		XCTAssertFalse(confirmation.isArmed(target))
	}

	func testStaleConfirmationWindowCannotDisarmANewerAttempt() throws {
		let first = makeTarget(accountID: "account-a", expiresAt: 200)
		let second = makeTarget(accountID: "account-a", expiresAt: 300)
		var confirmation = ResetCreditUseConfirmation()
		let firstPreparation = try unwrapPreparation(confirmation.tap(first))
		let staleAttempt = try XCTUnwrap(
			confirmation.finishPreparation(firstPreparation, creditID: "credit-1")
		)
		let secondPreparation = try unwrapPreparation(confirmation.tap(second))
		let currentAttempt = try XCTUnwrap(
			confirmation.finishPreparation(secondPreparation, creditID: "credit-2")
		)

		XCTAssertFalse(confirmation.disarm(staleAttempt))
		XCTAssertTrue(confirmation.isArmed(currentAttempt))
	}

	func testClosingThePanelCancelsPreparationAndPreventsLateArming() throws {
		let target = makeTarget(accountID: "account-a", expiresAt: 200)
		var confirmation = ResetCreditUseConfirmation()
		let preparation = try unwrapPreparation(confirmation.tap(target))

		confirmation.cancelPendingConfirmation()
		let attempt = confirmation.finishPreparation(preparation, creditID: "credit-1")

		XCTAssertNil(attempt)
		XCTAssertFalse(confirmation.isPreparing(target))
		XCTAssertFalse(confirmation.isArmed(target))
	}

	func testTappingAnotherCardStartsASeparatePreparation() throws {
		let first = makeTarget(accountID: "account-a", expiresAt: 200)
		let second = makeTarget(accountID: "account-a", expiresAt: 300)
		var confirmation = ResetCreditUseConfirmation()
		let firstPreparation = try unwrapPreparation(
			confirmation.tap(first, makeIdempotencyKey: { "attempt-1" })
		)
		confirmation.finishPreparation(firstPreparation, creditID: "credit-1")

		let secondPreparation = try unwrapPreparation(
			confirmation.tap(second, makeIdempotencyKey: { "attempt-2" })
		)

		XCTAssertFalse(confirmation.isArmed(first))
		XCTAssertTrue(confirmation.isPreparing(second))
		XCTAssertEqual(secondPreparation.idempotencyKey, "attempt-2")
	}

	func testRemovedCardClearsPreparationAndConfirmation() throws {
		let target = makeTarget(accountID: "account-a", expiresAt: 200)
		var preparing = ResetCreditUseConfirmation()
		_ = try unwrapPreparation(preparing.tap(target))
		preparing.retainOnly([])
		XCTAssertFalse(preparing.isBusy)

		var armed = ResetCreditUseConfirmation()
		let preparation = try unwrapPreparation(armed.tap(target))
		armed.finishPreparation(preparation, creditID: "credit-1")
		armed.retainOnly([])
		XCTAssertFalse(armed.isArmed(target))
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

	private func unwrapPreparation(
		_ action: ResetCreditUseAction?,
		file: StaticString = #filePath,
		line: UInt = #line
	) throws -> ResetCreditUsePreparation {
		guard case .prepare(let preparation) = try XCTUnwrap(action, file: file, line: line) else {
			XCTFail("Expected a prepare action.", file: file, line: line)
			throw TestFailure.unexpectedAction
		}

		return preparation
	}

	private func unwrapAttempt(
		_ action: ResetCreditUseAction?,
		file: StaticString = #filePath,
		line: UInt = #line
	) throws -> ResetCreditUseAttempt {
		guard case .consume(let attempt) = try XCTUnwrap(action, file: file, line: line) else {
			XCTFail("Expected a consume action.", file: file, line: line)
			throw TestFailure.unexpectedAction
		}

		return attempt
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

	private enum TestFailure: Error {
		case unexpectedAction
	}
}
