use crate::{
	agent::{
		ISSUE_REVIEW_CHECKPOINT_TOOL_NAME, ISSUE_REVIEW_HANDOFF_TOOL_NAME,
		ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME, ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
	},
	config::ReviewLevel,
};

pub(super) fn build_handoff_review_guidance(review_level: ReviewLevel) -> String {
	match review_level {
		ReviewLevel::Off => format!(
			"- `[codex].review = \"off\"` for this project, so skip Decodex Review and do not call `{}`.\n",
			ISSUE_REVIEW_CHECKPOINT_TOOL_NAME
		),
		ReviewLevel::Standard | ReviewLevel::Strict => format!(
			"- Commit the lane work, rerun required validation, confirm review-blocking local changes are absent, push the branch, and prepare the non-draft PR for runtime review.\n- Do not request Decodex Review yourself and do not call `{}`. Decodex owns the independent current-head review request, checkpoint recording, finding routing, and post-review decision after PR-backed handoff succeeds.\n- Use the registered project workflow policy already injected above as the authoritative review policy source; do not look for or require a repo-local `WORKFLOW.md` unless it was explicitly listed in `context.read_first`.\n",
			ISSUE_REVIEW_CHECKPOINT_TOOL_NAME
		),
	}
}

pub(super) fn build_repair_review_guidance(review_level: ReviewLevel) -> String {
	match review_level {
		ReviewLevel::Off => format!(
			"- `[codex].review = \"off\"` for this project, so skip Decodex Review and do not call `{}`.\n",
			ISSUE_REVIEW_CHECKPOINT_TOOL_NAME
		),
		ReviewLevel::Standard | ReviewLevel::Strict => format!(
			"- Commit the repaired lane work, rerun required validation, confirm review-blocking local changes are absent, push the repaired branch head, and prepare the PR for runtime repair verification.\n- Do not request Decodex Review yourself and do not call `{}`. Decodex owns the independent current-head repair review request, checkpoint recording, finding routing, and retained post-review decision after repair completion succeeds.\n- Use the registered project workflow policy already injected above as the authoritative review policy source; do not look for or require a repo-local `WORKFLOW.md` unless it was explicitly listed in `context.read_first`.\n",
			ISSUE_REVIEW_CHECKPOINT_TOOL_NAME
		),
	}
}

pub(super) fn build_handoff_completion_guidance(review_level: ReviewLevel) -> String {
	match review_level {
		ReviewLevel::Standard | ReviewLevel::Strict => format!(
			"- Call `{}` after the branch is pushed, the non-draft PR is ready, and required validation has passed. Decodex will run the runtime-owned review gate after handoff. Then call `{}` with path `review_handoff`.\n",
			ISSUE_REVIEW_HANDOFF_TOOL_NAME, ISSUE_TERMINAL_FINALIZE_TOOL_NAME
		),
		ReviewLevel::Off => format!(
			"- Call `{}` after the branch is pushed, the non-draft PR is ready, and required validation has passed. Then call `{}` with path `review_handoff`.\n",
			ISSUE_REVIEW_HANDOFF_TOOL_NAME, ISSUE_TERMINAL_FINALIZE_TOOL_NAME
		),
	}
}

pub(super) fn build_repair_completion_guidance(review_level: ReviewLevel) -> String {
	match review_level {
		ReviewLevel::Standard | ReviewLevel::Strict => format!(
			"- Call `{}` after the repaired head is pushed and required validation has passed. Decodex will run the runtime-owned repair review gate after completion. Then call `{}` with path `review_repair`.\n",
			ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME, ISSUE_TERMINAL_FINALIZE_TOOL_NAME
		),
		ReviewLevel::Off => format!(
			"- Call `{}` after the repaired head is pushed and required validation has passed. Then call `{}` with path `review_repair`.\n",
			ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME, ISSUE_TERMINAL_FINALIZE_TOOL_NAME
		),
	}
}

pub(super) fn build_handoff_continuation_review_guidance(review_level: ReviewLevel) -> String {
	match review_level {
		ReviewLevel::Off => format!(
			"- `[codex].review = \"off\"` for this project, so continue without Decodex Review and do not call `{}`.\n",
			ISSUE_REVIEW_CHECKPOINT_TOOL_NAME
		),
		ReviewLevel::Standard | ReviewLevel::Strict => format!(
			"- Resume by committing any review-blocking lane edits, rerunning required validation, pushing the branch, and preparing the PR for runtime review. Do not request Decodex Review yourself and do not call `{}`; Decodex owns the independent current-head review checkpoint after handoff.\n- Use the registered project workflow policy injected above as the authoritative source, not a repo-local `WORKFLOW.md`.\n",
			ISSUE_REVIEW_CHECKPOINT_TOOL_NAME
		),
	}
}

pub(super) fn build_repair_continuation_review_guidance(review_level: ReviewLevel) -> String {
	match review_level {
		ReviewLevel::Off => format!(
			"- `[codex].review = \"off\"` for this project, so continue without Decodex Review and do not call `{}`.\n",
			ISSUE_REVIEW_CHECKPOINT_TOOL_NAME
		),
		ReviewLevel::Standard | ReviewLevel::Strict => format!(
			"- Resume by committing any review-blocking repair edits, rerunning required validation, pushing the repaired branch head, and preparing the PR for runtime repair verification. Do not request Decodex Review yourself and do not call `{}`; Decodex owns the independent current-head repair checkpoint after repair completion.\n- Use the registered project workflow policy injected above as the authoritative source, not a repo-local `WORKFLOW.md`.\n",
			ISSUE_REVIEW_CHECKPOINT_TOOL_NAME
		),
	}
}

pub(super) fn build_handoff_continuation_completion_guidance(
	review_level: ReviewLevel,
	pr_title: &str,
) -> String {
	match review_level {
		ReviewLevel::Standard | ReviewLevel::Strict => format!(
			"- If the implementation is handoff-ready, ensure the non-draft PR title is `{pr_title}` and finish the PR-backed tracker handoff after required validation has passed; Decodex will run the runtime-owned review gate after handoff.\n",
		),
		ReviewLevel::Off => format!(
			"- If the implementation is review-ready, ensure the non-draft PR title is `{pr_title}` and finish the PR-backed tracker handoff after required validation has passed.\n",
		),
	}
}

pub(super) fn build_repair_continuation_completion_guidance(review_level: ReviewLevel) -> String {
	match review_level {
		ReviewLevel::Standard | ReviewLevel::Strict => format!(
			"- Call `{}` after the repaired head is pushed and required validation has passed; Decodex will run the runtime-owned repair review gate after completion. Then call `{}` with path `review_repair`.\n",
			ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME, ISSUE_TERMINAL_FINALIZE_TOOL_NAME
		),
		ReviewLevel::Off => format!(
			"- Call `{}` after the repaired head is pushed and required validation has passed, and then call `{}` with path `review_repair`.\n",
			ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME, ISSUE_TERMINAL_FINALIZE_TOOL_NAME
		),
	}
}

pub(super) fn build_repair_github_review_guidance(
	review_level: ReviewLevel,
	repair_tool_name: &str,
) -> String {
	if review_level.uses_github_review() {
		return format!(
			"- Do not request GitHub Review yourself. Decodex will post the next runtime-owned GitHub Review request after `{repair_tool_name}` succeeds.\n",
		);
	}

	String::from(
		"- Do not request GitHub Review from this run; the configured review level does not use the runtime-owned GitHub Review path.\n",
	)
}

pub(super) fn build_repair_retained_tail_guidance(
	review_level: ReviewLevel,
	success_state: &str,
) -> String {
	if review_level.uses_github_review() {
		return format!(
			"- Keep the tracker issue in `{success_state}`. Decodex will handle the later GitHub Review request or clean-path runtime landing, closeout, and cleanup lifecycle.\n",
		);
	}

	format!(
		"- Keep the tracker issue in `{success_state}`. Decodex will handle the clean-path runtime landing, closeout, and cleanup lifecycle.\n",
	)
}
