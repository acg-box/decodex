use crate::{
	orchestrator::execution_failure::{
		self, Command, Digest, LoopGuardrailWorktreeFingerprint, Path, Result, Sha256,
	},
	state,
};

pub(crate) fn loop_guardrail_worktree_fingerprint(
	worktree_path: &Path,
) -> Result<Option<LoopGuardrailWorktreeFingerprint>> {
	let Some(head_sha) = execution_failure::worktree_head_oid(worktree_path)? else {
		return Ok(None);
	};
	let Some(tracked_status) =
		git_guardrail_output(worktree_path, &["status", "--porcelain", "--untracked-files=no"])?
	else {
		return Ok(None);
	};
	let Some(raw_status) = git_guardrail_output(worktree_path, &["status", "--porcelain"])? else {
		return Ok(None);
	};
	let Some(tracked_diff) =
		git_guardrail_output(worktree_path, &["diff", "--binary", "--no-ext-diff", "HEAD", "--"])?
	else {
		return Ok(None);
	};
	let effective_status = loop_guardrail_effective_status(&raw_status);
	let branch_delta_present = execution_failure::repo_gate_changed_tracked_files(worktree_path)
		.is_ok_and(|changed_files| !changed_files.is_empty());

	Ok(Some(LoopGuardrailWorktreeFingerprint {
		head_sha,
		tracked_status_hash: loop_guardrail_text_hash(&tracked_status),
		tracked_diff_hash: loop_guardrail_text_hash(&tracked_diff),
		effective_status_hash: loop_guardrail_text_hash(&effective_status),
		branch_delta_present,
		effective_delta_present: branch_delta_present
			|| !effective_status.trim().is_empty()
			|| !tracked_diff.trim().is_empty(),
	}))
}

pub(crate) fn loop_guardrail_effective_status(raw_status: &str) -> String {
	let lines = raw_status
		.lines()
		.map(str::trim_end)
		.filter(|line| !line.is_empty())
		.filter(|line| !state::is_untracked_decodex_runtime_artifact_status_line(line))
		.collect::<Vec<_>>();

	if lines.is_empty() {
		return String::new();
	}

	let mut status = lines.join("\n");

	status.push('\n');

	status
}

pub(crate) fn git_guardrail_output(worktree_path: &Path, args: &[&str]) -> Result<Option<String>> {
	let output = Command::new("git").arg("-C").arg(worktree_path).args(args).output()?;

	if !output.status.success() {
		return Ok(None);
	}

	Ok(Some(String::from_utf8_lossy(&output.stdout).into_owned()))
}

pub(crate) fn loop_guardrail_text_hash(text: &str) -> String {
	let digest = <Sha256 as Digest>::digest(text.as_bytes());
	let mut hash = String::with_capacity(64);

	for byte in digest {
		hash.push(char::from(b"0123456789abcdef"[(byte >> 4) as usize]));
		hash.push(char::from(b"0123456789abcdef"[(byte & 0x0f) as usize]));
	}

	hash
}
