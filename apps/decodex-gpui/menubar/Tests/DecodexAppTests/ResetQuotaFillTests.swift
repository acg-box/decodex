import Foundation
import XCTest
@testable import DecodexApp

final class ResetQuotaFillTests: XCTestCase {
	func testSynchronizedFillAndFreshObservationHandoff() throws {
		let start = Date(timeIntervalSince1970: 100)
		let quota = ResetCardQuotaWindow(durationMinutes: 300, observedAtUnixMicros: 1,
			state: .current(usedPercent: 64, resetsAtUnixMicros: 200_000_000))
		let attempt = ResetCardUseAttempt(target: ResetCardUseTarget(
			authority: ResetCardAuthority(profileName: "fake", serverID: "fake"),
			accountID: "fake", expectedRevision: 1,
			descriptor: try ResetCardDescriptor(grantedAtUnixSeconds: 1, expiresAtUnixSeconds: 2)),
			idempotencyKey: "fake")
		let fill = ResetQuotaFill(attempt: attempt, initial: [quota], started: start)
		XCTAssertEqual(fill.remaining(for: quota, at: start), 36)
		let midpoint = try XCTUnwrap(fill.remaining(for: quota, at: start.addingTimeInterval(0.425)))
		XCTAssertGreaterThan(midpoint, 36)
		XCTAssertLessThan(midpoint, 100)
		XCTAssertEqual(fill.remaining(for: quota, at: start.addingTimeInterval(1)), 100)
		XCTAssertEqual(fill.remaining(for: quota, at: start, reduceMotion: true), 100)
		let fresh = ResetCardQuotaWindow(durationMinutes: 300, observedAtUnixMicros: 101_000_000,
			state: .current(usedPercent: 1, resetsAtUnixMicros: 200_000_000))
		XCTAssertEqual(fill.remaining(for: fresh, at: start), 36)
		XCTAssertNil(fill.remaining(for: fresh, at: start.addingTimeInterval(1)))
		XCTAssertNil(fill.remaining(for: .unknown(durationMinutes: 10_080), at: start))
	}
}
