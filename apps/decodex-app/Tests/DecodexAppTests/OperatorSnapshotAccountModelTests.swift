@testable import DecodexApp
import XCTest

final class OperatorSnapshotAccountModelTests: XCTestCase {
	func testOperatorSnapshotAssignsCodexAccountRunsToAccountRows() throws {
		let assignedAccount = makeAccount(
			status: "available",
			email: "copy@example.com",
			accountFingerprint: "...123456"
		)
		let poolOnlyAccount = makeAccount(
			status: "available",
			email: "pool@example.com",
			accountFingerprint: "...654321"
		)
		let otherAssignedAccount = makeAccount(
			status: "available",
			email: "other@example.com",
			accountFingerprint: "...abcdef"
		)
		let payload = """
		{
		  "current_lanes": [
		    {
		      "run_id": "run-1",
		      "issue_identifier": "XY-445",
		      "codex_account": {
		        "account_email": "copy@example.com",
		        "account_fingerprint": "...123456"
		      },
		      "codex_accounts": [
		        {
		          "account_email": "copy@example.com",
		          "account_fingerprint": "...123456"
		        },
		        {
		          "account_email": "pool@example.com",
		          "account_fingerprint": "...654321"
		        }
		      ]
		    },
		    {
		      "run_id": "run-2",
		      "issue_identifier": "PUB-1147",
		      "codex_account": {
		        "account_email": "other@example.com",
		        "account_fingerprint": "...abcdef"
		      },
		      "codex_accounts": [
		        {
		          "account_email": "copy@example.com",
		          "account_fingerprint": "...123456"
		        },
		        {
		          "account_email": "other@example.com",
		          "account_fingerprint": "...abcdef"
		        },
		        {
		          "account_email": "pool@example.com",
		          "account_fingerprint": "...654321"
		        }
		      ]
		    }
		  ]
		}
		""".data(using: .utf8)!

		let snapshot = try JSONDecoder().decode(OperatorSnapshotResponse.self, from: payload)

		XCTAssertEqual(snapshot.currentLanes(for: assignedAccount).map(\.runID), ["run-1"])
		XCTAssertEqual(snapshot.currentLanes(for: otherAssignedAccount).map(\.runID), ["run-2"])
		XCTAssertTrue(snapshot.currentLanes(for: poolOnlyAccount).isEmpty)
	}

	func testOperatorSnapshotKeepsUnassignedCurrentLanesVisibleGlobally() throws {
		let account = makeAccount(
			status: "available",
			email: "pool@example.com",
			accountFingerprint: "...654321"
		)
		let payload = """
		{
		  "current_lanes": [
		    {
		      "run_id": "run-unassigned",
		      "project_id": "pubfi-platform",
		      "issue_identifier": "PUB-1296",
		      "status": "running"
		    }
		  ]
		}
		""".data(using: .utf8)!

		let snapshot = try JSONDecoder().decode(OperatorSnapshotResponse.self, from: payload)

		XCTAssertEqual(snapshot.currentLanes.map(\.runID), ["run-unassigned"])
		XCTAssertEqual(snapshot.currentLaneCount, 1)
		XCTAssertTrue(snapshot.currentLanes(for: account).isEmpty)
	}

	func testOperatorProjectStatusSeparatesCurrentAndRunningLaneCounts() throws {
		let payload = """
		{
		  "projects": [
		    {
		      "project_id": "pubfi-platform",
		      "current_lane_count": 2,
		      "running_lane_count": 1,
		      "attention_count": 1
		    }
		  ]
		}
		""".data(using: .utf8)!

		let snapshot = try JSONDecoder().decode(OperatorSnapshotResponse.self, from: payload)
		let project = try XCTUnwrap(snapshot.projects.first)

		XCTAssertEqual(project.currentLaneCount, 2)
		XCTAssertEqual(project.runningLaneCount, 1)
		XCTAssertEqual(snapshot.currentLaneCount, 2)
		XCTAssertEqual(snapshot.runningLaneCount, 1)
		XCTAssertEqual(snapshot.attentionCount, 1)
	}

	func testOperatorProjectStatusDefaultsRunningLaneCountToCurrentLaneCount() throws {
		let payload = """
		{
		  "projects": [
		    {
		      "project_id": "pubfi-platform",
		      "current_lane_count": 2
		    }
		  ]
		}
		""".data(using: .utf8)!

		let snapshot = try JSONDecoder().decode(OperatorSnapshotResponse.self, from: payload)
		let project = try XCTUnwrap(snapshot.projects.first)

		XCTAssertEqual(project.currentLaneCount, 2)
		XCTAssertEqual(project.runningLaneCount, 2)
		XCTAssertEqual(snapshot.runningLaneCount, 2)
	}

	func testShadowedPostReviewLaneDoesNotInflateReviewOrLandingCounts() throws {
		let payload = """
		{
		  "projects": [
		    {
		      "project_id": "pubfi-platform",
		      "post_review_lane_count": 0
		    }
		  ],
		  "post_review_lanes": [
		    {
		      "classification": "ready_to_land",
		      "shadowed_by_current_lane": true
		    }
		  ]
		}
		""".data(using: .utf8)!

		let snapshot = try JSONDecoder().decode(OperatorSnapshotResponse.self, from: payload)

		XCTAssertEqual(snapshot.reviewCount, 0)
		XCTAssertEqual(snapshot.landingCount, 0)
		XCTAssertEqual(snapshot.postReviewLanes.first?.shadowedByCurrentLane, true)
	}

	func testOperatorSnapshotAssignsSelectedAccountWhenPrimaryAccountIsMissing() throws {
		let assignedAccount = makeAccount(
			status: "available",
			email: "copy@example.com",
			accountFingerprint: "...123456"
		)
		let poolOnlyAccount = makeAccount(
			status: "available",
			email: "pool@example.com",
			accountFingerprint: "...654321"
		)
		let payload = """
		{
		  "current_lanes": [
		    {
		      "run_id": "run-1",
		      "issue_identifier": "XY-689",
		      "accounts": [
		        {
		          "email": "copy@example.com",
		          "account_fingerprint": "...123456",
		          "status": "selected"
		        },
		        {
		          "email": "pool@example.com",
		          "account_fingerprint": "...654321",
		          "status": "available"
		        }
		      ]
		    }
		  ]
		}
		""".data(using: .utf8)!

		let snapshot = try JSONDecoder().decode(OperatorSnapshotResponse.self, from: payload)

		XCTAssertEqual(snapshot.currentLanes(for: assignedAccount).map(\.runID), ["run-1"])
		XCTAssertTrue(snapshot.currentLanes(for: poolOnlyAccount).isEmpty)
	}

	func testPresentationCardsRequireServerOwnedFields() throws {
		let payload = """
		{
		  "current_lane_cards": [
		    {
		      "id": "run-690",
		      "run_id": "run-690",
		      "detail": "agent_run",
		      "tone": "running",
		      "counts_as_running": true,
		      "needs_attention": false,
		      "is_waiting": false,
		      "assigned_account_fingerprints": ["...123456"],
		      "assigned_account_emails": ["copy@example.com"],
		      "run": {
		        "run_id": "run-690",
		        "issue_identifier": "XY-690",
		        "current_operation": "agent_run"
		      }
		    }
		  ]
		}
		""".data(using: .utf8)!

		XCTAssertThrowsError(try JSONDecoder().decode(OperatorSnapshotPresentation.self, from: payload))
	}

	func testPresentationCardsAssignAccountsFromServerFields() throws {
		let account = makeAccount(
			status: "available",
			email: "copy@example.com",
			accountFingerprint: "...123456"
		)
		let payload = """
		{
		  "current_lane_cards": [
		    {
		      "id": "run-690",
		      "run_id": "run-690",
		      "title": "XY-690",
		      "detail": "agent_run",
		      "tone": "running",
		      "counts_as_running": true,
		      "needs_attention": false,
		      "is_waiting": false,
		      "assigned_account_fingerprints": ["...123456"],
		      "assigned_account_emails": ["copy@example.com"],
		      "run": {
		        "run_id": "run-690",
		        "issue_identifier": "XY-690"
		      }
		    }
		  ]
		}
		""".data(using: .utf8)!
		let presentation = try JSONDecoder().decode(OperatorSnapshotPresentation.self, from: payload)
		let card = try XCTUnwrap(presentation.currentLaneCards.first)

		XCTAssertEqual(card.runID, "run-690")
		XCTAssertTrue(card.isAssigned(to: account))
	}

	func testOperatorSnapshotWarningSummaryUsesRawWarningToken() throws {
		let payload = """
		{
		  "warnings": ["external_observer_status_skipped"]
		}
		""".data(using: .utf8)!

		let snapshot = try JSONDecoder().decode(OperatorSnapshotResponse.self, from: payload)

		XCTAssertEqual(snapshot.warningSummary, "external_observer_status_skipped")
	}
}
