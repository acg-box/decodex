@testable import DecodexApp
import XCTest

final class ResetCardRefreshCoordinatorTests: XCTestCase {
	func testObservationGenerationsSurviveUntilAFullReadConsumesThem() {
		var requests = ResetCardRefreshRequests()
		requests.requestObservationRefresh()
		requests.requestObservationRefresh()

		XCTAssertTrue(requests.hasFullRefreshRequest)
		XCTAssertEqual(requests.takeNext(), .full)
		XCTAssertFalse(requests.hasFullRefreshRequest)
		XCTAssertFalse(requests.hasWork)
	}

	func testSkeletonWorkWaitsBehindAFullRefresh() {
		var requests = ResetCardRefreshRequests()
		requests.requestSkeletonRefresh()
		requests.requestObservationRefresh()

		XCTAssertEqual(requests.takeNext(), .full)
		XCTAssertEqual(requests.takeNext(), .skeleton)
		XCTAssertNil(requests.takeNext())
	}

	func testResetDiscardsAllPendingWork() {
		var requests = ResetCardRefreshRequests()
		requests.requestManualRefresh()
		requests.requestObservationRefresh()
		requests.requestSkeletonRefresh()

		requests.reset()

		XCTAssertFalse(requests.hasWork)
		XCTAssertNil(requests.takeNext())
	}
}
