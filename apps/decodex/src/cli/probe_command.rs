use clap::Args;

use crate::{agent, prelude::Result};

#[derive(Debug, Args)]
pub(in crate::cli) struct ProbeCommand {
	/// Override the expected app-server transport during probing.
	#[arg(value_name = "TRANSPORT", default_value = "stdio://")]
	pub(in crate::cli) transport: String,
}
impl ProbeCommand {
	pub(in crate::cli) fn run(&self) -> Result<()> {
		let report = agent::probe_app_server(&self.transport)?;

		println!(
			"probe ok: preflight_checks={} thread={} turn={} events={} output={}",
			report.capability_preflight.check_count(),
			report.thread_id,
			report.turn_id,
			report.event_count,
			report.final_output
		);

		tracing::info!(
			user_agent = %report.user_agent,
			thread_id = %report.thread_id,
			turn_id = %report.turn_id,
			event_count = report.event_count,
			"Completed probe."
		);

		Ok(())
	}
}
