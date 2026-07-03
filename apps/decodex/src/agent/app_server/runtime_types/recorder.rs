use std::path::PathBuf;

use crate::{
	agent::app_server::{
		ChildActivityAccumulator, CodexAccountActivitySummary, EffectiveThreadConfig,
		ProtocolActivityAccumulator, StateStore, markers,
	},
	prelude::Result,
	state::ProtocolActivityMarker,
};

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

	pub(crate) fn mark_activity(&self) -> Result<()> {
		if let Some(marker_path) = self.activity_marker_path {
			markers::write_activity_marker_best_effort(
				marker_path,
				self.run_id,
				self.attempt_number,
			);
		};

		Ok(())
	}

	pub(crate) fn set_thread_id(&mut self, thread_id: &str) -> Result<()> {
		self.thread_id = Some(thread_id.to_owned());

		if let Some(marker_path) = self.activity_marker_path {
			markers::write_thread_marker_best_effort(
				marker_path,
				self.run_id,
				self.attempt_number,
				thread_id,
			);
		}

		Ok(())
	}

	pub(crate) fn set_turn_id(&mut self, turn_id: &str) -> Result<()> {
		self.turn_id = Some(turn_id.to_owned());

		if let Some(marker_path) = self.activity_marker_path {
			markers::write_turn_marker_best_effort(
				marker_path,
				self.run_id,
				self.attempt_number,
				turn_id,
			);
		}

		Ok(())
	}

	pub(crate) fn set_thread_status(
		&mut self,
		status: &str,
		active_flags: &[String],
	) -> Result<()> {
		if let Some(marker_path) = self.activity_marker_path {
			markers::write_thread_status_marker_best_effort(
				marker_path,
				self.run_id,
				self.attempt_number,
				self.thread_id.as_deref(),
				self.turn_id.as_deref(),
				status,
				active_flags,
			);
		}

		Ok(())
	}

	pub(crate) fn set_effective_runtime(&mut self, runtime: &EffectiveThreadConfig) -> Result<()> {
		if let Some(marker_path) = self.activity_marker_path {
			markers::write_effective_runtime_marker_best_effort(
				marker_path,
				self.run_id,
				self.attempt_number,
				self.thread_id.as_deref(),
				self.turn_id.as_deref(),
				runtime,
			);
		}

		Ok(())
	}

	pub(crate) fn set_codex_account(
		&mut self,
		summary: &CodexAccountActivitySummary,
		account_summaries: &[CodexAccountActivitySummary],
	) -> Result<()> {
		if let Some(marker_path) = self.activity_marker_path {
			markers::write_codex_account_marker_best_effort(
				marker_path,
				self.run_id,
				self.attempt_number,
				summary,
				account_summaries,
			);
		}

		Ok(())
	}

	pub(crate) fn record(&mut self, event_type: &str, payload: &str) -> Result<()> {
		self.state_store.append_event(self.run_id, self.next_sequence, event_type, payload)?;

		let child_activity = self.child_activity.record(event_type, payload);
		let protocol_activity = self.protocol_activity.record(event_type, payload, &child_activity);

		self.state_store.record_run_activity_summary(
			self.run_id,
			self.attempt_number,
			Some(&child_activity),
			Some(&protocol_activity),
		)?;

		if let Some(marker_path) = self.activity_marker_path {
			let activity = ProtocolActivityMarker {
				run_id: self.run_id,
				attempt_number: self.attempt_number,
				thread_id: self.thread_id.as_deref(),
				turn_id: self.turn_id.as_deref(),
				event_count: self.next_sequence,
				last_event_type: event_type,
				child_agent_activity: Some(&child_activity),
				protocol_activity: Some(&protocol_activity),
			};

			markers::write_protocol_activity_marker_best_effort(marker_path, &activity);
		}

		self.next_sequence += 1;

		Ok(())
	}
}
