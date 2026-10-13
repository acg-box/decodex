import XCTest
@testable import DecodexApp

final class UpdateHostTests: XCTestCase {
    @MainActor
    func testUnconfiguredDevelopmentHostDoesNotStartAnUpdater() {
        XCTAssertNil(UpdateHost())
        XCTAssertNil(decodexUpdateCreate())
    }

    func testAbsentHostCannotStartAnUpdate() {
        XCTAssertFalse(decodexUpdateCheck(nil))
        decodexUpdateDestroy(nil)
    }
}
