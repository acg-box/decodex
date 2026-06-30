use super::{Command, Path, state};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::orchestrator) enum WorktreeTrackedChangeState {
	Clean,
	TrackedChanges,
	Unknown,
}

impl WorktreeTrackedChangeState {
	pub(in crate::orchestrator) fn has_tracked_changes(self) -> bool {
		self == Self::TrackedChanges
	}

	pub(in crate::orchestrator) fn is_unknown(self) -> bool {
		self == Self::Unknown
	}
}

pub(in crate::orchestrator) fn worktree_tracked_change_state(
	worktree_path: &Path,
) -> WorktreeTrackedChangeState {
	match worktree_path.try_exists() {
		Ok(false) => WorktreeTrackedChangeState::Clean,
		Ok(true) => match worktree_path.join(".git").try_exists() {
			Ok(false) => {
				match state::retained_path_contains_only_decodex_runtime_artifacts(worktree_path) {
					Ok(true) => WorktreeTrackedChangeState::Clean,
					Ok(false) => WorktreeTrackedChangeState::TrackedChanges,
					Err(_) => WorktreeTrackedChangeState::Unknown,
				}
			},
			Ok(true) => {
				let Ok(output) = Command::new("git")
					.arg("-C")
					.arg(worktree_path)
					.args(["status", "--porcelain"])
					.output()
				else {
					return WorktreeTrackedChangeState::Unknown;
				};

				if !output.status.success() {
					return WorktreeTrackedChangeState::Unknown;
				}

				let has_blocking_status = String::from_utf8_lossy(&output.stdout)
					.lines()
					.filter(|line| !line.trim_end().is_empty())
					.any(|line| !state::is_untracked_decodex_runtime_artifact_status_line(line));

				if has_blocking_status {
					WorktreeTrackedChangeState::TrackedChanges
				} else {
					WorktreeTrackedChangeState::Clean
				}
			},
			Err(_) => WorktreeTrackedChangeState::Unknown,
		},
		Err(_) => WorktreeTrackedChangeState::Unknown,
	}
}

pub(in crate::orchestrator) fn worktree_has_tracked_changes(worktree_path: &Path) -> bool {
	worktree_tracked_change_state(worktree_path).has_tracked_changes()
}
