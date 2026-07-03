mod prompting_contracts;
mod prompting_recovery;
mod prompting_review_context;
mod prompting_review_guidance;
mod prompting_workflow_context;

use crate::orchestrator::{
	self, ISSUE_DELIVERY_CLOSEOUT_COMPLETE_TOOL_NAME, ISSUE_LABEL_ADD_TOOL_NAME,
	ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME, ISSUE_REVIEW_HANDOFF_TOOL_NAME,
	ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME, ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
	ISSUE_TRANSITION_TOOL_NAME, IssueDispatchMode, IssueRunPlan, IssueTracker, Result,
	ReviewHandoffContext, ReviewLevel, ServiceConfig, StateStore, TrackerIssue, WorkflowDocument,
};
pub(crate) const TRACKER_PUBLIC_TEXT_BOUNDARY_INSTRUCTION: &str =
	prompting_contracts::TRACKER_PUBLIC_TEXT_BOUNDARY_INSTRUCTION;
pub(crate) const DOCS_IMPACT_CONTRACT: &str = prompting_contracts::DOCS_IMPACT_CONTRACT;

pub(crate) fn build_review_run_context(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
) -> Result<ReviewHandoffContext> {
	prompting_review_context::build_review_run_context(project, state_store, issue_run)
}

pub(crate) fn review_pull_request_title(issue: &TrackerIssue) -> String {
	prompting_workflow_context::review_pull_request_title(issue)
}

pub(crate) fn validate_workflow_read_first_files(
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
) -> Result<()> {
	prompting_workflow_context::validate_workflow_read_first_files(project, workflow)
}

pub(crate) fn build_developer_instructions<T>(
	_tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	issue_run: &IssueRunPlan,
	state_store: &StateStore,
	recorded_pr_url: Option<&str>,
) -> Result<String>
where
	T: IssueTracker + ?Sized,
{
	let continuation_guidance = if prompting_contracts::allows_clean_continuation(
		workflow,
		issue_run.dispatch_mode,
	) {
		"\n- If more implementation work still remains at the current turn boundary, you may end the turn without `{terminal_finalize_tool}` and `decodex` may continue the same lane in a later turn."
	} else {
		""
	};
	let mut sections = Vec::new();

	push_developer_instruction_base_sections(&mut sections, project, workflow)?;

	if let Some(recovery_context) =
		prompting_recovery::build_retry_recovery_context(issue_run.dispatch_mode)
	{
		sections.push(recovery_context);
	}
	if let Some(recovery_context) =
		prompting_recovery::build_architecture_recovery_context(project, state_store, issue_run)
	{
		sections.push(recovery_context);
	}

	let repair_architecture_guidance =
		prompting_recovery::build_external_repair_architecture_guidance(
			project,
			state_store,
			issue_run,
		);
	let completed_state = workflow.frontmatter().tracker().resolved_completed_state();
	let review_level = project.codex().review_level();
	let needs_attention = workflow.frontmatter().tracker().needs_attention_label();
	let repair_manual_attention_guidance = prompting_contracts::build_manual_attention_guidance(
		needs_attention,
		ISSUE_LABEL_ADD_TOOL_NAME,
		ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
		None,
	);
	let handoff_manual_attention_guidance = prompting_contracts::build_manual_attention_guidance(
		needs_attention,
		ISSUE_LABEL_ADD_TOOL_NAME,
		ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
		Some(ISSUE_REVIEW_HANDOFF_TOOL_NAME),
	);
	let tracker_contract = match issue_run.dispatch_mode {
		IssueDispatchMode::ReviewRepair => format!(
			"Tracker tool contract\n- You own issue-scoped tracker writes for `{issue}` on retained PR `{pr_url}`.\n- This run resumes an existing `{success}` lane. Do not move the issue back to `{in_progress}` and do not call `{review_handoff_tool}`.\n- Update `{progress_checkpoint_tool}` whenever the execution phase, docs impact, focus, next action, blockers, evidence, or verification state changes materially.\n{decodex_review_guidance}- For each actionable review item on `{pr_url}`, including non-thread review summaries, validate the claim against the codebase, tests, and requirements before changing code, and keep pushback or clarification threads open until the repaired head is ready.\n- If this run was triggered by retained landing fallback, handle only the implementation-shaped blocker such as branch sync, conflict resolution, ambiguous mergeability, or repository-specific recovery. Do not merge or land the PR yourself.\n{repair_architecture_guidance}- Repair the current PR head on branch `{branch}`, run the repository validation needed to justify the repaired head, and push the repaired head.\n- Treat repo-native `canonicalize_commands` and `verify_commands` failures as continued repair: keep fixing the lane and rerun the gate. If the repo gate completes but leaves tracked rewrites, do not infer file semantics or widen scope; leave the retained worktree for operator review unless the issue-owned fix already makes the gate clean.\n{github_review_guidance}- After the repaired head is pushed, reply in-thread for every addressed comment and resolve only the GitHub review threads whose fixes landed and verified on the repaired head.\n{completion_guidance}- {manual_attention_guidance}\n{retained_tail_guidance}- Do not report the run as complete or treat `{progress_checkpoint_tool}` as terminal completion until `{terminal_finalize_tool}` succeeds.{continuation_guidance}\n- Never write to any other issue.",
			issue = issue_run.issue.identifier,
			pr_url = recorded_pr_url.unwrap_or("(missing review handoff marker)"),
			progress_checkpoint_tool = ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME,
			review_handoff_tool = ISSUE_REVIEW_HANDOFF_TOOL_NAME,
			terminal_finalize_tool = ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
			in_progress = workflow.frontmatter().tracker().in_progress_state(),
			success = workflow.frontmatter().tracker().success_state(),
			branch = issue_run.worktree.branch_name,
			manual_attention_guidance = repair_manual_attention_guidance,
			continuation_guidance = continuation_guidance,
			repair_architecture_guidance = repair_architecture_guidance,
			decodex_review_guidance =
				prompting_review_guidance::build_repair_review_guidance(review_level),
			github_review_guidance = prompting_review_guidance::build_repair_github_review_guidance(
				review_level,
				ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME,
			),
			retained_tail_guidance = prompting_review_guidance::build_repair_retained_tail_guidance(
				review_level,
				workflow.frontmatter().tracker().success_state(),
			),
			completion_guidance =
				prompting_review_guidance::build_repair_completion_guidance(review_level),
		),
		IssueDispatchMode::Closeout => format!(
			"Tracker tool contract\n- You own issue-scoped tracker writes for `{issue}` on retained PR `{pr_url}`.\n- This run resumes a merged post-review lane for the same PR lineage. The tracker issue may still be in `{success}` or may already be in `{completed}` while deterministic closeout tail work remains. Do not move the issue back to `{in_progress}` and do not call `{review_handoff_tool}` or `{review_repair_tool}`.\n- Treat retained closeout as a short deterministic tail. Reuse the existing merged PR evidence instead of restarting broad discovery, and only rerun the minimum validation needed to justify `Done` plus cleanup.\n- Update `{progress_checkpoint_tool}` whenever the execution phase, docs impact, focus, next action, blockers, evidence, or verification state changes materially.\n- If you call `{progress_checkpoint_tool}` during closeout, either omit `head_sha` and let `decodex` record the exact current lane HEAD automatically, or pass the exact full current `HEAD` SHA. Do not send an abbreviated SHA that differs from the live lane head.\n- Merge is already authoritative for `{pr_url}` before this run starts. Do not land, merge, or request review from this closeout run.\n- If the issue is still in `{success}`, transition it once to `{completed}` with `{transition_tool}` before `{closeout_tool}`. If it is already in `{completed}`, leave it there.\n- Finish the remaining Linear closeout tail work for this same merged PR lineage, then call `{closeout_tool}` with PR `{pr_url}` and a short result summary, then call `{terminal_finalize_tool}` with path `closeout`.\n- Do not end the turn without either `{closeout_tool}` plus `{terminal_finalize_tool}`, or the manual-attention path.\n- {manual_attention_guidance}\n- Keep all tracker and PR writes scoped to this retained lane. `decodex` will validate the merged PR lineage, the resolved completed state, and the later cleanup boundary.\n- Do not report the run as complete or treat `{progress_checkpoint_tool}` as terminal completion until `{terminal_finalize_tool}` succeeds.{continuation_guidance}\n- Never write to any other issue.",
			issue = issue_run.issue.identifier,
			pr_url = recorded_pr_url.unwrap_or("(missing review handoff marker)"),
			progress_checkpoint_tool = ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME,
			transition_tool = ISSUE_TRANSITION_TOOL_NAME,
			review_handoff_tool = ISSUE_REVIEW_HANDOFF_TOOL_NAME,
			review_repair_tool = ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME,
			closeout_tool = ISSUE_DELIVERY_CLOSEOUT_COMPLETE_TOOL_NAME,
			terminal_finalize_tool = ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
			in_progress = workflow.frontmatter().tracker().in_progress_state(),
			success = workflow.frontmatter().tracker().success_state(),
			completed = completed_state,
			manual_attention_guidance = repair_manual_attention_guidance,
			continuation_guidance = continuation_guidance,
		),
		_ => format!(
			"Tracker tool contract\n- You own issue-scoped tracker writes for `{issue}`.\n- At the start of execution, call `{transition_tool}` to move the issue to `{in_progress}`. Decodex already records the run-start Linear ledger, so do not add a separate start comment.\n- Update `{progress_checkpoint_tool}` whenever the execution phase, docs impact, focus, next action, blockers, evidence, or verification state changes materially.\n{decodex_review_guidance}- Treat repo-native `canonicalize_commands` and `verify_commands` failures as continued repair: keep fixing the lane and rerun the gate. If the repo gate completes but leaves tracked rewrites, do not infer file semantics or widen scope; leave the retained worktree for operator review unless the issue-owned fix already makes the gate clean.\n- When the implementation is ready, commit the lane, push branch `{branch}`, and create or update a non-draft PR titled `{pr_title}` for that branch.\n{completion_guidance}- {manual_attention_guidance}\n- Do not move the issue directly to `{success}` with `{transition_tool}`. `decodex` will complete the success writeback only after its own validation passes.\n- Do not report the run as complete or treat `{progress_checkpoint_tool}` as terminal completion until `{terminal_finalize_tool}` succeeds.{continuation_guidance}\n- Never write to any other issue.",
			issue = issue_run.issue.identifier,
			transition_tool = ISSUE_TRANSITION_TOOL_NAME,
			progress_checkpoint_tool = ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME,
			terminal_finalize_tool = ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
			in_progress = workflow.frontmatter().tracker().in_progress_state(),
			branch = issue_run.worktree.branch_name,
			success = workflow.frontmatter().tracker().success_state(),
			manual_attention_guidance = handoff_manual_attention_guidance,
			continuation_guidance = continuation_guidance,
			pr_title = review_pull_request_title(&issue_run.issue),
			decodex_review_guidance =
				prompting_review_guidance::build_handoff_review_guidance(review_level),
			completion_guidance =
				prompting_review_guidance::build_handoff_completion_guidance(review_level),
		),
	};

	sections.push(tracker_contract);

	Ok(sections.join("\n\n"))
}

pub(crate) fn build_user_input<T>(
	_tracker: &T,
	project: &ServiceConfig,
	issue: &TrackerIssue,
	workflow: &WorkflowDocument,
	issue_run: &IssueRunPlan,
	state_store: &StateStore,
	recorded_pr_url: Option<&str>,
) -> String
where
	T: IssueTracker + ?Sized,
{
	let continuation_guidance = if prompting_contracts::allows_clean_continuation(
		workflow,
		issue_run.dispatch_mode,
	) {
		"\n- If more work still remains at the current turn boundary, you may end the turn without `{terminal_finalize_tool}` and `decodex` will decide whether to continue the lane."
	} else {
		""
	};
	let description = orchestrator::render_issue_description_for_prompt(issue);
	let repair_architecture_guidance =
		prompting_recovery::build_external_repair_architecture_guidance(
			project,
			state_store,
			issue_run,
		);
	let completed_state = workflow.frontmatter().tracker().resolved_completed_state();
	let review_level = project.codex().review_level();
	let needs_attention = workflow.frontmatter().tracker().needs_attention_label();
	let repair_manual_attention_guidance = prompting_contracts::build_manual_attention_guidance(
		needs_attention,
		ISSUE_LABEL_ADD_TOOL_NAME,
		ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
		None,
	);
	let handoff_manual_attention_guidance = prompting_contracts::build_manual_attention_guidance(
		needs_attention,
		ISSUE_LABEL_ADD_TOOL_NAME,
		ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
		Some(ISSUE_REVIEW_HANDOFF_TOOL_NAME),
	);
	let recovery_context =
		prompting_recovery::build_retry_recovery_context(issue_run.dispatch_mode)
			.into_iter()
			.chain(prompting_recovery::build_architecture_recovery_context(
				project,
				state_store,
				issue_run,
			))
			.map(|section| format!("{section}\n\n"))
			.collect::<String>();

	match issue_run.dispatch_mode {
		IssueDispatchMode::ReviewRepair => format!(
			"Continue retained review repair for Linear issue {identifier}: {title}\n\nDescription:\n{description}\n\nCurrent PR:\n- `{pr_url}`\n\nExecution checklist:\n- Resume from the current branch and PR state in this worktree. Do not move the issue back to `{in_progress}`.\n- Update `{progress_checkpoint_tool}` whenever the execution phase, docs impact, focus, next action, blockers, evidence, or verification state changes materially.\n{decodex_review_guidance}- Read the current review feedback on `{pr_url}`, including non-thread review summaries, validate each actionable claim against the codebase, tests, and requirements, fix only the verified issues on branch `{branch}`, and keep scope limited to the outstanding retained repair.\n- If the lane is here because retained landing was not a deterministic clean path, handle only the branch sync, conflict resolution, ambiguous mergeability, or repository-specific recovery needed to make the PR clean again. Do not merge or land the PR yourself.\n- Leave pushback or clarification threads open until the repaired head is ready.\n{repair_architecture_guidance}- Treat repo-native `canonicalize_commands` and `verify_commands` failures as continued repair: keep fixing the lane and rerun the gate. If the repo gate completes but leaves tracked rewrites, do not infer file semantics or widen scope; leave the retained worktree for operator review unless the issue-owned fix already makes the gate clean.\n- Run the repository validation needed to justify the repaired head.\n- Commit the repair and push the same branch.\n{github_review_guidance}- After the repaired head is pushed, reply in-thread for every addressed comment and resolve only the GitHub review threads whose fixes landed and verified on the repaired head.\n{completion_guidance}- {manual_attention_guidance}\n- Keep the issue in `{success}` and do not treat `{progress_checkpoint_tool}` as terminal completion until `{terminal_finalize_tool}` succeeds.{continuation_guidance}",
			identifier = issue.identifier,
			title = issue.title,
			description = description,
			pr_url = recorded_pr_url.unwrap_or("(missing review handoff marker)"),
			in_progress = workflow.frontmatter().tracker().in_progress_state(),
			branch = issue_run.worktree.branch_name,
			progress_checkpoint_tool = ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME,
			terminal_finalize_tool = ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
			manual_attention_guidance = repair_manual_attention_guidance,
			success = workflow.frontmatter().tracker().success_state(),
			continuation_guidance = continuation_guidance,
			repair_architecture_guidance = repair_architecture_guidance,
			decodex_review_guidance =
				prompting_review_guidance::build_repair_review_guidance(review_level),
			github_review_guidance = prompting_review_guidance::build_repair_github_review_guidance(
				review_level,
				ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME,
			),
			completion_guidance =
				prompting_review_guidance::build_repair_completion_guidance(review_level),
		),
		IssueDispatchMode::Closeout => format!(
			"Continue retained closeout for Linear issue {identifier}: {title}\n\nDescription:\n{description}\n\nCurrent PR:\n- `{pr_url}`\n\nExecution checklist:\n- Resume from the current branch and merged PR lineage in this worktree. Do not move the issue back to `{in_progress}`.\n- Treat retained closeout as a short deterministic tail. Reuse the existing merged PR evidence instead of restarting broad discovery, and only rerun the minimum validation needed to justify `Done` plus cleanup.\n- Update `{progress_checkpoint_tool}` whenever the execution phase, docs impact, focus, next action, blockers, evidence, or verification state changes materially.\n- If you call `{progress_checkpoint_tool}` during closeout, either omit `head_sha` and let `decodex` record the exact current lane HEAD automatically, or pass the exact full current `HEAD` SHA.\n- Merge is already authoritative for `{pr_url}` before this run starts. Do not land, merge, or request review from this closeout run.\n- The tracker issue may already be in `{completed}` while this deterministic tail work remains pending.\n- If the issue is still in `{success}`, move it once to `{completed}` with `{transition_tool}` before `{closeout_tool}`.\n- Call `{closeout_tool}` with `{pr_url}` and a short result summary, then call `{terminal_finalize_tool}` with path `closeout`.\n- Do not end the turn without either `{closeout_tool}` plus `{terminal_finalize_tool}`, or the manual-attention path.\n- {manual_attention_guidance}\n- Keep the lane scoped to this retained post-review work and do not treat `{progress_checkpoint_tool}` as terminal completion until `{terminal_finalize_tool}` succeeds.{continuation_guidance}",
			identifier = issue.identifier,
			title = issue.title,
			description = description,
			pr_url = recorded_pr_url.unwrap_or("(missing review handoff marker)"),
			in_progress = workflow.frontmatter().tracker().in_progress_state(),
			progress_checkpoint_tool = ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME,
			transition_tool = ISSUE_TRANSITION_TOOL_NAME,
			closeout_tool = ISSUE_DELIVERY_CLOSEOUT_COMPLETE_TOOL_NAME,
			terminal_finalize_tool = ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
			success = workflow.frontmatter().tracker().success_state(),
			completed = completed_state,
			manual_attention_guidance = repair_manual_attention_guidance,
			continuation_guidance = continuation_guidance,
		),
		_ => format!(
			"Resolve Linear issue {identifier}: {title}\n\nDescription:\n{description}\n\n{recovery_context}Execution checklist:\n- Move the issue to `{in_progress}` with `{transition_tool}`. Decodex already records the run-start Linear ledger, so do not leave a separate start comment.\n- Update `{progress_checkpoint_tool}` whenever the execution phase, docs impact, focus, next action, blockers, evidence, or verification state changes materially.\n- Keep discovery bounded to the minimal implementation files needed for this issue; defer broader docs or upstream reading unless a concrete ambiguity blocks the change.\n- Implement the fix in the current worktree.\n{decodex_review_guidance}- Treat repo-native `canonicalize_commands` and `verify_commands` failures as continued repair: keep fixing the lane and rerun the gate. If the repo gate completes but leaves tracked rewrites, do not infer file semantics or widen scope; leave the retained worktree for operator review unless the issue-owned fix already makes the gate clean.\n- Run the repository validation needed to justify a reviewable PR.\n- Commit the lane, push branch `{branch}`, and create or update a non-draft PR titled `{pr_title}` for that branch.\n{completion_guidance}- {manual_attention_guidance}\n- Do not move the issue directly to `{success}` with `{transition_tool}`; `decodex` will finish that writeback after its own validation passes.\n- Do not report the run as complete or treat `{progress_checkpoint_tool}` as terminal completion until `{terminal_finalize_tool}` succeeds.{continuation_guidance}",
			identifier = issue.identifier,
			title = issue.title,
			description = description,
			recovery_context = recovery_context,
			transition_tool = ISSUE_TRANSITION_TOOL_NAME,
			progress_checkpoint_tool = ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME,
			terminal_finalize_tool = ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
			in_progress = workflow.frontmatter().tracker().in_progress_state(),
			branch = issue_run.worktree.branch_name,
			success = workflow.frontmatter().tracker().success_state(),
			manual_attention_guidance = handoff_manual_attention_guidance,
			continuation_guidance = continuation_guidance,
			pr_title = review_pull_request_title(issue),
			decodex_review_guidance =
				prompting_review_guidance::build_handoff_review_guidance(review_level),
			completion_guidance =
				prompting_review_guidance::build_handoff_completion_guidance(review_level),
		),
	}
}

pub(crate) fn build_continuation_user_input(
	issue: &TrackerIssue,
	workflow: &WorkflowDocument,
	dispatch_mode: IssueDispatchMode,
	recorded_pr_url: Option<&str>,
	success_state: &str,
	review_level: ReviewLevel,
) -> String {
	let completed_state = workflow.frontmatter().tracker().resolved_completed_state();
	let needs_attention = workflow.frontmatter().tracker().needs_attention_label();
	let repair_manual_attention_guidance = prompting_contracts::build_manual_attention_guidance(
		needs_attention,
		ISSUE_LABEL_ADD_TOOL_NAME,
		ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
		None,
	);
	let handoff_manual_attention_guidance = prompting_contracts::build_manual_attention_guidance(
		needs_attention,
		ISSUE_LABEL_ADD_TOOL_NAME,
		ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
		Some(ISSUE_REVIEW_HANDOFF_TOOL_NAME),
	);

	match dispatch_mode {
		IssueDispatchMode::ReviewRepair => format!(
			"Continue retained review repair for Linear issue {identifier} in the current thread and worktree.\n\nContinuation checklist:\n- Resume from the current repository state and outstanding review feedback or retained landing fallback on `{pr_url}`.\n- Keep changes scoped to the same retained review lane and do not move the issue out of `{success}`.\n- Record a current-HEAD `{progress_checkpoint_tool}` with `docs_impact` before claiming the repaired head is ready or taking a terminal path.\n{decodex_review_guidance}- Validate each actionable review claim against the codebase, tests, and requirements before changing code, and keep pushback or clarification threads open until the repaired head is ready.\n- If the blocker is landing fallback, repair only the branch sync, conflict, ambiguous mergeability, or repository-specific recovery issue; do not merge or land the PR yourself.\n- Treat repo-native `canonicalize_commands` and `verify_commands` failures as continued repair: keep fixing the lane and rerun the gate. If the repo gate completes but leaves tracked rewrites, do not infer file semantics or widen scope; leave the retained worktree for operator review unless the issue-owned fix already makes the gate clean.\n- If the repaired head is ready, push it.\n{github_review_guidance}- After the repaired head is pushed, reply in-thread for every addressed comment and resolve only the GitHub review threads whose fixes landed and verified on the repaired head.\n{completion_guidance}- {manual_attention_guidance}\n- If more work still remains after this turn, you may end the turn without terminal finalization and Decodex will decide whether to continue.",
			identifier = issue.identifier,
			pr_url = recorded_pr_url.unwrap_or("(missing review handoff marker)"),
			progress_checkpoint_tool = ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME,
			success = success_state,
			manual_attention_guidance = repair_manual_attention_guidance,
			github_review_guidance = prompting_review_guidance::build_repair_github_review_guidance(
				review_level,
				ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME,
			),
			decodex_review_guidance =
				prompting_review_guidance::build_repair_continuation_review_guidance(review_level),
			completion_guidance =
				prompting_review_guidance::build_repair_continuation_completion_guidance(
					review_level
				),
		),
		IssueDispatchMode::Closeout => format!(
			"Continue retained closeout for Linear issue {identifier} in the current thread and worktree.\n\nContinuation checklist:\n- Resume from the current repository state and merged PR lineage on `{pr_url}`.\n- Keep changes scoped to the same retained post-review lane. Do not move the issue back to implementation; the tracker may already be in `{completed}` while closeout or cleanup remains pending.\n- Treat this resumed closeout as a short deterministic tail. Reuse the existing merged PR evidence instead of restarting broad discovery, and only rerun the minimum validation needed to justify `Done` plus cleanup.\n- Record a current-HEAD `{progress_checkpoint_tool}` with `docs_impact`; either omit `head_sha` or pass the exact full current `HEAD` SHA.\n- Merge is already authoritative for `{pr_url}` before this run starts. Do not land, merge, or request review from this closeout run.\n- If the issue is still in `{success}`, transition it once to `{completed}` with `{transition_tool}` before `{closeout_tool}`.\n- If Linear closeout is complete, call `{closeout_tool}` and then call `{terminal_finalize_tool}` with path `closeout`.\n- Do not end the turn without either `{closeout_tool}` plus `{terminal_finalize_tool}`, or the manual-attention path.\n- {manual_attention_guidance}",
			identifier = issue.identifier,
			pr_url = recorded_pr_url.unwrap_or("(missing review handoff marker)"),
			progress_checkpoint_tool = ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME,
			transition_tool = ISSUE_TRANSITION_TOOL_NAME,
			success = success_state,
			completed = completed_state,
			closeout_tool = ISSUE_DELIVERY_CLOSEOUT_COMPLETE_TOOL_NAME,
			terminal_finalize_tool = ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
			manual_attention_guidance = repair_manual_attention_guidance,
		),
		_ => format!(
			"Continue working on Linear issue {identifier} in the current thread and worktree.\n\nContinuation checklist:\n- Resume from the current repository state instead of restarting broad discovery.\n- Keep changes scoped to the same issue lane.\n- Record a current-HEAD `{progress_checkpoint_tool}` with `docs_impact` before claiming the lane is ready or taking a terminal path.\n{decodex_review_guidance}- Treat repo-native `canonicalize_commands` and `verify_commands` failures as continued repair: keep fixing the lane and rerun the gate. If the repo gate completes but leaves tracked rewrites, do not infer file semantics or widen scope; leave the retained worktree for operator review unless the issue-owned fix already makes the gate clean.\n{completion_guidance}- {manual_attention_guidance}\n- If more work still remains after this turn, you may end the turn without terminal finalization and Decodex will decide whether to continue.",
			identifier = issue.identifier,
			progress_checkpoint_tool = ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME,
			manual_attention_guidance = handoff_manual_attention_guidance,
			decodex_review_guidance =
				prompting_review_guidance::build_handoff_continuation_review_guidance(review_level),
			completion_guidance =
				prompting_review_guidance::build_handoff_continuation_completion_guidance(
					review_level,
					&review_pull_request_title(issue),
				),
		),
	}
}

fn push_developer_instruction_base_sections(
	sections: &mut Vec<String>,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
) -> Result<()> {
	if !workflow.body().trim().is_empty() {
		sections.push(format!("Workflow policy\n{}", workflow.body()));
	}

	for relative_path in workflow.frontmatter().context().read_first() {
		let contents =
			prompting_workflow_context::read_workflow_read_first_file(project, relative_path)?;

		sections.push(format!("File: {relative_path}\n{contents}"));
	}

	sections.push(String::from(
		"Execution discipline\n- Keep pre-edit discovery bounded to the smallest code surface that can satisfy the current issue.\n- Start with the implementation files directly implicated by the issue before reading broader docs or repo-wide guidance.\n- Do not browse upstream references or general repository documentation unless a concrete ambiguity blocks the change.\n- Once the relevant change surface is identified, patch code and run validation instead of continuing broad searches.",
	));
	sections.push(String::from(DOCS_IMPACT_CONTRACT));
	sections.push(String::from(
		"Commit contract\n- When you create a local commit for this lane, use a single-line `decodex/commit/1` JSON commit message.\n- Required fields: `schema`, `summary`, and `authority`.\n- `authority` must be the authoritative Linear issue identifier for this lane.\n- Optional fields: `related` and `breaking`.\n- Do not encode landing mode, CI status, closeout state, or other process-state fields in the commit message.",
	));
	sections.push(prompting_contracts::build_phase_goal_runtime_contract());
	sections.push(String::from(TRACKER_PUBLIC_TEXT_BOUNDARY_INSTRUCTION));

	Ok(())
}
