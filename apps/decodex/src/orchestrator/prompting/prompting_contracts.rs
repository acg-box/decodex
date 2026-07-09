use crate::{
	agent::ISSUE_TERMINAL_FINALIZE_TOOL_NAME, orchestrator::IssueDispatchMode,
	workflow::WorkflowDocument,
};

pub(crate) const TRACKER_PUBLIC_TEXT_BOUNDARY_INSTRUCTION: &str = "Tracker public text boundary\n- Linear tracker text is public/team-visible. Do not include local host paths, routed identity details, account details, credential-like names, private config paths, tokens, or secrets in issue comments, progress checkpoints, review summaries, closeout summaries, blockers, evidence, verification, failed commands, or raw errors.\n- `issue_comment` accepts only allowlisted public comment kinds. For manual attention, call it with `kind: \"manual_attention\"` and structured public fields; do not send arbitrary comment bodies.\n- Use public collaboration identifiers when needed: PR URLs, issue identifiers, branch names, commit SHAs, and repository-relative paths.\n- Decodex may apply a local-only secondary privacy classifier to rendered public projections, but that classifier is not the privacy boundary; keep private evidence out of public fields before tool calls.";

pub(super) const DOCS_IMPACT_CONTRACT: &str = "OpenWiki impact contract\n- Before any terminal finalize path, classify OpenWiki impact as `none`, `update_required`, `research_required`, or `drift_required`, and record it in a current-HEAD `issue_progress_checkpoint` as `docs_impact` for compatibility.\n- Decodex records internal validation evidence for phase transitions; do not create checkpoint comments solely to satisfy phase ceremony.\n- If behavior, commands, config, schemas, status, validation, workflow, or project knowledge changed, update the owning Decodex page under `openwiki/` when the changed claim is Decodex product or runtime authority.\n- If authority is missing or contradictory, use `research_required` and route the investigation through the external installed team research workflow; only accepted results become OpenWiki updates.\n- Run the repository validation gate that matches the touched surface.\n- Treat touched-surface documentation or semantic-drift failure as a completion blocker. Fix issue-local failures in the lane. Do not route pre-existing, repo-wide, or global-baseline failures through `manual_attention`; record the blocker in private evidence and let Decodex retain or isolate the baseline lane.";

pub(super) fn build_manual_attention_guidance(
	needs_attention: &str,
	label_tool: &str,
	terminal_finalize_tool: &str,
	review_handoff_tool: Option<&str>,
) -> String {
	let review_handoff_guidance = review_handoff_tool
		.map(|tool| {
			format!(
				" Do not call `{tool}` in that case; `decodex` will stop the lane as a human-required failure without automatic retry."
			)
		})
		.unwrap_or_default();

	format!(
		"If you determine the issue needs human attention, request label `{needs_attention}` with `{label_tool}`; that records manual-attention label intent only. Then call `issue_comment` with kind `manual_attention` and structured public fields (`error_class`, `next_action`, `blockers`, `evidence`; include `failed_command` and `raw_error` only when public-safe). Decodex applies the actual label only after that manual_attention comment validates. Use a human-owned blocker class; do not use runtime-owned retry/repair classes such as app-server timeout, transport, turn, dynamic-tool, or usage-limit failures; stalled-run detection; phase-goal terminal-path misses; repo-gate canonicalize, verify, baseline, tracked-rewrite, or git-lock failures; or generic retryable execution failures. Then call `{terminal_finalize_tool}` with path `manual_attention`. Do not speculate about capabilities you did not directly verify.{review_handoff_guidance}"
	)
}

pub(super) fn build_phase_goal_runtime_contract() -> String {
	format!(
		"Phase goal runtime contract\n- Decodex may set an active phase goal that narrows the immediate turn below the full issue lifecycle checklist.\n- Treat the active phase goal as the authoritative current step, not as a checklist ceremony.\n- For `implement_to_validation_ready`, `repair_validation_failures`, and `repair_accepted_review_findings`, stop at coherent local work, then complete the active phase goal so Decodex can run its repo gate, record validation evidence, and select the next step.\n- The later `handoff_evidence` phase creates or updates the PR and records the normal review handoff terminal path.\n- The later `review_repair_evidence` phase pushes the retained PR repair head, records review-repair completion, and calls `{terminal_finalize_tool}` with path `review_repair`; it must not call `issue_review_handoff` or move the issue out of its retained review state.",
		terminal_finalize_tool = ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
	)
}

pub(super) fn allows_clean_continuation(
	workflow: &WorkflowDocument,
	dispatch_mode: IssueDispatchMode,
) -> bool {
	workflow.frontmatter().execution().max_turns() > 1
		&& dispatch_mode != IssueDispatchMode::Closeout
}
