use std::process::Output;

use crate::prelude::Result;

const WORKSPACE_HOOK_CAPTURE_LIMIT: usize = 1_024 * 1_024;
const WORKSPACE_HOOK_TRUNCATED_MARKER: &[u8] = b"\n[decodex truncated workspace hook output]\n";

pub(in crate::worktree) fn append_output_details(buffer: &mut String, output: &Output) {
	let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
	let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();

	if !stdout.is_empty() {
		buffer.push_str(&format!(" stdout: `{stdout}`."));
	}
	if !stderr.is_empty() {
		buffer.push_str(&format!(" stderr: `{stderr}`."));
	}
}

pub(in crate::worktree::hooks) fn append_capped_workspace_hook_output(
	buffer: &mut Vec<u8>,
	chunk: &[u8],
) {
	if buffer.len() >= WORKSPACE_HOOK_CAPTURE_LIMIT {
		return;
	}

	let remaining = WORKSPACE_HOOK_CAPTURE_LIMIT - buffer.len();

	if chunk.len() <= remaining {
		buffer.extend_from_slice(chunk);

		return;
	}

	let marker_len = remaining.min(WORKSPACE_HOOK_TRUNCATED_MARKER.len());
	let chunk_len = remaining.saturating_sub(marker_len);

	buffer.extend_from_slice(&chunk[..chunk_len]);
	buffer.extend_from_slice(&WORKSPACE_HOOK_TRUNCATED_MARKER[..marker_len]);
}

pub(in crate::worktree::hooks) fn append_process_group_cleanup_details(
	buffer: &mut String,
	cleanup_result: Result<()>,
) {
	if let Err(error) = cleanup_result {
		buffer.push_str(&format!(" process-group cleanup error: `{error}`."));
	}
}
