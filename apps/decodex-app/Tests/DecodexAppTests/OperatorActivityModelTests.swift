@testable import DecodexApp
import XCTest

final class OperatorActivityModelTests: XCTestCase {
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
		XCTAssertEqual(lifecycle.buckets.first?.wallSeconds, 1_313)
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
		XCTAssertEqual(run.inactiveDurationSeconds, 20_840)
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
}
