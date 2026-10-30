@testable import DecodexApp
import XCTest

final class SecondTapConfirmationTests: XCTestCase {
	func testOnlyTheSecondTapInsideTheWindowConfirms() {
		let start = ContinuousClock.now
		var confirmation = SecondTapConfirmation()
		XCTAssertFalse(confirmation.tap(at: start))
		XCTAssertTrue(confirmation.isArmed(at: start))
		XCTAssertTrue(confirmation.tap(at: start.advanced(by: .seconds(1))))
		XCTAssertNil(confirmation.deadline)
	}

	func testExpiredTapStartsANewConfirmation() {
		let start = ContinuousClock.now
		var confirmation = SecondTapConfirmation()
		XCTAssertFalse(confirmation.tap(at: start))
		XCTAssertFalse(confirmation.isArmed(at: start.advanced(by: .seconds(5))))
		XCTAssertFalse(confirmation.tap(at: start.advanced(by: .seconds(5))))
		XCTAssertTrue(confirmation.tap(at: start.advanced(by: .seconds(6))))
	}

	func testCancellationRequiresTwoNewTaps() {
		let start = ContinuousClock.now
		var confirmation = SecondTapConfirmation()
		XCTAssertFalse(confirmation.tap(at: start))
		confirmation.cancel()
		XCTAssertFalse(confirmation.isArmed(at: start))
		XCTAssertFalse(confirmation.tap(at: start.advanced(by: .seconds(1))))
	}
}
