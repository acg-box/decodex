use crate::prelude::Result;

pub(in crate::recovery::git_worktree) fn trimmed_stdout(stdout: &[u8]) -> Result<String> {
	Ok(String::from_utf8(stdout.to_vec())?.trim().to_owned())
}
