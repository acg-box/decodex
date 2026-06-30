@testable import DecodexApp
import XCTest

final class AccountModelTests: XCTestCase {
	func testRefresh401RequiresReloginAndHidesCapacity() {
		let account = makeAccount(
			status: "unusable",
			recoveryAction: "login",
			refreshStatus: "failed",
			checkedAtUnixEpoch: 1_800_000_000,
			primaryRemainingPercent: 89
		)

		XCTAssertTrue(account.needsLogin)
		XCTAssertFalse(account.canRouteRuns)
		XCTAssertEqual(account.statusLabel, "login")
		XCTAssertNil(account.currentCapacityLabel)
	}

	func testExpiredAccountCanRefreshButDoesNotShowCapacity() {
		let account = makeAccount(
			status: "expired",
			recoveryAction: "refresh",
			checkedAtUnixEpoch: 1_800_000_000,
			primaryRemainingPercent: 89
		)

		XCTAssertFalse(account.needsLogin)
		XCTAssertTrue(account.canRouteRuns)
		XCTAssertEqual(account.statusLabel, "refresh")
		XCTAssertNil(account.currentCapacityLabel)
	}

	func testAvailableMeasuredAccountShowsCapacity() {
		let account = makeAccount(
			status: "available",
			planType: "pro",
			checkedAtUnixEpoch: 1_800_000_000,
			primaryRemainingPercent: 89
		)

		XCTAssertEqual(account.currentCapacityLabel, "20x")
	}

	func testProfilePeakFallsBackToDailyUsageBuckets() {
		let account = makeAccount(
			status: "available",
			profileDailyUsage: [
				AccountProfileDailyUsage(date: "2026-05-30", tokens: 123_456),
				AccountProfileDailyUsage(date: "2026-05-31", tokens: 789_000),
			]
		)

		XCTAssertEqual(account.profilePeakDailyTokensForDisplay, 789_000)
	}

	func testProfilePeakUsesExplicitStatsValueFirst() {
		let account = makeAccount(
			status: "available",
			profilePeakDailyTokens: 1_500_000,
			profileDailyUsage: [
				AccountProfileDailyUsage(date: "2026-05-30", tokens: 2_000_000),
			]
		)

		XCTAssertEqual(account.profilePeakDailyTokensForDisplay, 1_500_000)
	}

	func testCompactEmailKeepsDottedLocalSuffixesConsistent() {
		XCTAssertEqual(AccountDisplay.compactEmail("aurevoirxavier@gmail.com"), "aur...ier@gmail.com")
		XCTAssertEqual(AccountDisplay.compactEmail("aurevoirxavier.us@gmail.com"), "aur...us@gmail.com")
		XCTAssertEqual(AccountDisplay.compactEmail("aurevoirxavier.jp@gmail.com"), "aur...jp@gmail.com")
		XCTAssertEqual(AccountDisplay.compactEmail("aurevoirxavier.hk@gmail.com"), "aur...hk@gmail.com")
		XCTAssertEqual(AccountDisplay.compactEmail("xavier.lau@helixbox.ai"), "xav...lau@helixbox.ai")
	}

	@MainActor
	func testOptimisticLogoutRemovalMasksStaleAccountListsUntilBackendCatchesUp() {
		let removedAccount = makeAccount(
			status: "available",
			email: "remove@example.com",
			accountFingerprint: "fp-remove"
		)
		let keptAccount = makeAccount(
			status: "available",
			email: "keep@example.com",
			accountFingerprint: "fp-keep"
		)
		let store = AccountStore()

		store.applyAccountList(makeAccountList([removedAccount, keptAccount]))
		XCTAssertEqual(store.accounts.map(\.id), [removedAccount.id, keptAccount.id])

		store.beginOptimisticLogoutRemoval(removedAccount)
		XCTAssertEqual(store.accounts.map(\.id), [keptAccount.id])

		store.applyAccountList(makeAccountList([removedAccount, keptAccount]))
		XCTAssertEqual(store.accounts.map(\.id), [keptAccount.id])

		store.applyAccountList(makeAccountList([keptAccount]))
		XCTAssertEqual(store.accounts.map(\.id), [keptAccount.id])

		store.applyAccountList(makeAccountList([removedAccount, keptAccount]))
		XCTAssertEqual(store.accounts.map(\.id), [removedAccount.id, keptAccount.id])
	}

	func testUsageResetDisplayUsesInjectedClock() {
		let base = Date(timeIntervalSince1970: 1_800_000_000)
		let thirteenMinutesLater = Int(base.timeIntervalSince1970) + 780

		let pending = UsageResetDisplay.make(
			resetAtUnixEpoch: thirteenMinutesLater,
			now: base
		)
		let due = UsageResetDisplay.make(
			resetAtUnixEpoch: thirteenMinutesLater,
			now: base.addingTimeInterval(781)
		)

		XCTAssertEqual(pending.short, "13m")
		XCTAssertEqual(due.short, "0m")
		XCTAssertTrue(due.accessibility.contains("reset due now"))
	}

	func testOperatorChildActivityAdvancesCurrentElapsedFromStartedAt() throws {
		let payload = """
		{
		  "current_bucket": "Model",
		  "current_detail": "model output",
		  "current_elapsed_seconds": 5,
		  "current_started_unix_epoch": 100,
		  "wall_seconds": 20,
		  "buckets": [
		    {
		      "name": "Model",
		      "wall_seconds": 15
		    },
		    {
		      "name": "Tool",
		      "wall_seconds": 5
		    }
		  ]
		}
		""".data(using: .utf8)!

		let activity = try JSONDecoder().decode(OperatorChildAgentActivity.self, from: payload)
		let modelBucket = try XCTUnwrap(activity.buckets.first { $0.name == "Model" })
		let toolBucket = try XCTUnwrap(activity.buckets.first { $0.name == "Tool" })
		let now = Date(timeIntervalSince1970: 110)

		XCTAssertEqual(activity.currentElapsedSeconds(at: now), 10)
		XCTAssertEqual(activity.wallSeconds(at: now), 25)
		XCTAssertEqual(activity.wallSeconds(for: modelBucket, at: now), 20)
		XCTAssertEqual(activity.wallSeconds(for: toolBucket, at: now), 5)
	}

	func testOperatorRunCompactActivitySummaryShowsRunningLaneCosts() throws {
		let payload = """
		{
		  "run_id": "xy-941-attempt-1",
		  "issue_identifier": "XY-941",
		  "status": "running",
		  "phase": "executing",
		  "process_alive": true,
		  "child_agent_activity": {
		    "current_bucket": "Model",
		    "current_elapsed_seconds": 10,
		    "current_started_unix_epoch": 100,
		    "wall_seconds": 80,
		    "event_count": 9,
		    "input_tokens_cumulative": 4270000,
		    "output_tokens_cumulative": 12000,
		    "tool_call_count": 11,
		    "largest_tool_output_bytes": 180000,
		    "buckets": [
		      {
		        "name": "Model",
		        "wall_seconds": 70
		      },
		      {
		        "name": "Shell",
		        "wall_seconds": 10,
		        "tool_call_count": 11
		      }
		    ]
		  }
		}
		""".data(using: .utf8)!

		let run = try JSONDecoder().decode(OperatorRunStatus.self, from: payload)
		let summary = try XCTUnwrap(
			run.compactActivitySummary(at: Date(timeIntervalSince1970: 130))
		)

		XCTAssertTrue(summary.contains("Model 1m 30s"))
		XCTAssertTrue(summary.contains("in 4.27M / out 12.0k"))
		XCTAssertTrue(summary.contains("11 tools"))
		XCTAssertTrue(summary.contains("176KiB output"))
	}

	func testOperatorRunDecodesLifecycleMetricsByPhase() throws {
		let payload = """
		{
		  "run_id": "xy-941-attempt-2",
		  "issue_identifier": "XY-941",
		  "lifecycle_metrics": {
		    "attempt_count": 2,
		    "run_count": 2,
		    "captured_attempt_count": 2,
		    "missing_attempt_count": 0,
		    "protocol_event_count": 48,
		    "child_event_count": 54,
		    "wall_seconds": 1740,
		    "tool_call_count": 18,
		    "input_tokens_current": 105000,
		    "input_tokens_peak": 128000,
		    "input_tokens_cumulative": 7120000,
		    "output_tokens_cumulative": 20500,
		    "largest_tool_output_bytes": 180000,
		    "largest_tool_output_tool": "view_image",
		    "buckets": [
		      {
		        "name": "Model",
		        "wall_seconds": 1313,
		        "event_count": 23,
		        "tool_call_count": 0,
		        "input_tokens": 7120000,
		        "output_tokens": 20500,
		        "output_bytes": 0
		      }
		    ],
		    "phases": [
		      {
		        "phase": "development",
		        "label": "Development",
		        "attempt_count": 1,
		        "run_count": 1,
		        "captured_attempt_count": 1,
		        "missing_attempt_count": 0,
		        "protocol_event_count": 18,
		        "child_event_count": 24,
		        "wall_seconds": 910,
		        "tool_call_count": 7,
		        "input_tokens_cumulative": 2850000,
		        "output_tokens_cumulative": 8500,
		        "largest_tool_output_bytes": 34000,
		        "largest_tool_output_tool": "shell",
		        "buckets": []
		      },
		      {
		        "phase": "review",
		        "label": "Review",
		        "attempt_count": 1,
		        "run_count": 1,
		        "captured_attempt_count": 1,
		        "missing_attempt_count": 0,
		        "protocol_event_count": 30,
		        "child_event_count": 30,
		        "wall_seconds": 830,
		        "tool_call_count": 11,
		        "input_tokens_cumulative": 4270000,
		        "output_tokens_cumulative": 12000,
		        "largest_tool_output_bytes": 180000,
		        "largest_tool_output_tool": "view_image",
		        "buckets": []
		      }
		    ]
		  }
		}
		""".data(using: .utf8)!

		let run = try JSONDecoder().decode(OperatorRunStatus.self, from: payload)
		let lifecycle = try XCTUnwrap(run.lifecycleMetrics)

		XCTAssertEqual(lifecycle.attemptCount, 2)
		XCTAssertEqual(lifecycle.inputTokensCurrent, 105_000)
		XCTAssertEqual(lifecycle.inputTokensPeak, 128_000)
		XCTAssertEqual(lifecycle.inputTokensCumulative, 7_120_000)
		XCTAssertEqual(lifecycle.phases.map(\.label), ["Development", "Review"])
		XCTAssertEqual(lifecycle.phases.map(\.toolCallCount), [7, 11])
		XCTAssertEqual(lifecycle.buckets.first?.wallSeconds, 1313)
	}

	func testStoppedCurrentLaneUsesInactiveDurationAndAttentionTone() throws {
		let payload = """
		{
		  "run_id": "pub-1524-attempt-2",
		  "issue_identifier": "PUB-1524",
		  "status": "running",
		  "phase": "executing",
		  "process_alive": false,
		  "idle_for_seconds": 20815,
		  "protocol_idle_for_seconds": 20816,
		  "child_agent_activity": {
		    "current_bucket": "Model",
		    "current_detail": "waiting after completed item",
		    "current_elapsed_seconds": 20840,
		    "wall_seconds": 788,
		    "buckets": [
		      {
		        "name": "Model",
		        "wall_seconds": 21389
		      }
		    ]
		  }
		}
		""".data(using: .utf8)!

		let run = try JSONDecoder().decode(OperatorRunStatus.self, from: payload)

		XCTAssertFalse(run.countsAsRunning)
		XCTAssertTrue(run.hasAttentionTone)
		XCTAssertEqual(run.inactiveDurationSeconds, 20840)
	}

	func testFreshProtocolExecutionOverridesStaleProcessMarker() throws {
		let payload = """
		{
		  "run_id": "xy-957-attempt-3",
		  "issue_identifier": "XY-957",
		  "status": "running",
		  "phase": "executing",
		  "wait_reason": "model_execution",
		  "execution_liveness": "process_identity_mismatch",
		  "has_fresh_execution": true,
		  "counts_as_running": true,
		  "needs_attention": false,
		  "process_alive": false,
		  "process_liveness_reason": "host_boot_id_mismatch",
		  "thread_status": "active",
		  "idle_for_seconds": 1,
		  "protocol_idle_for_seconds": 1,
		  "last_progress_at": "2026-06-16T03:27:31Z",
		  "last_event_type": "turn/diff/updated"
		}
		""".data(using: .utf8)!

		let run = try JSONDecoder().decode(OperatorRunStatus.self, from: payload)

		XCTAssertTrue(run.hasFreshExecution)
		XCTAssertTrue(run.countsAsRunning)
		XCTAssertFalse(run.hasAttentionTone)
		XCTAssertEqual(run.inactiveDurationSeconds, 1)
	}

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

	private func dashboardEvent(
		type: String,
		payload: String
	) throws -> OperatorDashboardSocketEvent {
		let data = """
		{
		  "type": "\(type)",
		  "payload": \(payload)
		}
		""".data(using: .utf8)!

		return try JSONDecoder().decode(OperatorDashboardSocketEvent.self, from: data)
	}

	private func makeAccount(
		status: String,
		email: String = "copy@example.com",
		accountFingerprint: String = "...123456",
		recoveryAction: String? = nil,
		refreshStatus: String? = nil,
		planType: String? = nil,
		checkedAtUnixEpoch: Int? = nil,
		primaryRemainingPercent: Int? = nil,
		profilePeakDailyTokens: Int? = nil,
		profileDailyUsage: [AccountProfileDailyUsage]? = nil
	) -> CodexAccount {
		CodexAccount(
			accountFingerprint: accountFingerprint,
			email: email,
			selector: email,
			randomName: nil,
			randomNameKey: nil,
			randomNameOffset: nil,
			status: status,
			selected: false,
			codexActive: false,
			disabled: false,
			refreshTokenPresent: true,
			accessTokenExpiresAtUnixEpoch: nil,
			lastSelectedAtUnixEpoch: nil,
			cooldownUntilUnixEpoch: nil,
			note: nil,
			planType: planType,
			capacityMultiplier: nil,
			recoveryAction: recoveryAction,
			refreshStatus: refreshStatus,
			checkedAtUnixEpoch: checkedAtUnixEpoch,
			primaryWindowSeconds: nil,
			primaryRemainingPercent: primaryRemainingPercent,
			primaryResetsAtUnixEpoch: nil,
			secondaryWindowSeconds: nil,
			secondaryRemainingPercent: nil,
			secondaryResetsAtUnixEpoch: nil,
			creditsHasCredits: nil,
			creditsUnlimited: nil,
			creditsBalance: nil,
			rateLimitReachedType: nil,
			profileDisplayName: nil,
			profileUsername: nil,
			profileCheckedAtUnixEpoch: nil,
			profileLifetimeTokens: nil,
			profilePeakDailyTokens: profilePeakDailyTokens,
			profileLongestTaskSeconds: nil,
			profileCurrentStreakDays: nil,
			profileLongestStreakDays: nil,
			profileDailyUsage: profileDailyUsage,
			sevenDayUsedPercent: nil,
			sevenDayDailyAveragePercent: nil,
			usageRecords: nil
		)
	}

	private func makeAccountList(_ accounts: [CodexAccount]) -> AccountListResponse {
		AccountListResponse(
			accountsPath: "/tmp/accounts.json",
			globalConfigPath: "/tmp/config.toml",
			codexAuthPath: "/tmp/auth.json",
			codexAuth: nil,
			control: AccountControl(mode: "balanced", accountSelector: nil),
			accounts: accounts,
			usageEstimate: nil,
			usageProbeError: nil
		)
	}
}
