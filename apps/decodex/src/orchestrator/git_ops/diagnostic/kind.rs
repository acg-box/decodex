use crate::orchestrator::git_ops::RepoGateFailureKind;

impl RepoGateFailureKind {
	pub(in crate::orchestrator::git_ops) fn retry_schedule_kind(self) -> Option<&'static str> {
		match self {
			Self::GitLockContention => Some("git_lock_contention"),
			_ => None,
		}
	}
}

pub(crate) fn repo_gate_failure_kind_for_output(
	default_kind: RepoGateFailureKind,
	output_text: &str,
) -> RepoGateFailureKind {
	if repo_gate_is_git_lock_contention(output_text) {
		RepoGateFailureKind::GitLockContention
	} else {
		default_kind
	}
}

fn repo_gate_is_git_lock_contention(output_text: &str) -> bool {
	let output_text = output_text.to_ascii_lowercase();

	output_text.contains("index.lock")
		&& (output_text.contains("file exists")
			|| output_text.contains("already exists")
			|| output_text.contains("another git process seems to be running"))
}
