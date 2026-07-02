use std::io::{ErrorKind, Write};

use crate::{
	orchestrator::{OperatorStateEndpoint, OperatorStatusSnapshot},
	prelude::Result,
};

pub(in crate::orchestrator) fn write_cli_output<W>(writer: &mut W, output: &str) -> Result<()>
where
	W: Write,
{
	match writer.write_all(output.as_bytes()).and_then(|()| writer.flush()) {
		Ok(()) => Ok(()),
		Err(error) if error.kind() == ErrorKind::BrokenPipe => Ok(()),
		Err(error) => Err(error.into()),
	}
}

pub(in crate::orchestrator) fn publish_operator_snapshot(
	operator_state_endpoint: &OperatorStateEndpoint,
	snapshot: &OperatorStatusSnapshot,
) {
	if let Err(error) = operator_state_endpoint.publish_snapshot(snapshot) {
		let _ = error;

		tracing::warn!(
			"Operator snapshot publish failed; sensitive runtime details were withheld from control-plane logs."
		);
	}
}
