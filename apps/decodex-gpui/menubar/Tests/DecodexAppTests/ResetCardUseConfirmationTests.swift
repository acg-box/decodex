@testable import DecodexApp
import XCTest

final class ResetCardUseConfirmationTests: XCTestCase {
	func testFirstTapArmsImmediatelyAndSecondTapSubmitsTheSameAttempt() throws {
		let target = try makeTarget(expiresAt: 200)
		var confirmation = ResetCardUseConfirmation()

		XCTAssertNil(
			confirmation.tap(target, makeIdempotencyKey: { "attempt-1" })
		)
		XCTAssertTrue(confirmation.isArmed(target))
		XCTAssertFalse(confirmation.isSubmitting)
		XCTAssertEqual(confirmation.armedAttempt?.idempotencyKey, "attempt-1")

		let attempt = try XCTUnwrap(
			confirmation.tap(target, makeIdempotencyKey: { "unexpected-key" })
		)

		XCTAssertEqual(attempt.target, target)
		XCTAssertEqual(attempt.idempotencyKey, "attempt-1")
		XCTAssertTrue(confirmation.isSubmitting(target))
	}

	func testSubmittedAttemptDisarmsWhileDurableStatusRemainsPending() throws {
		let target = try makeTarget(expiresAt: 200)
		var confirmation = ResetCardUseConfirmation()
		XCTAssertNil(
			confirmation.tap(target, makeIdempotencyKey: { "attempt-1" })
		)
		let attempt = try XCTUnwrap(confirmation.tap(target))

		confirmation.finish(
			attempt,
			completion: ResetCardUseCompletion(resolved: false)
		)

		XCTAssertFalse(confirmation.isArmed(target))
		XCTAssertFalse(confirmation.isSubmitting)
	}

	func testResolvedAttemptDisarmsTheCard() throws {
		let target = try makeTarget(expiresAt: 200)
		var confirmation = ResetCardUseConfirmation()
		XCTAssertNil(confirmation.tap(target))
		let attempt = try XCTUnwrap(confirmation.tap(target))

		confirmation.finish(
			attempt,
			completion: ResetCardUseCompletion(resolved: true)
		)

		XCTAssertFalse(confirmation.isArmed(target))
		XCTAssertFalse(confirmation.isSubmitting(target))
	}

	func testConfirmationWindowCanDisarmTheExactAttempt() throws {
		let target = try makeTarget(expiresAt: 200)
		var confirmation = ResetCardUseConfirmation()
		XCTAssertNil(confirmation.tap(target))
		let attempt = try XCTUnwrap(confirmation.armedAttempt)

		XCTAssertTrue(confirmation.disarm(attempt))
		XCTAssertFalse(confirmation.isArmed(target))
	}

	func testStaleConfirmationCannotDisarmANewerAttempt() throws {
		let first = try makeTarget(expiresAt: 200)
		let second = try makeTarget(expiresAt: 300)
		var confirmation = ResetCardUseConfirmation()
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

	func testClosingThePanelCancelsConfirmation() throws {
		let target = try makeTarget(expiresAt: 200)
		var confirmation = ResetCardUseConfirmation()
		XCTAssertNil(confirmation.tap(target))

		confirmation.cancelPendingConfirmation()

		XCTAssertFalse(confirmation.isArmed(target))
		XCTAssertFalse(confirmation.isSubmitting)
	}

	func testTappingAnotherCardMovesConfirmation() throws {
		let first = try makeTarget(expiresAt: 200)
		let second = try makeTarget(expiresAt: 300)
		var confirmation = ResetCardUseConfirmation()
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
		let first = try makeTarget(expiresAt: 200)
		let second = try makeTarget(expiresAt: 300)
		var confirmation = ResetCardUseConfirmation()
		XCTAssertNil(confirmation.tap(first))
		let attempt = try XCTUnwrap(confirmation.tap(first))

		XCTAssertNil(confirmation.tap(second))
		XCTAssertTrue(confirmation.isSubmitting(first))
		XCTAssertTrue(confirmation.isArmed(attempt))
		XCTAssertFalse(confirmation.isArmed(second))
	}

	func testRemovedCardClearsConfirmation() throws {
		let target = try makeTarget(expiresAt: 200)
		var confirmation = ResetCardUseConfirmation()
		XCTAssertNil(confirmation.tap(target))

		confirmation.retainOnly([])

		XCTAssertFalse(confirmation.isArmed(target))
		XCTAssertFalse(confirmation.isSubmitting)
	}

	func testSubmittingAttemptSurvivesTargetRemovalUntilItFinishes() throws {
		let target = try makeTarget(expiresAt: 200)
		var confirmation = ResetCardUseConfirmation()
		XCTAssertNil(confirmation.tap(target))
		let attempt = try XCTUnwrap(confirmation.tap(target))

		confirmation.retainOnly([])

		XCTAssertTrue(confirmation.isArmed(attempt))
		XCTAssertTrue(confirmation.isSubmitting(target))

		confirmation.finish(
			attempt,
			completion: ResetCardUseCompletion(resolved: true)
		)

		XCTAssertFalse(confirmation.isArmed(target))
		XCTAssertFalse(confirmation.isSubmitting)
	}

	func testRevisionIsPartOfThePublicTargetIdentity() throws {
		let revisionOne = try makeTarget(expiresAt: 200, revision: 1)
		let revisionTwo = try makeTarget(expiresAt: 200, revision: 2)

		XCTAssertNotEqual(revisionOne, revisionTwo)
	}

	func testPendingCardStaysVisibleDuringInventoryRefreshWithoutDuplication() throws {
		let target = try makeTarget(expiresAt: 200)
		let attempt = ResetCardUseAttempt(target: target, idempotencyKey: "pending")
		XCTAssertEqual(ResetCardChipPresentation.targets(inventory: [], pending: [attempt],
			completed: nil, dismissedKey: nil), [target])
		let refreshed = try makeTarget(expiresAt: 200, revision: 8)
		XCTAssertEqual(ResetCardChipPresentation.targets(inventory: [refreshed], pending: [attempt],
			completed: nil, dismissedKey: nil), [refreshed])
	}

	func testConfirmedCardExitsWithoutReturningFromStaleInventory() throws {
		let used = try makeTarget(expiresAt: 200)
		let other = try makeTarget(expiresAt: 300)
		let attempt = ResetCardUseAttempt(target: used, idempotencyKey: "confirmed")
		XCTAssertEqual(ResetCardChipPresentation.targets(inventory: [other], pending: [],
			completed: attempt, dismissedKey: nil), [used, other])
		XCTAssertEqual(ResetCardChipPresentation.targets(inventory: [used, other], pending: [],
			completed: attempt, dismissedKey: "confirmed"), [other])
		// An older completion must not hide a newer confirmed card before its exit.
		XCTAssertEqual(ResetCardChipPresentation.targets(inventory: [used], pending: [],
			completed: attempt, dismissedKey: "older"), [used])
	}

	func testFailedRequestLeavesInventoryCardAvailable() throws {
		let target = try makeTarget(expiresAt: 200)
		XCTAssertEqual(ResetCardChipPresentation.targets(inventory: [target], pending: [],
			completed: nil, dismissedKey: nil), [target])
	}

	private func makeTarget(
		expiresAt: Int64,
		revision: UInt64 = 7
	) throws -> ResetCardUseTarget {
		ResetCardUseTarget(
			authority: ResetCardAuthority(
				profileName: "local",
				serverID: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"
			),
			accountID: "11111111-1111-4111-8111-111111111111",
			expectedRevision: revision,
			descriptor: try ResetCardDescriptor(
				grantedAtUnixSeconds: 100,
				expiresAtUnixSeconds: expiresAt
			)
		)
	}
}
