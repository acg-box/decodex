mod marker;
mod output;
mod shell;

pub(super) use self::{
	marker::{
		after_create_pending_marker_path, remove_orphan_marker_directory_if_safe,
		workspace_requires_after_create_pending_marker,
	},
	output::append_output_details,
	shell::run_workspace_hook_shell_command,
};
#[cfg(test)] pub(super) use marker::workspace_hook_shell_from_env;
