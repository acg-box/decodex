use super::*;

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

mod accounts;
mod dashboard_html;
mod lane_control;
mod run_activity;
mod state_endpoint;
mod websocket;

use websocket::{
	open_dashboard_websocket_client, read_websocket_json_until, websocket_text_payload,
};
