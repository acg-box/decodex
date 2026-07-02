@testable import DecodexApp
import XCTest

final class AccountStorePresentationRecencyTests: XCTestCase {
	@MainActor
	func testOlderRunActivityDoesNotOverrideNewerSnapshotPresentation() throws {
		let store = AccountStore()

		try store.applyOperatorDashboardEvent(dashboardEvent(
			type: "snapshot",
			payload: """
			{
				  "snapshotPublishedAtUnixEpoch": 20,
				  "snapshot": {
				    "current_lanes": [
				      {
				        "run_id": "run-new",
				        "issue_identifier": "XY-672"
				      }
				    ],
				    "presentation": {
				      "current_lane_cards": [
				        {
				          "id": "run-new",
				          "run_id": "run-new",
				          "title": "XY-672",
				          "detail": "agent_run",
				          "tone": "running",
				          "counts_as_running": true,
				          "needs_attention": false,
				          "is_waiting": false,
				          "assigned_account_fingerprints": [],
				          "assigned_account_emails": [],
				          "run": {
				            "run_id": "run-new",
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
				  "emittedAtUnixEpoch": 10,
				  "presentation": {
				    "current_lane_cards": [
				      {
				        "id": "run-old",
				        "run_id": "run-old",
				        "title": "PUB-1147",
				        "detail": "agent_run",
				        "tone": "running",
				        "counts_as_running": true,
				        "needs_attention": false,
				        "is_waiting": false,
				        "assigned_account_fingerprints": [],
				        "assigned_account_emails": [],
				        "run": {
				          "run_id": "run-old",
				          "issue_identifier": "PUB-1147"
				        }
				      }
				    ]
				  }
				}
			"""
		))

		XCTAssertEqual(store.operatorSnapshot?.currentLanes.map(\.runID), ["run-new"])
		XCTAssertEqual(store.operatorPresentation?.currentLaneCards.map(\.runID), ["run-new"])
	}

	@MainActor
	func testNewerRunActivityOverridesSnapshotPresentation() throws {
		let store = AccountStore()

		try store.applyOperatorDashboardEvent(dashboardEvent(
			type: "snapshot",
			payload: """
			{
				  "snapshotPublishedAtUnixEpoch": 20,
				  "snapshot": {
				    "current_lanes": [
				      {
				        "run_id": "run-old",
				        "issue_identifier": "XY-672"
				      }
				    ],
				    "presentation": {
				      "current_lane_cards": [
				        {
				          "id": "run-old",
				          "run_id": "run-old",
				          "title": "XY-672",
				          "detail": "agent_run",
				          "tone": "running",
				          "counts_as_running": true,
				          "needs_attention": false,
				          "is_waiting": false,
				          "assigned_account_fingerprints": [],
				          "assigned_account_emails": [],
				          "run": {
				            "run_id": "run-old",
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
				    "current_lane_cards": [
				      {
				        "id": "run-live",
				        "run_id": "run-live",
				        "title": "PUB-1147",
				        "detail": "agent_run",
				        "tone": "waiting",
				        "counts_as_running": true,
				        "needs_attention": false,
				        "is_waiting": true,
				        "assigned_account_fingerprints": [],
				        "assigned_account_emails": [],
				        "run": {
				          "run_id": "run-live",
				          "issue_identifier": "PUB-1147",
				          "wait_reason": "model_execution"
				        }
				      }
				    ]
				  }
				}
			"""
		))

		XCTAssertEqual(store.operatorSnapshot?.currentLanes.map(\.runID), ["run-old"])
		XCTAssertEqual(store.operatorPresentation?.currentLaneCards.map(\.runID), ["run-live"])
	}

	@MainActor
	func testRunActivityBeforeSnapshotCreatesVisiblePresentation() throws {
		let store = AccountStore()

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
				        "assigned_account_fingerprints": ["...123456"],
				        "assigned_account_emails": ["copy@example.com"],
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

		XCTAssertNil(store.operatorSnapshot)
		XCTAssertEqual(store.operatorPresentation?.currentLaneCards.map(\.runID), ["run-live"])
	}

	@MainActor
	func testSnapshotCurrentLanesWithoutPresentationDoNotCreateVisiblePresentation() throws {
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
			        "issue_identifier": "XY-672",
			        "wait_reason": "model_execution",
			        "account": {
			          "email": "copy@example.com",
			          "account_fingerprint": "...123456"
			        }
			      }
			    ]
			  }
			}
			"""
		))

		XCTAssertEqual(store.operatorSnapshot?.currentLanes.map(\.runID), ["run-live"])
		XCTAssertNil(store.operatorPresentation)
	}

	@MainActor
	func testRunActivityCurrentLanesWithoutPresentationAreIgnored() throws {
		let store = AccountStore()

		try store.applyOperatorDashboardEvent(dashboardEvent(
			type: "runActivity",
			payload: """
			{
			  "emittedAtUnixEpoch": 30,
			  "currentLanes": [
			    {
			      "run_id": "run-live",
			      "issue_identifier": "XY-672",
			      "current_operation": "agent_run",
			      "accounts": [
			        {
			          "email": "copy@example.com",
			          "account_fingerprint": "...123456",
			          "status": "selected"
			        }
			      ]
			    }
			  ]
			}
			"""
		))

		XCTAssertNil(store.operatorSnapshot)
		XCTAssertNil(store.operatorPresentation)
	}
}
