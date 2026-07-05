mod markers;
mod protocol;

use std::path::PathBuf;

use crate::agent::app_server::{ChildActivityAccumulator, ProtocolActivityAccumulator, StateStore};

pub(crate) struct RunRecorder<'a> {
	pub(crate) state_store: &'a StateStore,
	pub(crate) project_id: &'a str,
	pub(crate) issue_id: &'a str,
	pub(crate) run_id: &'a str,
	pub(crate) attempt_number: i64,
	pub(crate) activity_marker_path: Option<&'a PathBuf>,
	pub(crate) thread_id: Option<String>,
	pub(crate) turn_id: Option<String>,
	pub(crate) next_sequence: i64,
	pub(crate) child_activity: ChildActivityAccumulator,
	pub(crate) protocol_activity: ProtocolActivityAccumulator,
}
impl<'a> RunRecorder<'a> {
	#[cfg(test)]
	pub(crate) fn new(
		state_store: &'a StateStore,
		run_id: &'a str,
		attempt_number: i64,
		activity_marker_path: Option<&'a PathBuf>,
	) -> Self {
		Self::new_with_context(
			state_store,
			"unknown",
			"unknown",
			run_id,
			attempt_number,
			activity_marker_path,
		)
	}

	pub(crate) fn new_with_context(
		state_store: &'a StateStore,
		project_id: &'a str,
		issue_id: &'a str,
		run_id: &'a str,
		attempt_number: i64,
		activity_marker_path: Option<&'a PathBuf>,
	) -> Self {
		Self {
			state_store,
			project_id,
			issue_id,
			run_id,
			attempt_number,
			activity_marker_path,
			thread_id: None,
			turn_id: None,
			next_sequence: 1,
			child_activity: ChildActivityAccumulator::new(),
			protocol_activity: ProtocolActivityAccumulator::new(),
		}
	}

	pub(crate) fn project_id(&self) -> &str {
		self.project_id
	}

	pub(crate) fn issue_id(&self) -> &str {
		self.issue_id
	}
}
