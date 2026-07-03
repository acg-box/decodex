mod accounts;
mod dashboard_html;
mod lane_control;
mod run_activity;
mod state_endpoint;
mod websocket;

use crate::orchestrator::tests::operator::status::{
	Arc, Child, CodexAccountActivitySummary, CodexAccountMarker, Command,
	DASHBOARD_MAX_WEBSOCKET_CLIENTS, DashboardClientSubscription, DashboardEventHub, Duration,
	ErrorKind, Instant, Mutex, OPERATOR_DASHBOARD_ALIAS_ENDPOINT_PATH,
	OPERATOR_DASHBOARD_ENDPOINT_PATH, OffsetDateTime, OperatorControlRequests, Path, PathBuf,
	ProjectRegistration, ProtocolActivityMarker, ProtocolActivitySummary,
	PublishedOperatorSnapshot, RUN_CONTROL_CHANNEL_DIR, RUN_CONTROL_CHANNEL_TRANSPORT_LOCAL_FILE,
	Read, ServiceConfig, Shutdown, SocketAddr, StateStore, TcpListener, TcpStream, TempDir,
	TestEnvVarGuard, TrackerIssue, Value, Write, fs, git_status_success, orchestrator, panic,
	rewrite_run_activity_marker_host_boot_id, runtime, sample_issue, sample_issue_with_sort_fields,
	seed_local_linear_execution_events, service_config_path, slice, state,
	successful_linear_execution_history_comments_with_cleanup, temp_project_layout, thread,
};
use websocket::{
	open_dashboard_websocket_client, read_websocket_json_until, websocket_text_payload,
};

const OPERATOR_DASHBOARD_TEST_TIMEOUT: Duration = Duration::from_secs(5);

#[cfg(unix)]
struct RunLeaseMissingControlFixture {
	issue: TrackerIssue,
	channel_path: PathBuf,
	child: Child,
	child_process_id: u32,
}
#[cfg(unix)]
impl Drop for RunLeaseMissingControlFixture {
	fn drop(&mut self) {
		if matches!(self.child.try_wait(), Ok(None)) {
			let _ = self.child.kill();
			let _ = self.child.wait();
		}
	}
}
