use std::{
	ffi::OsStr,
	fs,
	path::{Path, PathBuf},
};

use crate::{prelude::Result, state, workflow::WorkflowWorkspaceHooks};

const AFTER_CREATE_PENDING_MARKER: &str = ".decodex-after-create.pending";

pub(in crate::worktree) fn after_create_pending_marker_path(worktree_path: &Path) -> PathBuf {
	worktree_path.join(AFTER_CREATE_PENDING_MARKER)
}

pub(in crate::worktree) fn workspace_requires_after_create_pending_marker(
	hooks: Option<&WorkflowWorkspaceHooks>,
) -> bool {
	hooks.is_some_and(|hooks| !hooks.after_create_commands().is_empty())
}

pub(in crate::worktree) fn remove_orphan_marker_directory_if_safe(path: &Path) -> Result<bool> {
	if path.join(".git").exists() || !path.is_dir() {
		return Ok(false);
	}

	for entry in fs::read_dir(path)? {
		let entry = entry?;
		let entry_path = entry.path();
		let relative = entry_path.strip_prefix(path).unwrap_or(entry_path.as_path());

		if relative == Path::new(AFTER_CREATE_PENDING_MARKER)
			|| state::is_decodex_runtime_artifact_relative_path(relative)
		{
			continue;
		}

		return Ok(false);
	}

	fs::remove_dir_all(path)?;

	Ok(true)
}

pub(in crate::worktree) fn workspace_hook_shell_from_env(
	shell: Option<std::ffi::OsString>,
) -> (std::ffi::OsString, &'static str) {
	if let Some(shell) = shell
		&& !shell.is_empty()
	{
		let shell_path = Path::new(&shell);
		let shell_name = shell_path.file_name().and_then(OsStr::to_str);

		if shell_name == Some("sh") {
			return (std::ffi::OsString::from("/bin/sh"), "-c");
		}
		if !shell_path.is_absolute() || shell_path.is_file() {
			return (shell, "-lc");
		}
	}

	(std::ffi::OsString::from("/bin/sh"), "-c")
}
