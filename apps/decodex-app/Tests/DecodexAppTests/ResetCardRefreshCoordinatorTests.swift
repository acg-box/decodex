@testable import DecodexApp
import XCTest

final class ResetCardRefreshCoordinatorTests: XCTestCase {
	func testObservationGenerationsSurviveUntilAFullReadConsumesThem() {
		var requests = ResetCardRefreshRequests()
		requests.requestObservationRefresh()
		requests.requestObservationRefresh()

		XCTAssertTrue(requests.hasObservationRefreshRequest)
		XCTAssertEqual(requests.takeNext(), .observation)
		XCTAssertFalse(requests.hasObservationRefreshRequest)
		XCTAssertFalse(requests.hasWork)
	}

	func testSkeletonWorkWaitsBehindAnObservationRefresh() {
		var requests = ResetCardRefreshRequests()
		requests.requestSkeletonRefresh()
		requests.requestObservationRefresh()

		XCTAssertEqual(requests.takeNext(), .observation)
		XCTAssertEqual(requests.takeNext(), .skeleton)
		XCTAssertNil(requests.takeNext())
	}

	func testManualRefreshSubsumesPendingObservation() {
		var requests = ResetCardRefreshRequests()
		requests.requestObservationRefresh()
		requests.requestManualRefresh()

		XCTAssertEqual(requests.takeNext(), .full)
		XCTAssertFalse(requests.hasWork)
	}

	func testManualRefreshCompletionDoesNotWaitForFollowUpWork() {
		var requests = ResetCardRefreshRequests()
		let firstGeneration = requests.requestManualRefresh()
		let secondGeneration = requests.requestManualRefresh()

		XCTAssertFalse(requests.didCompleteManualRefresh(upTo: firstGeneration))
		XCTAssertEqual(requests.takeNext(), .full)
		requests.completeManualRefresh()

		XCTAssertTrue(requests.didCompleteManualRefresh(upTo: firstGeneration))
		XCTAssertTrue(requests.didCompleteManualRefresh(upTo: secondGeneration))
		XCTAssertFalse(requests.hasWork)
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
