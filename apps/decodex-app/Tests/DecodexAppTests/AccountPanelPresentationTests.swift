@testable import DecodexApp
import XCTest

final class AccountPanelPresentationTests: XCTestCase {
	func testUnsupportedQuotaWindowIsMutedInsteadOfDestructive() {
		let presentation = ResetCardQuotaPresentation(
			window: ResetCardQuotaWindow(
				durationMinutes: 300,
				observedAtUnixMicros: 1_000_000,
				state: .error(.unsupportedWindow)
			)
		)

		XCTAssertEqual(presentation.valueText, "—")
		XCTAssertEqual(presentation.detailText, "Not reported")
		XCTAssertEqual(presentation.tone, .muted)
		XCTAssertNil(presentation.usedPercent)
		XCTAssertNil(presentation.resetDate)
	}

	func testProtocolFailureRemainsDestructive() {
		let presentation = ResetCardQuotaPresentation(
			window: ResetCardQuotaWindow(
				durationMinutes: 10_080,
				observedAtUnixMicros: 1_000_000,
				state: .error(.protocolUnavailable)
			)
		)

		XCTAssertEqual(presentation.valueText, "Error")
		XCTAssertEqual(presentation.detailText, "Invalid provider response")
		XCTAssertEqual(presentation.tone, .error)
	}

	func testCurrentQuotaRetainsValueAndResetDate() {
		let presentation = ResetCardQuotaPresentation(
			window: ResetCardQuotaWindow(
				durationMinutes: 10_080,
				observedAtUnixMicros: 1_000_000,
				state: .current(
					usedPercent: 79,
					resetsAtUnixMicros: 2_000_000
				)
			)
		)

		XCTAssertEqual(presentation.valueText, "79% used")
		XCTAssertEqual(presentation.tone, .current)
		XCTAssertEqual(presentation.usedPercent, 79)
		XCTAssertNotNil(presentation.resetDate)
	}

	func testStaleQuotaHasANonColorStatusMarker() {
		let presentation = ResetCardQuotaPresentation(
			window: ResetCardQuotaWindow(
				durationMinutes: 300,
				observedAtUnixMicros: 1_000_000,
				state: .stale(
					usedPercent: 42,
					resetsAtUnixMicros: 2_000_000
				)
			)
		)

		XCTAssertEqual(presentation.valueText, "42% stale")
		XCTAssertNil(presentation.detailText)
		XCTAssertEqual(presentation.tone, .warning)
		XCTAssertEqual(presentation.usedPercent, 42)
		XCTAssertNotNil(presentation.resetDate)
	}
}
