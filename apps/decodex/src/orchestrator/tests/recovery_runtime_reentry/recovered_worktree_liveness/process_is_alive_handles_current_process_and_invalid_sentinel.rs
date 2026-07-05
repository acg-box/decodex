use std::process::{self};

use crate::orchestrator::{self};

#[cfg(unix)]
#[test]
fn process_is_alive_handles_current_process_and_invalid_sentinel() {
	assert!(
		orchestrator::process_is_alive(process::id()),
		"current process should always be reported as alive"
	);
	assert!(
		!orchestrator::process_is_alive(u32::MAX),
		"sentinel pid values should never be treated as live processes"
	);
}
