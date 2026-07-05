use crate::{
	agent::app_server::{CodexAccountActivitySummary, EffectiveThreadConfig, RunRecorder, markers},
	prelude::Result,
};

impl RunRecorder<'_> {
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
}
