use std::io;

use crate::{
	orchestrator::{self, OperatorStatusSnapshot, entrypoints::output},
	prelude::Result,
};

pub(in crate::orchestrator::entrypoints::status) fn print_operator_status_snapshot(
	snapshot: &OperatorStatusSnapshot,
	json: bool,
) -> Result<()> {
	let output = if json {
		format!("{}\n", serde_json::to_string_pretty(snapshot)?)
	} else {
		orchestrator::render_operator_status(snapshot)
	};
	let stdout = io::stdout();
	let mut stdout = stdout.lock();

	output::write_cli_output(&mut stdout, &output)
}
