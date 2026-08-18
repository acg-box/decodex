use std::io::{self, Write as _};

use serde::Serialize;
use serde_json::Value;

use crate::prelude::Result;

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(in crate::app_bridge) enum AppBridgeEvent<T = Value>
where
	T: Serialize,
{
	Result { payload: T },
	Error { message: String },
}

pub(in crate::app_bridge) fn emit_result<T>(payload: &T) -> Result<()>
where
	T: Serialize,
{
	emit_event(&AppBridgeEvent::Result { payload })
}

pub(in crate::app_bridge) fn emit_event<T>(event: &AppBridgeEvent<T>) -> Result<()>
where
	T: Serialize,
{
	let mut stdout = io::stdout().lock();

	serde_json::to_writer(&mut stdout, event)?;

	stdout.write_all(b"\n")?;
	stdout.flush()?;

	Ok(())
}
