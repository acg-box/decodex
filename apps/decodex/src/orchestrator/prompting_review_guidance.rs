use crate::{
	agent::{
		ISSUE_REVIEW_CHECKPOINT_TOOL_NAME, ISSUE_REVIEW_HANDOFF_TOOL_NAME,
		ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME, ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
	},
	config::ReviewLevel,
};

const SELF_CHECK_INSTRUCTION: &str =
	"Review your work repeatedly and fix any logic bugs until no new issues are found.";

pub(super) fn build_handoff_review_guidance(review_level: ReviewLevel) -> String {
	match review_level {
		ReviewLevel::Off => format!(
			"- `[codex].review = \"off\"` for this project, so skip Self Check and Decodex Review, and do not call `{}`.\n",
			ISSUE_REVIEW_CHECKPOINT_TOOL_NAME
		),
		ReviewLevel::Basic => format!("- Self Check: {SELF_CHECK_INSTRUCTION}\n"),
		ReviewLevel::Standard | ReviewLevel::Strict => format!(
			"- Self Check: {SELF_CHECK_INSTRUCTION}\n- Before Decodex Review, commit the lane work, rerun required validation, and confirm review-blocking local changes are absent. Formal `{}` evidence is accepted only for a clean committed `HEAD`.\n- Decodex Review: request an independent fresh-context read-only review pass for the actual committed branch state. The reviewer must not edit files, push, land, or mutate tracker state.\n- Use the registered project workflow policy already injected above as the authoritative review policy source; do not look for or require a repo-local `WORKFLOW.md` unless it was explicitly listed in `context.read_first`.\n- Build an explicit `review_contract` for the checkpoint with `workflow_policy_source = \"registered_project_workflow\"`, `review_type = \"full_current_head_review\"`, the risk tier, objective, scope, non-goals, required checks, allowed expansion triggers, and validation evidence. Include expansion triggers for safety, authority-boundary, data-loss, security, live-mutation, public-API, migration, and operator-facing regressions when relevant.\n- Classify review cost with `review_cost_control`: `compact_current_head_review` is allowed only for low-risk small current-head, validation-backed, clean handoff review after both intended-behavior and adversarial checks; otherwise record `full_current_head_review` with `fallback_reason`. Full review is forced when high-risk surfaces, accepted findings or nonclean rounds, missing or stale validation, docs/config/API/security/data/privacy changes without sufficient evidence, weak evidence, repair review, or architecture risk exists. Compact review is not review skipping; it remains independent fresh-context current-head review.\n{route_guidance}- Validate reviewer comments before repair. Fix only accepted findings routed as `current_blocker`, keep the repair batch scoped to the smallest coherent owned set, rerun verification, and re-read `HEAD` before deciding the normalized review status.\n- Every time the Decodex Review pass produces a result for the current committed head, call `{}` with reviewer `independent_fresh_context`, that normalized status, the exact current `HEAD` SHA, the explicit `review_contract`, `review_cost_control`, concise evidence, checklist notes, structured accepted/rejected findings, and `finding_routes`.\n",
			ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
			ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
			route_guidance = review_signal_route_guidance()
		),
	}
}

pub(super) fn build_repair_review_guidance(review_level: ReviewLevel) -> String {
	match review_level {
		ReviewLevel::Off => format!(
			"- `[codex].review = \"off\"` for this project, so skip Self Check and Decodex Review, and do not call `{}`.\n",
			ISSUE_REVIEW_CHECKPOINT_TOOL_NAME
		),
		ReviewLevel::Basic => format!("- Self Check: {SELF_CHECK_INSTRUCTION}\n"),
		ReviewLevel::Standard | ReviewLevel::Strict => format!(
			"- Self Check: {SELF_CHECK_INSTRUCTION}\n- Before Decodex Review, commit the repaired lane work, rerun required validation, and confirm review-blocking local changes are absent. Formal `{}` evidence is accepted only for a clean committed `HEAD`.\n- Decodex Review: request an independent fresh-context read-only verification pass for the actual committed repaired branch state. The reviewer must not edit files, push, land, or mutate tracker state.\n- Use the registered project workflow policy already injected above as the authoritative review policy source; do not look for or require a repo-local `WORKFLOW.md` unless it was explicitly listed in `context.read_first`.\n- Build an explicit `review_contract` for the checkpoint with `workflow_policy_source = \"registered_project_workflow\"`, `review_type = \"repair_verification\"`, the risk tier, objective, scope, non-goals, required checks, allowed expansion triggers, and validation evidence. Limit repair review to accepted findings from the previous review plus contract regressions; route unrelated new comments as rejected/follow-up unless they match an allowed expansion trigger such as safety, authority-boundary, data-loss, security, live-mutation, public-API, migration, or operator-facing regression.\n- Classify review cost with `review_cost_control` and record `review_class = \"full_current_head_review\"` with a `fallback_reason`; repair verification, accepted findings, nonclean rounds, weak evidence, architecture risk, and high-risk surfaces are not compact-review eligible. Compact review is not review skipping and never removes the independent current-head checkpoint requirement.\n{route_guidance}- Validate reviewer comments before repair. Fix only accepted findings routed as `current_blocker`, keep the repair batch scoped to the smallest coherent owned set, rerun verification, and re-read `HEAD` before deciding the normalized review status.\n- Every time the Decodex Review pass produces a result for the current repaired committed head, call `{}` with reviewer `independent_fresh_context`, that normalized status, the exact current `HEAD` SHA, the explicit `review_contract`, `review_cost_control`, concise evidence, checklist notes, structured accepted/rejected findings, and `finding_routes`.\n",
			ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
			ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
			route_guidance = review_signal_route_guidance()
		),
	}
}

pub(super) fn build_handoff_completion_guidance(review_level: ReviewLevel) -> String {
	match review_level {
		ReviewLevel::Standard | ReviewLevel::Strict => format!(
			"- Call `{}` only after the latest `{}` for this handoff phase and current `HEAD` is `clean`. Then call `{}` with path `review_handoff`.\n",
			ISSUE_REVIEW_HANDOFF_TOOL_NAME,
			ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
			ISSUE_TERMINAL_FINALIZE_TOOL_NAME
		),
		ReviewLevel::Off | ReviewLevel::Basic => format!(
			"- Call `{}` after the branch is pushed, the non-draft PR is ready, and required validation has passed. Then call `{}` with path `review_handoff`.\n",
			ISSUE_REVIEW_HANDOFF_TOOL_NAME, ISSUE_TERMINAL_FINALIZE_TOOL_NAME
		),
	}
}

pub(super) fn build_repair_completion_guidance(review_level: ReviewLevel) -> String {
	match review_level {
		ReviewLevel::Standard | ReviewLevel::Strict => format!(
			"- Call `{}` only after the latest `{}` for this repair phase and current `HEAD` is `clean`. Then call `{}` with path `review_repair`.\n",
			ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME,
			ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
			ISSUE_TERMINAL_FINALIZE_TOOL_NAME
		),
		ReviewLevel::Off | ReviewLevel::Basic => format!(
			"- Call `{}` after the repaired head is pushed and required validation has passed. Then call `{}` with path `review_repair`.\n",
			ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME, ISSUE_TERMINAL_FINALIZE_TOOL_NAME
		),
	}
}

pub(super) fn build_handoff_continuation_review_guidance(review_level: ReviewLevel) -> String {
	match review_level {
		ReviewLevel::Off => format!(
			"- `[codex].review = \"off\"` for this project, so continue without Self Check or Decodex Review and do not call `{}`.\n",
			ISSUE_REVIEW_CHECKPOINT_TOOL_NAME
		),
		ReviewLevel::Basic => format!("- Self Check: {SELF_CHECK_INSTRUCTION}\n"),
		ReviewLevel::Standard | ReviewLevel::Strict => format!(
			"- Resume by committing any review-blocking lane edits, rerunning required validation, and requesting a Decodex Review pass for the actual committed branch state; the reviewer must not edit files, push, land, or mutate tracker state.\n- Use the registered project workflow policy injected above as the authoritative source, not a repo-local `WORKFLOW.md`; include an explicit `review_contract` with `workflow_policy_source = \"registered_project_workflow\"` and `review_type = \"full_current_head_review\"`.\n- Include `review_cost_control`: use `compact_current_head_review` only for low-risk small current-head, validation-backed, clean handoff review after intended-behavior and adversarial checks; otherwise use `full_current_head_review` with `fallback_reason`. Compact review is not review skipping.\n{route_guidance}- Apply the contract-bounded review method, validate comments before repair, fix only accepted findings routed as `current_blocker`, rerun verification, and re-read `HEAD` before deciding the normalized review status.\n- After each Decodex Review result for the current committed head, call `{}` with reviewer `independent_fresh_context`, the normalized status, current `HEAD` SHA, `review_contract`, `review_cost_control`, checklist notes, structured accepted/rejected findings, and `finding_routes`.\n",
			ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
			route_guidance = review_signal_route_guidance()
		),
	}
}

pub(super) fn build_repair_continuation_review_guidance(review_level: ReviewLevel) -> String {
	match review_level {
		ReviewLevel::Off => format!(
			"- `[codex].review = \"off\"` for this project, so continue without Self Check or Decodex Review and do not call `{}`.\n",
			ISSUE_REVIEW_CHECKPOINT_TOOL_NAME
		),
		ReviewLevel::Basic => format!("- Self Check: {SELF_CHECK_INSTRUCTION}\n"),
		ReviewLevel::Standard | ReviewLevel::Strict => format!(
			"- Resume by committing any review-blocking repair edits, rerunning required validation, and requesting a Decodex Review verification pass for the actual committed repaired branch state; the reviewer must not edit files, push, land, or mutate tracker state.\n- Use the registered project workflow policy injected above as the authoritative source, not a repo-local `WORKFLOW.md`; include an explicit `review_contract` with `workflow_policy_source = \"registered_project_workflow\"` and `review_type = \"repair_verification\"`.\n- Include `review_cost_control` with `review_class = \"full_current_head_review\"` and a `fallback_reason`; repair verification, accepted findings, nonclean rounds, weak evidence, architecture risk, and high-risk surfaces are not compact-review eligible.\n- Limit the review to accepted findings from the previous review plus contract regressions; route unrelated new comments as rejected/follow-up unless they match an allowed expansion trigger.\n{route_guidance}- Validate comments before repair, fix only accepted findings routed as `current_blocker`, rerun verification, and re-read `HEAD` before deciding the normalized review status.\n- After each Decodex Review result for the repaired committed head, call `{}` with reviewer `independent_fresh_context`, the normalized status, current `HEAD` SHA, `review_contract`, `review_cost_control`, checklist notes, structured accepted/rejected findings, and `finding_routes`.\n",
			ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
			route_guidance = review_signal_route_guidance()
		),
	}
}

pub(super) fn build_handoff_continuation_completion_guidance(
	review_level: ReviewLevel,
	pr_title: &str,
) -> String {
	match review_level {
		ReviewLevel::Standard | ReviewLevel::Strict => format!(
			"- If the implementation is review-ready, ensure the non-draft PR title is `{pr_title}` and finish the PR-backed tracker handoff only after the latest `{}` for the current `HEAD` is `clean`.\n",
			ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
		),
		ReviewLevel::Off | ReviewLevel::Basic => format!(
			"- If the implementation is review-ready, ensure the non-draft PR title is `{pr_title}` and finish the PR-backed tracker handoff after required validation has passed.\n",
		),
	}
}

pub(super) fn build_repair_continuation_completion_guidance(review_level: ReviewLevel) -> String {
	match review_level {
		ReviewLevel::Standard | ReviewLevel::Strict => format!(
			"- Call `{}` only after the latest `{}` for the current `HEAD` is `clean`, and then call `{}` with path `review_repair`.\n",
			ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME,
			ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
			ISSUE_TERMINAL_FINALIZE_TOOL_NAME
		),
		ReviewLevel::Off | ReviewLevel::Basic => format!(
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

fn review_signal_route_guidance() -> &'static str {
	"- Adjudicate every reviewer signal into `finding_routes` before repair: accepted current repair work must route to `current_blocker`; requests for evidence, follow-up, risk notes, reviewer rubric gaps, architecture signals, issue-contract gaps, landing blockers, and authority blockers must use the matching non-current or landing-blocking route.\n- Preserve reviewer and agent judgment: the reviewer may accept, reject, request evidence, mark follow-up/risk/rubric gaps, or identify architecture/authority blockers, but the runtime must receive structured route evidence before any repair loop uses the signal.\n- Non-current `finding_routes` such as `follow_up`, `risk_note`, `reviewer_rubric_gap`, and `invalid_or_unsubstantiated` are durable evidence and must not drive repair churn.\n"
}
