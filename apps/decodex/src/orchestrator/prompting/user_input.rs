use crate::orchestrator::{
	ISSUE_DELIVERY_CLOSEOUT_COMPLETE_TOOL_NAME, ISSUE_LABEL_ADD_TOOL_NAME,
	ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME, ISSUE_REVIEW_HANDOFF_TOOL_NAME,
	ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME, ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
	ISSUE_TRANSITION_TOOL_NAME, IssueDispatchMode, IssueRunPlan, IssueTracker, ReviewLevel,
	ServiceConfig, StateStore, TrackerIssue, WorkflowDocument, dispatch_policy,
	prompting::{
		prompting_contracts, prompting_recovery, prompting_review_guidance,
		prompting_workflow_context,
	},
};

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
	let description = dispatch_policy::render_issue_description_for_prompt(issue);
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
			pr_url = recorded_pr_url.unwrap_or("(missing review lifecycle handoff fixture)"),
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
			pr_url = recorded_pr_url.unwrap_or("(missing review lifecycle handoff fixture)"),
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
			pr_title = prompting_workflow_context::review_pull_request_title(issue),
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
			pr_url = recorded_pr_url.unwrap_or("(missing review lifecycle handoff fixture)"),
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
			pr_url = recorded_pr_url.unwrap_or("(missing review lifecycle handoff fixture)"),
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
					&prompting_workflow_context::review_pull_request_title(issue),
				),
		),
	}
}
