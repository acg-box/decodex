use std::{path::Path, process::Command};

pub(in crate::recovery) enum ReviewHandoffLineage {
	Descends,
	Diverged,
	Unknown,
}

pub(in crate::recovery) fn worktree_head_descends_from_review_handoff(
	worktree_path: &Path,
	recorded_head_oid: &str,
	local_head_oid: &str,
) -> ReviewHandoffLineage {
	if recorded_head_oid == local_head_oid {
		return ReviewHandoffLineage::Descends;
	}

	let Ok(output) = Command::new("git")
		.arg("-C")
		.arg(worktree_path)
		.args(["merge-base", "--is-ancestor", recorded_head_oid, local_head_oid])
		.output()
	else {
		return ReviewHandoffLineage::Unknown;
	};

	match output.status.code() {
		Some(0) => ReviewHandoffLineage::Descends,
		Some(1) => ReviewHandoffLineage::Diverged,
		_ => ReviewHandoffLineage::Unknown,
	}
}
