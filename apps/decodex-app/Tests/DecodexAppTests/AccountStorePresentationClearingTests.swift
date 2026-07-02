@testable import DecodexApp
import XCTest

final class AccountStorePresentationClearingTests: XCTestCase {
	@MainActor
	func testFullSnapshotClearsStaleLiveRunActivityOverlay() throws {
		let store = AccountStore()

		try store.applyOperatorDashboardEvent(dashboardEvent(
			type: "snapshot",
			payload: """
			{
				  "snapshotPublishedAtUnixEpoch": 20,
				  "snapshot": {
				    "current_lanes": [],
				    "presentation": {
				      "current_lane_cards": []
				    }
				  }
				}
			"""
		))
		try store.applyOperatorDashboardEvent(dashboardEvent(
			type: "runActivity",
				payload: """
				{
				  "emittedAtUnixEpoch": 30,
				  "presentation": {
				    "current_lane_cards": [
				      {
				        "id": "run-live",
				        "run_id": "run-live",
				        "title": "XY-672",
				        "detail": "agent_run",
				        "tone": "running",
				        "counts_as_running": true,
				        "needs_attention": false,
				        "is_waiting": false,
				        "assigned_account_fingerprints": [],
				        "assigned_account_emails": [],
				        "run": {
				          "run_id": "run-live",
				          "issue_identifier": "XY-672"
				        }
				      }
				    ]
				  }
				}
			"""
		))
		try store.applyOperatorDashboardEvent(dashboardEvent(
			type: "snapshot",
			payload: """
			{
				  "snapshotPublishedAtUnixEpoch": 40,
				  "snapshot": {
				    "current_lanes": [],
				    "presentation": {
				      "current_lane_cards": []
				    }
				  }
				}
			"""
		))

		XCTAssertTrue(store.operatorPresentation?.currentLaneCards.isEmpty ?? false)
	}

	@MainActor
	func testCompleteEmptyRunActivityClearsLiveRuns() throws {
		let store = AccountStore()

		try store.applyOperatorDashboardEvent(dashboardEvent(
			type: "snapshot",
			payload: """
			{
				  "snapshotPublishedAtUnixEpoch": 20,
				  "snapshot": {
				    "current_lanes": [
				      {
				        "run_id": "run-live",
				        "issue_identifier": "XY-672"
				      }
				    ],
				    "presentation": {
				      "current_lane_cards": [
				        {
				          "id": "run-live",
				          "run_id": "run-live",
				          "title": "XY-672",
				          "detail": "agent_run",
				          "tone": "running",
				          "counts_as_running": true,
				          "needs_attention": false,
				          "is_waiting": false,
				          "assigned_account_fingerprints": [],
				          "assigned_account_emails": [],
				          "run": {
				            "run_id": "run-live",
				            "issue_identifier": "XY-672"
				          }
				        }
				      ]
				    }
				  }
				}
			"""
		))
		try store.applyOperatorDashboardEvent(dashboardEvent(
			type: "runActivity",
				payload: """
				{
				  "emittedAtUnixEpoch": 30,
				  "presentation": {
				    "current_lane_cards": []
				  }
				}
			"""
		))

		XCTAssertEqual(store.operatorSnapshot?.currentLanes.map(\.runID), ["run-live"])
		XCTAssertTrue(store.operatorPresentation?.currentLaneCards.isEmpty ?? false)
	}

	@MainActor
	func testCompleteEmptyRunActivityDoesNotKeepClearingNewSnapshots() throws {
		let store = AccountStore()

		try store.applyOperatorDashboardEvent(dashboardEvent(
			type: "snapshot",
			payload: """
			{
				  "snapshotPublishedAtUnixEpoch": 20,
				  "snapshot": {
				    "current_lanes": [
				      {
				        "run_id": "run-live",
				        "issue_identifier": "XY-672"
				      }
				    ],
				    "presentation": {
				      "current_lane_cards": [
				        {
				          "id": "run-live",
				          "run_id": "run-live",
				          "title": "XY-672",
				          "detail": "agent_run",
				          "tone": "running",
				          "counts_as_running": true,
				          "needs_attention": false,
				          "is_waiting": false,
				          "assigned_account_fingerprints": [],
				          "assigned_account_emails": [],
				          "run": {
				            "run_id": "run-live",
				            "issue_identifier": "XY-672"
				          }
				        }
				      ]
				    }
				  }
				}
			"""
		))
		try store.applyOperatorDashboardEvent(dashboardEvent(
			type: "runActivity",
				payload: """
				{
				  "emittedAtUnixEpoch": 30,
				  "presentation": {
				    "current_lane_cards": []
				  }
				}
			"""
		))
		XCTAssertTrue(store.operatorPresentation?.currentLaneCards.isEmpty ?? false)

		try store.applyOperatorDashboardEvent(dashboardEvent(
			type: "snapshot",
			payload: """
			{
				  "snapshotPublishedAtUnixEpoch": 40,
				  "snapshot": {
				    "current_lanes": [
				      {
				        "run_id": "run-returned",
				        "issue_identifier": "XY-934"
				      }
				    ],
				    "presentation": {
				      "current_lane_cards": [
				        {
				          "id": "run-returned",
				          "run_id": "run-returned",
				          "title": "XY-934",
				          "detail": "agent_run",
				          "tone": "running",
				          "counts_as_running": true,
				          "needs_attention": false,
				          "is_waiting": false,
				          "assigned_account_fingerprints": [],
				          "assigned_account_emails": [],
				          "run": {
				            "run_id": "run-returned",
				            "issue_identifier": "XY-934"
				          }
				        }
				      ]
				    }
				  }
				}
			"""
		))

		XCTAssertEqual(store.operatorPresentation?.currentLaneCards.map(\.runID), ["run-returned"])
	}}
