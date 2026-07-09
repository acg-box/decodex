mod active_child_reconciliation_keeps_spawn_time_workflow_until_exit;
mod configured_cycle_workflow_snapshot_overrides_invalid_disk_workflow;
mod daemon_workflow_reload_keeps_last_known_good_on_same_path_failure;
mod daemon_workflow_reload_replaces_cached_document_after_valid_update;

use crate::{
	orchestrator::{
		ISSUE_LABEL_ADD_TOOL_NAME, ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME,
		ISSUE_REVIEW_HANDOFF_TOOL_NAME, ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
		ISSUE_TRANSITION_TOOL_NAME, IssueRunPlan, TRACKER_PUBLIC_TEXT_BOUNDARY_INSTRUCTION,
	},
	workflow::WorkflowDocument,
};

pub(super) fn expected_developer_instructions(
	read_first_files: &[(&str, &str)],
	workflow: &WorkflowDocument,
	issue_run: &IssueRunPlan,
) -> String {
	let continuation_guidance = if workflow.frontmatter().execution().max_turns() > 1 {
		"\n- If more implementation work still remains at the current turn boundary, you may end the turn without `{terminal_finalize_tool}` and `decodex` may continue the same lane in a later turn."
	} else {
		""
	};
	let mut sections = Vec::new();

	if !workflow.body().trim().is_empty() {
		sections.push(format!("Workflow policy\n{}", workflow.body()));
	}

	sections.push(format!(
		"Registered repo gate\n- `canonicalize_commands`: {}\n- `verify_commands`: {}\n- When Decodex prompts say required validation or repo gate, run these registered command lists in order. Do not substitute broader repo-documentation examples for this lane.",
		format_command_list(workflow.frontmatter().execution().canonicalize_commands()),
		format_command_list(workflow.frontmatter().execution().verify_commands())
	));

	sections.extend(
		read_first_files
			.iter()
			.map(|(relative_path, contents)| format!("File: {relative_path}\n{contents}")),
	);
	sections.push(String::from(
			"Execution discipline\n- Keep pre-edit discovery bounded to the smallest code surface that can satisfy the current issue.\n- Start with the implementation files directly implicated by the issue before reading broader OpenWiki or repo-wide guidance.\n- Do not browse upstream references or general repository documentation unless a concrete ambiguity blocks the change.\n- Once the relevant change surface is identified, patch code and run validation instead of continuing broad searches.",
		));
	sections.push(String::from(
				"OpenWiki impact contract\n- Before any terminal finalize path, classify OpenWiki impact as `none`, `update_required`, `research_required`, or `drift_required`, and record it in a current-HEAD `issue_progress_checkpoint` as `docs_impact` for compatibility.\n- Decodex records internal validation evidence for phase transitions; do not create checkpoint comments solely to satisfy phase ceremony.\n- If behavior, commands, config, schemas, status, validation, workflow, or project knowledge changed, update the owning Decodex page under `openwiki/` when the changed claim is Decodex product or runtime authority.\n- If authority is missing or contradictory, use `research_required` and route the investigation through the external installed team research workflow; only accepted results become OpenWiki updates.\n- Run the repository validation gate that matches the touched surface.\n- Treat touched-surface documentation or semantic-drift failure as a completion blocker. Fix issue-local failures in the lane. Do not route pre-existing, repo-wide, or global-baseline failures through `manual_attention`; record the blocker in private evidence and let Decodex retain or isolate the baseline lane.",
	));
	sections.push(String::from(
		"Commit contract\n- When you create a local commit for this lane, use a single-line `decodex/commit/2` JSON commit message.\n- Required fields: `schema`, `change`, `authority`, and `impact`.\n- `authority` must be the authoritative Linear issue identifier for this lane.\n- `impact` must be `compatible` or `breaking`.\n- Do not encode related issues, source branch, landing mode, PR state, closeout state, or other process-state fields in the commit message.",
	));
	sections.push(String::from(
			"Phase goal runtime contract\n- Decodex may set an active phase goal that narrows the immediate turn below the full issue lifecycle checklist.\n- Treat the active phase goal as the authoritative current step, not as a checklist ceremony.\n- For `implement_to_validation_ready`, `repair_validation_failures`, and `repair_accepted_review_findings`, stop at coherent local work, then complete the active phase goal so Decodex can run its repo gate, record validation evidence, and select the next step.\n- The later `handoff_evidence` phase creates or updates the PR and records the normal review handoff terminal path.\n- The later `review_repair_evidence` phase pushes the retained PR repair head, records review-repair completion, and calls `issue_terminal_finalize` with path `review_repair`; it must not call `issue_review_handoff` or move the issue out of its retained review state.",
		));
	sections.push(String::from(TRACKER_PUBLIC_TEXT_BOUNDARY_INSTRUCTION));

	let issue_title = issue_run.issue.title.trim();
	let issue_prefix = format!("{}:", issue_run.issue.identifier);
	let pr_title = if let Some(candidate_prefix) = issue_title.get(..issue_prefix.len())
		&& candidate_prefix.eq_ignore_ascii_case(&issue_prefix)
	{
		let summary = issue_title.get(issue_prefix.len()..).unwrap_or_default().trim();

		if summary.is_empty() {
			issue_run.issue.identifier.clone()
		} else {
			format!("{issue_prefix} {summary}")
		}
	} else {
		format!("{issue_prefix} {issue_title}")
	};

	sections.push(format!(
				"Tracker tool contract\n- You own issue-scoped tracker writes for `{issue}`.\n- At the start of execution, call `{transition_tool}` to move the issue to `{in_progress}`. Decodex already records the run-start Linear ledger, so do not add a separate start comment.\n- Use `{progress_checkpoint_tool}` only when OpenWiki impact or a blocker must be recorded before a terminal path; do not emit routine progress checkpoints.\n- Commit the lane work, rerun required validation, confirm review-blocking local changes are absent, push the branch, and prepare the non-draft PR for runtime review.\n- Do not request Decodex Review yourself and do not call `issue_review_checkpoint`. Decodex owns the independent current-head review request, checkpoint recording, finding routing, and post-review decision after PR-backed handoff succeeds.\n- Use the registered project workflow policy already injected above as the authoritative review policy source; do not look for or require a repo-local `WORKFLOW.md` unless it was explicitly listed in `context.read_first`.\n- Treat repo-native `canonicalize_commands` and `verify_commands` failures as continued repair: keep fixing the lane and rerun the gate. If the repo gate completes but leaves tracked rewrites, do not infer file semantics or widen scope; leave the retained worktree for operator review unless the issue-owned fix already makes the gate clean.\n- When the implementation is ready, commit the lane, push branch `{branch}`, and create or update a non-draft PR titled `{pr_title}` for that branch.\n- Call `{review_handoff_tool}` after the branch is pushed, the non-draft PR is ready, and required validation has passed. Decodex will run the runtime-owned review gate after handoff. Then call `{terminal_finalize_tool}` with path `review_handoff`.\n- If you determine the issue needs human attention, request label `{needs_attention}` with `{label_tool}`; that records manual-attention label intent only. Then call `issue_comment` with kind `manual_attention` and structured public fields (`error_class`, `next_action`, `blockers`, `evidence`; include `failed_command` and `raw_error` only when public-safe). Decodex applies the actual label only after that manual_attention comment validates. Use a human-owned blocker class; do not use runtime-owned retry/repair classes such as app-server timeout, transport, turn, dynamic-tool, or usage-limit failures; stalled-run detection; phase-goal terminal-path misses; repo-gate canonicalize, verify, baseline, tracked-rewrite, or git-lock failures; or generic retryable execution failures. Then call `{terminal_finalize_tool}` with path `manual_attention`. Do not speculate about capabilities you did not directly verify. Do not call `{review_handoff_tool}` in that case; `decodex` will stop the lane as a human-required failure without automatic retry.\n- Do not move the issue directly to `{success}` with `{transition_tool}`. `decodex` will complete the success writeback only after its own validation passes.\n- Do not report the run as complete or treat `{progress_checkpoint_tool}` as terminal completion until `{terminal_finalize_tool}` succeeds.{continuation_guidance}\n- Never write to any other issue.",
			issue = issue_run.issue.identifier,
			transition_tool = ISSUE_TRANSITION_TOOL_NAME,
			label_tool = ISSUE_LABEL_ADD_TOOL_NAME,
			progress_checkpoint_tool = ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME,
			review_handoff_tool = ISSUE_REVIEW_HANDOFF_TOOL_NAME,
			terminal_finalize_tool = ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
			in_progress = workflow.frontmatter().tracker().in_progress_state(),
			branch = issue_run.worktree.branch_name,
			pr_title = pr_title,
		success = workflow.frontmatter().tracker().success_state(),
		needs_attention = workflow.frontmatter().tracker().needs_attention_label(),
		continuation_guidance = continuation_guidance,
	));

	sections.join("\n\n")
}

fn format_command_list(commands: &[String]) -> String {
	if commands.is_empty() {
		String::from("[]")
	} else {
		commands.iter().map(|command| format!("`{command}`")).collect::<Vec<_>>().join(", ")
	}
}
