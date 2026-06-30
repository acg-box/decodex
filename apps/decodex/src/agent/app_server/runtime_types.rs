use std::{path::PathBuf, time::Duration};

use super::{
	ChildActivityAccumulator, CodexAccountActivitySummary, CodexAccountProvider,
	CommandExecHealthCheck, DynamicToolHandler, EffectiveThreadConfig, PhaseGoalController,
	PhaseGoalRunStatus, ProtocolActivityAccumulator, StateStore,
	markers::{
		write_activity_marker_best_effort, write_codex_account_marker_best_effort,
		write_effective_runtime_marker_best_effort, write_protocol_activity_marker_best_effort,
		write_thread_marker_best_effort, write_thread_status_marker_best_effort,
		write_turn_marker_best_effort,
	},
	state,
};
use crate::agent::json_rpc::AppServerProcessEnv;

pub(crate) trait TurnContinuationGuard {
	fn should_continue_turn(&self, turn_count: u32) -> crate::prelude::Result<bool>;
	fn validate_continuation_boundary(&self, _turn_count: u32) -> crate::prelude::Result<()> {
		Ok(())
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AppServerThreadArchiveOutcome {
	Archived,
	DiscardedMissingThread,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RequestWaitPhase {
	Initialize,
	AccountLogin,
	ThreadStart,
	ThreadResume,
	TurnStart,
	TurnExecution,
}
impl RequestWaitPhase {
	pub(super) fn label(self) -> &'static str {
		match self {
			Self::Initialize => "initialize",
			Self::AccountLogin => "account/login/start",
			Self::ThreadStart => "thread/start",
			Self::ThreadResume => "thread/resume",
			Self::TurnStart => "turn/start",
			Self::TurnExecution => "turn execution",
		}
	}

	pub(super) fn transport_failure_is_retryable_startup(self) -> bool {
		matches!(
			self,
			Self::Initialize | Self::AccountLogin | Self::ThreadStart | Self::ThreadResume
		)
	}
}

pub(crate) struct AppServerThreadArchiveRequest<'a> {
	pub(crate) run_id: &'a str,
	pub(crate) issue_id: &'a str,
	pub(crate) attempt_number: i64,
	pub(crate) listen: &'a str,
	pub(crate) process_env: &'a AppServerProcessEnv,
	pub(crate) thread_id: &'a str,
	pub(crate) sequence_number: i64,
}

#[derive(Clone)]
pub(crate) struct AppServerRunRequest<'a> {
	pub(crate) project_id: String,
	pub(crate) run_id: String,
	pub(crate) issue_id: String,
	pub(crate) attempt_number: i64,
	pub(crate) listen: String,
	pub(crate) cwd: String,
	pub(crate) developer_instructions: String,
	pub(crate) user_input: String,
	pub(crate) max_turns: u32,
	pub(crate) timeout: Duration,
	pub(crate) process_env: AppServerProcessEnv,
	pub(crate) continuation_user_input: Option<String>,
	pub(crate) activity_marker_path: Option<PathBuf>,
	pub(crate) resume_thread_id: Option<String>,
	pub(crate) ephemeral_thread: bool,
	pub(crate) command_exec_health_check: Option<CommandExecHealthCheck>,
	pub(crate) dynamic_tool_handler: Option<&'a dyn DynamicToolHandler>,
	pub(crate) continuation_guard: Option<&'a dyn TurnContinuationGuard>,
	pub(crate) phase_goal_controller: Option<&'a dyn PhaseGoalController>,
	pub(crate) codex_account_provider: Option<&'a dyn CodexAccountProvider>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AppServerRunResult {
	pub(crate) user_agent: String,
	pub(crate) capability_preflight: super::AppServerCapabilityPreflightReport,
	pub(crate) thread_id: String,
	pub(crate) turn_id: String,
	pub(crate) turn_count: u32,
	pub(crate) event_count: i64,
	pub(crate) final_output: String,
	pub(crate) continuation_pending: bool,
	pub(crate) phase_goal_status: Option<PhaseGoalRunStatus>,
}

pub(super) struct RunRecorder<'a> {
	pub(super) state_store: &'a StateStore,
	pub(super) project_id: &'a str,
	pub(super) issue_id: &'a str,
	pub(super) run_id: &'a str,
	pub(super) attempt_number: i64,
	pub(super) activity_marker_path: Option<&'a PathBuf>,
	pub(super) thread_id: Option<String>,
	pub(super) turn_id: Option<String>,
	pub(super) next_sequence: i64,
	pub(super) child_activity: ChildActivityAccumulator,
	pub(super) protocol_activity: ProtocolActivityAccumulator,
}
impl<'a> RunRecorder<'a> {
	#[cfg(test)]
	pub(super) fn new(
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

	pub(super) fn new_with_context(
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

	pub(super) fn project_id(&self) -> &str {
		self.project_id
	}

	pub(super) fn issue_id(&self) -> &str {
		self.issue_id
	}

	pub(super) fn mark_activity(&self) -> crate::prelude::Result<()> {
		if let Some(marker_path) = self.activity_marker_path {
			write_activity_marker_best_effort(marker_path, self.run_id, self.attempt_number);
		};

		Ok(())
	}

	pub(super) fn set_thread_id(&mut self, thread_id: &str) -> crate::prelude::Result<()> {
		self.thread_id = Some(thread_id.to_owned());

		if let Some(marker_path) = self.activity_marker_path {
			write_thread_marker_best_effort(
				marker_path,
				self.run_id,
				self.attempt_number,
				thread_id,
			);
		}

		Ok(())
	}

	pub(super) fn set_turn_id(&mut self, turn_id: &str) -> crate::prelude::Result<()> {
		self.turn_id = Some(turn_id.to_owned());

		if let Some(marker_path) = self.activity_marker_path {
			write_turn_marker_best_effort(marker_path, self.run_id, self.attempt_number, turn_id);
		}

		Ok(())
	}

	pub(super) fn set_thread_status(
		&mut self,
		status: &str,
		active_flags: &[String],
	) -> crate::prelude::Result<()> {
		if let Some(marker_path) = self.activity_marker_path {
			write_thread_status_marker_best_effort(
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

	pub(super) fn set_effective_runtime(
		&mut self,
		runtime: &EffectiveThreadConfig,
	) -> crate::prelude::Result<()> {
		if let Some(marker_path) = self.activity_marker_path {
			write_effective_runtime_marker_best_effort(
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

	pub(super) fn set_codex_account(
		&mut self,
		summary: &CodexAccountActivitySummary,
		account_summaries: &[CodexAccountActivitySummary],
	) -> crate::prelude::Result<()> {
		if let Some(marker_path) = self.activity_marker_path {
			write_codex_account_marker_best_effort(
				marker_path,
				self.run_id,
				self.attempt_number,
				summary,
				account_summaries,
			);
		}

		Ok(())
	}

	pub(super) fn record(&mut self, event_type: &str, payload: &str) -> crate::prelude::Result<()> {
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
			let activity = state::ProtocolActivityMarker {
				run_id: self.run_id,
				attempt_number: self.attempt_number,
				thread_id: self.thread_id.as_deref(),
				turn_id: self.turn_id.as_deref(),
				event_count: self.next_sequence,
				last_event_type: event_type,
				child_agent_activity: Some(&child_activity),
				protocol_activity: Some(&protocol_activity),
			};

			write_protocol_activity_marker_best_effort(marker_path, &activity);
		}

		self.next_sequence += 1;

		Ok(())
	}
}

pub(super) struct TurnLoopResult {
	pub(super) turn_id: String,
	pub(super) turn_count: u32,
	pub(super) final_output: String,
	pub(super) continuation_pending: bool,
	pub(super) phase_goal_status: Option<PhaseGoalRunStatus>,
}

#[derive(Clone, Copy)]
pub(super) struct RequestDispatchContext<'a> {
	pub(super) phase: RequestWaitPhase,
	pub(super) dynamic_tool_handler: Option<&'a dyn DynamicToolHandler>,
	pub(super) codex_account_provider: Option<&'a dyn CodexAccountProvider>,
	pub(super) target_thread_id: Option<&'a str>,
	pub(super) target_turn_id: Option<&'a str>,
}
impl<'a> RequestDispatchContext<'a> {
	pub(super) fn new(
		phase: RequestWaitPhase,
		dynamic_tool_handler: Option<&'a dyn DynamicToolHandler>,
		codex_account_provider: Option<&'a dyn CodexAccountProvider>,
		target_thread_id: Option<&'a str>,
		target_turn_id: Option<&'a str>,
	) -> Self {
		Self {
			phase,
			dynamic_tool_handler,
			codex_account_provider,
			target_thread_id,
			target_turn_id,
		}
	}
}
