use crate::orchestrator::tests::operator::status::dashboard;

#[test]
fn operator_dashboard_omits_lane_mutation_controls() {
	let response = dashboard::dashboard_response();

	assert!(!response.contains("function dashboardSubscriptionMatches(subscription)"));
	assert!(!response.contains("function clearDashboardSubscription(shouldSend = true)"));
	assert!(!response.contains("function toggleDashboardSubscription(subscription)"));
	assert!(!response.contains("toggleDashboardSubscription({ projectId })"));
	assert!(!response.contains("toggleDashboardSubscription({ projectId, issueId, runId })"));
	assert!(!response.contains("data-dashboard-control=\"focusProject\""));
	assert!(!response.contains("data-dashboard-control=\"focusRun\""));
	assert!(!response.contains("data-dashboard-control=\"pauseProject\""));
	assert!(!response.contains("data-dashboard-control=\"resumeProject\""));
	assert!(!response.contains(">Watch</button>"));
	assert!(!response.contains(">Watching</button>"));
	assert!(!response.contains(">Pause</button>"));
	assert!(!response.contains(">Resume</button>"));
	assert!(!response.contains("data-dashboard-control=\"retryRun\""));
	assert!(!response.contains(">Retry now</button>"));
	assert!(!response.contains("data-dashboard-control=\"interruptRun\""));
	assert!(!response.contains("aria-label=\"Stop this active Decodex work\""));
	assert!(!response.contains("runInterruptControlEnabled"));
	assert!(!response.contains("renderRunStopControl"));
	assert!(!response.contains("const statusLineParts = [...statusBits];"));
	assert!(!response.contains("statusLineParts.splice(1, 0, stopControl);"));
	assert!(!response.contains(".status-line .run-stop-button {"));
	assert!(!response.contains("action === \"interruptRun\""));
	assert!(!response.contains("case \"interruptRun\""));
	assert!(response.contains("<div class=\"status-line\">${statusBits.join(\"\")}</div>"));
	assert!(!response.contains("<rect x=\"4.2\" y=\"3.2\" width=\"2.9\" height=\"9.6\""));
	assert!(!response.contains("class=\"row-head run-row-head\""));
	assert!(!response.contains("class=\"run-head-aside\""));
	assert!(!response.contains("class=\"run-actions\""));
	assert!(!response.contains("data-tone=\"danger\" title=\"Stop this active Decodex work.\""));
	assert!(!response.contains("run-stop-button {\n\t\t\t\tposition: absolute;"));
}
