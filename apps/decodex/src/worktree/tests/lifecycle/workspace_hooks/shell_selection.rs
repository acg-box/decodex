use std::ffi::OsString;

use crate::worktree::{self};

#[test]
fn workspace_hook_shell_uses_posix_sh_for_sh_or_missing_shell() {
	for shell_env in [Some(OsString::from("/bin/sh")), None] {
		let (shell, shell_flag) = worktree::workspace_hook_shell_from_env(shell_env);

		assert_eq!(shell, std::ffi::OsString::from("/bin/sh"));
		assert_eq!(shell_flag, "-c");
	}
}
