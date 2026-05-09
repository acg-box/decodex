const PROMPT_ONLY_INTERNAL_REVIEW_INSTRUCTION: &str =
	"Review your work repeatedly and fix any logic bugs until no new issues are found.";

fn review_pull_request_title(issue: &TrackerIssue) -> String {
	let title = issue.title.trim();
	let prefix = format!("{}:", issue.identifier);

	if let Some(candidate_prefix) = title.get(..prefix.len())
		&& candidate_prefix.eq_ignore_ascii_case(&prefix)
	{
		let summary = title.get(prefix.len()..).unwrap_or_default().trim();

		if summary.is_empty() {
			return issue.identifier.clone();
		}

		return format!("{prefix} {summary}");
	}

	format!("{prefix} {title}")
}

fn build_developer_instructions<T>(
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
	let continuation_guidance = if allows_clean_continuation(workflow, issue_run.dispatch_mode) {
		"\n- If more implementation work still remains at the current turn boundary, you may end the turn without `{terminal_finalize_tool}` and `decodex` may continue the same lane in a later turn."
	} else {
		""
	};
	let mut sections = Vec::new();

	if !workflow.body().trim().is_empty() {
		sections.push(format!("Workflow policy\n{}", workflow.body()));
	}

	for relative_path in workflow.frontmatter().context().read_first() {
		let absolute_path = project.repo_root().join(relative_path);
		let contents = fs::read_to_string(&absolute_path)?;

		sections.push(format!("File: {relative_path}\n{contents}"));
	}

	sections.push(String::from(
		"Execution discipline\n- Keep pre-edit discovery bounded to the smallest code surface that can satisfy the current issue.\n- Start with the implementation files directly implicated by the issue before reading broader docs or repo-wide guidance.\n- Do not browse upstream references or general repository documentation unless a concrete ambiguity blocks the change.\n- Once the relevant change surface is identified, patch code and run validation instead of continuing broad searches.",
	));
	sections.push(String::from(
		"Commit contract\n- When you create a local commit for this lane, use a single-line `decodex/commit/1` JSON commit message.\n- Required fields: `schema`, `summary`, and `authority`.\n- `authority` must be the authoritative Linear issue identifier for this lane.\n- Optional fields: `related` and `breaking`.\n- Do not encode landing mode, CI status, closeout state, or other process-state fields in the commit message.",
	));

	let repair_architecture_guidance =
		build_external_repair_architecture_guidance(project, state_store, issue_run);
	let completed_state = workflow
		.frontmatter()
		.tracker()
		.resolved_completed_state();
	let internal_review_mode = project.codex().internal_review_mode();
	let tracker_contract = match issue_run.dispatch_mode {
			IssueDispatchMode::ReviewRepair => format!(
				"Tracker tool contract\n- You own issue-scoped tracker writes for `{issue}` on retained PR `{pr_url}`.\n- This run resumes an existing `{success}` lane. Do not move the issue back to `{in_progress}` and do not call `{review_handoff_tool}`.\n- Update `{progress_checkpoint_tool}` whenever the execution phase, focus, next action, blockers, evidence, or verification state changes materially.\n{internal_review_guidance}- For each actionable review item on `{pr_url}`, including non-thread review summaries, validate the claim against the codebase, tests, and requirements before changing code, and keep pushback or clarification threads open until the repaired head is ready.\n- If this run was triggered by retained landing fallback, handle only the implementation-shaped blocker such as branch sync, conflict resolution, ambiguous mergeability, or repository-specific recovery. Do not merge or land the PR yourself.\n{repair_architecture_guidance}- Repair the current PR head on branch `{branch}`, run the repository validation needed to justify the repaired head, and push the repaired head.\n- Treat failures from repo-native `canonicalize_commands`, `verify_commands`, or tracked rewrites left by that repo gate as continued repair by default: keep fixing the lane and rerun the gate instead of taking `manual_attention` unless the blocker is clearly toolchain, environment, or operator-owned.\n- Do not request fresh external review yourself. `decodex` will post the next runtime-owned external review request after `{review_repair_tool}` succeeds.\n- After the repaired head is pushed, reply in-thread for every addressed comment and resolve only the GitHub review threads whose fixes landed and verified on the repaired head.\n{completion_guidance}- If you determine the issue needs human attention, add label `{needs_attention}` with `{label_tool}`, explain the exact observed blocker in a comment, including the failed command and raw error when available, and then call `{terminal_finalize_tool}` with path `manual_attention`. Do not speculate about capabilities you did not directly verify.\n- Keep the tracker issue in `{success}`. `decodex` will handle the later external review request or clean-path runtime landing, closeout, and cleanup lifecycle.\n- Do not report the run as complete or treat `{progress_checkpoint_tool}` as terminal completion until `{terminal_finalize_tool}` succeeds.{continuation_guidance}\n- Never write to any other issue.",
			issue = issue_run.issue.identifier,
			pr_url = recorded_pr_url.unwrap_or("(missing review handoff marker)"),
			progress_checkpoint_tool = ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME,
			review_handoff_tool = ISSUE_REVIEW_HANDOFF_TOOL_NAME,
			review_repair_tool = ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME,
			terminal_finalize_tool = ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
			in_progress = workflow.frontmatter().tracker().in_progress_state(),
			success = workflow.frontmatter().tracker().success_state(),
			branch = issue_run.worktree.branch_name,
			needs_attention = workflow.frontmatter().tracker().needs_attention_label(),
			label_tool = ISSUE_LABEL_ADD_TOOL_NAME,
			continuation_guidance = continuation_guidance,
			repair_architecture_guidance = repair_architecture_guidance,
			internal_review_guidance = build_repair_internal_review_guidance(internal_review_mode),
			completion_guidance = build_repair_completion_guidance(internal_review_mode),
		),
		IssueDispatchMode::Closeout => format!(
			"Tracker tool contract\n- You own issue-scoped tracker writes for `{issue}` on retained PR `{pr_url}`.\n- This run resumes a merged post-review lane for the same PR lineage. The tracker issue may still be in `{success}` or may already be in `{completed}` while deterministic closeout tail work remains. Do not move the issue back to `{in_progress}` and do not call `{review_handoff_tool}` or `{review_repair_tool}`.\n- Treat retained closeout as a short deterministic tail. Reuse the existing merged PR evidence instead of restarting broad discovery, and only rerun the minimum validation needed to justify `Done` plus cleanup.\n- Update `{progress_checkpoint_tool}` whenever the execution phase, focus, next action, blockers, evidence, or verification state changes materially.\n- If you call `{progress_checkpoint_tool}` during closeout, either omit `head_sha` and let `decodex` record the exact current lane HEAD automatically, or pass the exact full current `HEAD` SHA. Do not send an abbreviated SHA that differs from the live lane head.\n- Merge is already authoritative for `{pr_url}` before this run starts. Do not land, merge, or request review from this closeout run.\n- If the issue is still in `{success}`, transition it once to `{completed}` with `{transition_tool}` before `{closeout_tool}`. If it is already in `{completed}`, leave it there.\n- Finish the remaining Linear closeout tail work for this same merged PR lineage, then call `{closeout_tool}` with PR `{pr_url}` and a short result summary, then call `{terminal_finalize_tool}` with path `closeout`.\n- Do not end the turn without either `{closeout_tool}` plus `{terminal_finalize_tool}`, or the manual-attention path.\n- If you determine the issue needs human attention, add label `{needs_attention}` with `{label_tool}`, explain the exact observed blocker in a comment, including the failed command and raw error when available, and then call `{terminal_finalize_tool}` with path `manual_attention`. Do not speculate about capabilities you did not directly verify.\n- Keep all tracker and PR writes scoped to this retained lane. `decodex` will validate the merged PR lineage, the resolved completed state, and the later cleanup boundary.\n- Do not report the run as complete or treat `{progress_checkpoint_tool}` as terminal completion until `{terminal_finalize_tool}` succeeds.{continuation_guidance}\n- Never write to any other issue.",
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
			needs_attention = workflow.frontmatter().tracker().needs_attention_label(),
			label_tool = ISSUE_LABEL_ADD_TOOL_NAME,
			continuation_guidance = continuation_guidance,
		),
		_ => format!(
			"Tracker tool contract\n- You own issue-scoped tracker writes for `{issue}`.\n- At the start of execution, call `{transition_tool}` to move the issue to `{in_progress}` and add a brief `{comment_tool}` comment that you started work on run `{run_id}` attempt `{attempt}`.\n- Update `{progress_checkpoint_tool}` whenever the execution phase, focus, next action, blockers, evidence, or verification state changes materially.\n{internal_review_guidance}- Treat failures from repo-native `canonicalize_commands`, `verify_commands`, or tracked rewrites left by that repo gate as continued repair by default: keep fixing the lane and rerun the gate instead of taking `manual_attention` unless the blocker is clearly toolchain, environment, or operator-owned.\n- When the implementation is ready, commit the lane, push branch `{branch}`, and create or update a non-draft PR titled `{pr_title}` for that branch.\n{completion_guidance}- If you determine the issue needs human attention, add label `{needs_attention}` with `{label_tool}`, explain the exact observed blocker in a comment, including the failed command and raw error when available, and then call `{terminal_finalize_tool}` with path `manual_attention`. Do not speculate about capabilities you did not directly verify. Do not call `{review_handoff_tool}` in that case; `decodex` will stop the lane as a human-required failure without automatic retry.\n- Do not move the issue directly to `{success}` with `{transition_tool}`. `decodex` will complete the success writeback only after its own validation passes.\n- Do not report the run as complete or treat `{progress_checkpoint_tool}` as terminal completion until `{terminal_finalize_tool}` succeeds.{continuation_guidance}\n- Never write to any other issue.",
			issue = issue_run.issue.identifier,
			transition_tool = ISSUE_TRANSITION_TOOL_NAME,
			comment_tool = ISSUE_COMMENT_TOOL_NAME,
			label_tool = ISSUE_LABEL_ADD_TOOL_NAME,
			progress_checkpoint_tool = ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME,
			review_handoff_tool = ISSUE_REVIEW_HANDOFF_TOOL_NAME,
			terminal_finalize_tool = ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
			in_progress = workflow.frontmatter().tracker().in_progress_state(),
			run_id = issue_run.run_id,
			attempt = issue_run.attempt_number,
			branch = issue_run.worktree.branch_name,
			success = workflow.frontmatter().tracker().success_state(),
			needs_attention = workflow.frontmatter().tracker().needs_attention_label(),
			continuation_guidance = continuation_guidance,
			pr_title = review_pull_request_title(&issue_run.issue),
			internal_review_guidance = build_handoff_internal_review_guidance(
				internal_review_mode
			),
			completion_guidance = build_handoff_completion_guidance(internal_review_mode),
		),
	};

	sections.push(tracker_contract);

	Ok(sections.join("\n\n"))
}

fn build_user_input<T>(
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
	let continuation_guidance = if allows_clean_continuation(workflow, issue_run.dispatch_mode) {
		"\n- If more work still remains at the current turn boundary, you may end the turn without `{terminal_finalize_tool}` and `decodex` will decide whether to continue the lane."
	} else {
		""
	};
	let description = render_issue_description_for_prompt(issue);
	let repair_architecture_guidance =
		build_external_repair_architecture_guidance(project, state_store, issue_run);
	let completed_state = workflow
		.frontmatter()
		.tracker()
		.resolved_completed_state();
	let internal_review_mode = project.codex().internal_review_mode();

	match issue_run.dispatch_mode {
			IssueDispatchMode::ReviewRepair => format!(
				"Continue retained review repair for Linear issue {identifier}: {title}\n\nDescription:\n{description}\n\nCurrent PR:\n- `{pr_url}`\n\nExecution checklist:\n- Resume from the current branch and PR state in this worktree. Do not move the issue back to `{in_progress}`.\n- Update `{progress_checkpoint_tool}` whenever the execution phase, focus, next action, blockers, evidence, or verification state changes materially.\n{internal_review_guidance}- Read the current review feedback on `{pr_url}`, including non-thread review summaries, validate each actionable claim against the codebase, tests, and requirements, fix only the verified issues on branch `{branch}`, and keep scope limited to the outstanding retained repair.\n- If the lane is here because retained landing was not a deterministic clean path, handle only the branch sync, conflict resolution, ambiguous mergeability, or repository-specific recovery needed to make the PR clean again. Do not merge or land the PR yourself.\n- Leave pushback or clarification threads open until the repaired head is ready.\n{repair_architecture_guidance}- Treat failures from repo-native `canonicalize_commands`, `verify_commands`, or tracked rewrites left by that repo gate as continued repair by default: keep fixing the lane and rerun the gate instead of taking `manual_attention` unless the blocker is clearly toolchain, environment, or operator-owned.\n- Run the repository validation needed to justify the repaired head.\n- Commit the repair and push the same branch. Do not request fresh external review yourself; `decodex` will post the next runtime-owned external review request after `{review_repair_tool}` succeeds.\n- After the repaired head is pushed, reply in-thread for every addressed comment and resolve only the GitHub review threads whose fixes landed and verified on the repaired head.\n{completion_guidance}- If the issue needs manual attention, add label `{needs_attention}` with `{label_tool}`, explain why in a comment, and then call `{terminal_finalize_tool}` with path `manual_attention`.\n- Keep the issue in `{success}` and do not treat `{progress_checkpoint_tool}` as terminal completion until `{terminal_finalize_tool}` succeeds.{continuation_guidance}",
			identifier = issue.identifier,
			title = issue.title,
			description = description,
			pr_url = recorded_pr_url.unwrap_or("(missing review handoff marker)"),
			in_progress = workflow.frontmatter().tracker().in_progress_state(),
			branch = issue_run.worktree.branch_name,
			progress_checkpoint_tool = ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME,
			review_repair_tool = ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME,
			terminal_finalize_tool = ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
			needs_attention = workflow.frontmatter().tracker().needs_attention_label(),
			label_tool = ISSUE_LABEL_ADD_TOOL_NAME,
			success = workflow.frontmatter().tracker().success_state(),
			continuation_guidance = continuation_guidance,
			repair_architecture_guidance = repair_architecture_guidance,
			internal_review_guidance = build_repair_internal_review_guidance(internal_review_mode),
			completion_guidance = build_repair_completion_guidance(internal_review_mode),
		),
		IssueDispatchMode::Closeout => format!(
			"Continue retained closeout for Linear issue {identifier}: {title}\n\nDescription:\n{description}\n\nCurrent PR:\n- `{pr_url}`\n\nExecution checklist:\n- Resume from the current branch and merged PR lineage in this worktree. Do not move the issue back to `{in_progress}`.\n- Treat retained closeout as a short deterministic tail. Reuse the existing merged PR evidence instead of restarting broad discovery, and only rerun the minimum validation needed to justify `Done` plus cleanup.\n- Update `{progress_checkpoint_tool}` whenever the execution phase, focus, next action, blockers, evidence, or verification state changes materially.\n- If you call `{progress_checkpoint_tool}` during closeout, either omit `head_sha` and let `decodex` record the exact current lane HEAD automatically, or pass the exact full current `HEAD` SHA.\n- Merge is already authoritative for `{pr_url}` before this run starts. Do not land, merge, or request review from this closeout run.\n- The tracker issue may already be in `{completed}` while this deterministic tail work remains pending.\n- If the issue is still in `{success}`, move it once to `{completed}` with `{transition_tool}` before `{closeout_tool}`.\n- Call `{closeout_tool}` with `{pr_url}` and a short result summary, then call `{terminal_finalize_tool}` with path `closeout`.\n- Do not end the turn without either `{closeout_tool}` plus `{terminal_finalize_tool}`, or the manual-attention path.\n- If the issue needs manual attention, add label `{needs_attention}` with `{label_tool}`, explain why in a comment, and then call `{terminal_finalize_tool}` with path `manual_attention`.\n- Keep the lane scoped to this retained post-review work and do not treat `{progress_checkpoint_tool}` as terminal completion until `{terminal_finalize_tool}` succeeds.{continuation_guidance}",
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
			needs_attention = workflow.frontmatter().tracker().needs_attention_label(),
			label_tool = ISSUE_LABEL_ADD_TOOL_NAME,
			continuation_guidance = continuation_guidance,
		),
		_ => format!(
			"Resolve Linear issue {identifier}: {title}\n\nDescription:\n{description}\n\nExecution checklist:\n- Move the issue to `{in_progress}` with `{transition_tool}` and leave a short `{comment_tool}` comment that includes run `{run_id}` attempt `{attempt}`.\n- Update `{progress_checkpoint_tool}` whenever the execution phase, focus, next action, blockers, evidence, or verification state changes materially.\n- Keep discovery bounded to the minimal implementation files needed for this issue; defer broader docs or upstream reading unless a concrete ambiguity blocks the change.\n- Implement the fix in the current worktree.\n{internal_review_guidance}- Treat failures from repo-native `canonicalize_commands`, `verify_commands`, or tracked rewrites left by that repo gate as continued repair by default: keep fixing the lane and rerun the gate instead of taking `manual_attention` unless the blocker is clearly toolchain, environment, or operator-owned.\n- Run the repository validation needed to justify a reviewable PR.\n- Commit the lane, push branch `{branch}`, and create or update a non-draft PR titled `{pr_title}` for that branch.\n{completion_guidance}- If the issue needs manual attention, add label `{needs_attention}` with `{label_tool}`, explain why in a comment, and then call `{terminal_finalize_tool}` with path `manual_attention`. Do not call `{review_handoff_tool}` in that case; `decodex` will stop the lane as a human-required failure without automatic retry.\n- Do not move the issue directly to `{success}` with `{transition_tool}`; `decodex` will finish that writeback after its own validation passes.\n- Do not report the run as complete or treat `{progress_checkpoint_tool}` as terminal completion until `{terminal_finalize_tool}` succeeds.{continuation_guidance}",
			identifier = issue.identifier,
			title = issue.title,
			description = description,
			transition_tool = ISSUE_TRANSITION_TOOL_NAME,
			comment_tool = ISSUE_COMMENT_TOOL_NAME,
			label_tool = ISSUE_LABEL_ADD_TOOL_NAME,
			progress_checkpoint_tool = ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME,
			review_handoff_tool = ISSUE_REVIEW_HANDOFF_TOOL_NAME,
			terminal_finalize_tool = ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
			in_progress = workflow.frontmatter().tracker().in_progress_state(),
			run_id = issue_run.run_id,
			attempt = issue_run.attempt_number,
			branch = issue_run.worktree.branch_name,
			success = workflow.frontmatter().tracker().success_state(),
			needs_attention = workflow.frontmatter().tracker().needs_attention_label(),
			continuation_guidance = continuation_guidance,
			pr_title = review_pull_request_title(issue),
			internal_review_guidance = build_handoff_internal_review_guidance(
				internal_review_mode
			),
			completion_guidance = build_handoff_completion_guidance(internal_review_mode),
		),
	}
}

fn build_continuation_user_input(
	issue: &TrackerIssue,
	workflow: &WorkflowDocument,
	dispatch_mode: IssueDispatchMode,
	recorded_pr_url: Option<&str>,
	success_state: &str,
	internal_review_mode: InternalReviewMode,
) -> String {
	let completed_state = workflow
		.frontmatter()
		.tracker()
		.resolved_completed_state();

	match dispatch_mode {
			IssueDispatchMode::ReviewRepair => format!(
				"Continue retained review repair for Linear issue {identifier} in the current thread and worktree.\n\nContinuation checklist:\n- Resume from the current repository state and outstanding review feedback or retained landing fallback on `{pr_url}`.\n- Keep changes scoped to the same retained review lane and do not move the issue out of `{success}`.\n{internal_review_guidance}- Validate each actionable review claim against the codebase, tests, and requirements before changing code, and keep pushback or clarification threads open until the repaired head is ready.\n- If the blocker is landing fallback, repair only the branch sync, conflict, ambiguous mergeability, or repository-specific recovery issue; do not merge or land the PR yourself.\n- Treat failures from repo-native `canonicalize_commands`, `verify_commands`, or tracked rewrites left by that repo gate as continued repair by default: keep fixing the lane and rerun the gate instead of taking `manual_attention` unless the blocker is clearly toolchain, environment, or operator-owned.\n- If the repaired head is ready, push it. Do not request fresh external review yourself; Decodex will post the next runtime-owned external review request after `{review_repair_tool}` succeeds.\n- After the repaired head is pushed, reply in-thread for every addressed comment and resolve only the GitHub review threads whose fixes landed and verified on the repaired head.\n{completion_guidance}- If the issue requires manual attention, record the manual-attention tracker path before ending the turn.\n- If more work still remains after this turn, you may end the turn without terminal finalization and Decodex will decide whether to continue.",
			identifier = issue.identifier,
			pr_url = recorded_pr_url.unwrap_or("(missing review handoff marker)"),
			success = success_state,
			review_repair_tool = ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME,
			internal_review_guidance = build_repair_continuation_review_guidance(
				internal_review_mode
			),
			completion_guidance = build_repair_continuation_completion_guidance(
				internal_review_mode
			),
		),
		IssueDispatchMode::Closeout => format!(
			"Continue retained closeout for Linear issue {identifier} in the current thread and worktree.\n\nContinuation checklist:\n- Resume from the current repository state and merged PR lineage on `{pr_url}`.\n- Keep changes scoped to the same retained post-review lane. Do not move the issue back to implementation; the tracker may already be in `{completed}` while closeout or cleanup remains pending.\n- Treat this resumed closeout as a short deterministic tail. Reuse the existing merged PR evidence instead of restarting broad discovery, and only rerun the minimum validation needed to justify `Done` plus cleanup.\n- If you record `{progress_checkpoint_tool}` during closeout, either omit `head_sha` or pass the exact full current `HEAD` SHA.\n- Merge is already authoritative for `{pr_url}` before this run starts. Do not land, merge, or request review from this closeout run.\n- If the issue is still in `{success}`, transition it once to `{completed}` with `{transition_tool}` before `{closeout_tool}`.\n- If Linear closeout is complete, call `{closeout_tool}` and then call `{terminal_finalize_tool}` with path `closeout`.\n- Do not end the turn without either `{closeout_tool}` plus `{terminal_finalize_tool}`, or the manual-attention path.\n- If the issue requires manual attention, record the manual-attention tracker path before ending the turn.",
			identifier = issue.identifier,
			pr_url = recorded_pr_url.unwrap_or("(missing review handoff marker)"),
			progress_checkpoint_tool = ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME,
			transition_tool = ISSUE_TRANSITION_TOOL_NAME,
			success = success_state,
			completed = completed_state,
			closeout_tool = ISSUE_DELIVERY_CLOSEOUT_COMPLETE_TOOL_NAME,
			terminal_finalize_tool = ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
		),
		_ => format!(
			"Continue working on Linear issue {identifier} in the current thread and worktree.\n\nContinuation checklist:\n- Resume from the current repository state instead of restarting broad discovery.\n- Keep changes scoped to the same issue lane.\n{internal_review_guidance}- Treat failures from repo-native `canonicalize_commands`, `verify_commands`, or tracked rewrites left by that repo gate as continued repair by default: keep fixing the lane and rerun the gate instead of taking `manual_attention` unless the blocker is clearly toolchain, environment, or operator-owned.\n{completion_guidance}- If the issue requires manual attention, record the manual-attention tracker path before ending the turn.\n- If more work still remains after this turn, you may end the turn without terminal finalization and Decodex will decide whether to continue.",
			identifier = issue.identifier,
			internal_review_guidance = build_handoff_continuation_review_guidance(
				internal_review_mode
			),
			completion_guidance = build_handoff_continuation_completion_guidance(
				internal_review_mode,
				&review_pull_request_title(issue),
			),
		),
	}
}

fn build_handoff_internal_review_guidance(internal_review_mode: InternalReviewMode) -> String {
	match internal_review_mode {
		InternalReviewMode::Loop => format!(
			"- Follow the repo-native bounded review method from `WORKFLOW.md`: review the actual current diff and branch state, run both the requirements pass and the adversarial reviewer pass, fix only the smallest coherent owned batch, rerun verification, and re-read `HEAD` before deciding the next normalized review status.\n- Every time the repo-native bounded review method produces a result for the current head, call `{}` with that normalized status, the exact current `HEAD` SHA, and any concise evidence items.\n",
			ISSUE_REVIEW_CHECKPOINT_TOOL_NAME
		),
		InternalReviewMode::Prompt => format!("- {PROMPT_ONLY_INTERNAL_REVIEW_INSTRUCTION}\n"),
		InternalReviewMode::Off => format!(
			"- `codex.internal_review_mode = \"off\"` for this project, so skip internal self-review and do not call `{}`.\n",
			ISSUE_REVIEW_CHECKPOINT_TOOL_NAME
		),
	}
}

fn build_repair_internal_review_guidance(internal_review_mode: InternalReviewMode) -> String {
	match internal_review_mode {
		InternalReviewMode::Loop => format!(
			"- Follow the repo-native bounded review method from `WORKFLOW.md`: review the actual repaired branch state, run both the requirements pass and the adversarial reviewer pass, fix only the smallest coherent owned batch, rerun verification, and re-read `HEAD` before deciding the next normalized review status.\n- Every time the repo-native bounded review method produces a result for the current repaired head, call `{}` with that normalized status, the exact current `HEAD` SHA, and any concise evidence items.\n",
			ISSUE_REVIEW_CHECKPOINT_TOOL_NAME
		),
		InternalReviewMode::Prompt => format!("- {PROMPT_ONLY_INTERNAL_REVIEW_INSTRUCTION}\n"),
		InternalReviewMode::Off => format!(
			"- `codex.internal_review_mode = \"off\"` for this project, so skip internal self-review and do not call `{}`.\n",
			ISSUE_REVIEW_CHECKPOINT_TOOL_NAME
		),
	}
}

fn build_handoff_completion_guidance(internal_review_mode: InternalReviewMode) -> String {
	match internal_review_mode {
		InternalReviewMode::Loop => format!(
			"- Call `{}` only after the latest `{}` for this handoff phase and current `HEAD` is `clean`. Then call `{}` with path `review_handoff`.\n",
			ISSUE_REVIEW_HANDOFF_TOOL_NAME,
			ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
			ISSUE_TERMINAL_FINALIZE_TOOL_NAME
		),
		InternalReviewMode::Prompt | InternalReviewMode::Off => format!(
			"- Call `{}` after the branch is pushed, the non-draft PR is ready, and required validation has passed. Then call `{}` with path `review_handoff`.\n",
			ISSUE_REVIEW_HANDOFF_TOOL_NAME, ISSUE_TERMINAL_FINALIZE_TOOL_NAME
		),
	}
}

fn build_repair_completion_guidance(internal_review_mode: InternalReviewMode) -> String {
	match internal_review_mode {
		InternalReviewMode::Loop => format!(
			"- Call `{}` only after the latest `{}` for this repair phase and current `HEAD` is `clean`. Then call `{}` with path `review_repair`.\n",
			ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME,
			ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
			ISSUE_TERMINAL_FINALIZE_TOOL_NAME
		),
		InternalReviewMode::Prompt | InternalReviewMode::Off => format!(
			"- Call `{}` after the repaired head is pushed and required validation has passed. Then call `{}` with path `review_repair`.\n",
			ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME, ISSUE_TERMINAL_FINALIZE_TOOL_NAME
		),
	}
}

fn build_handoff_continuation_review_guidance(
	internal_review_mode: InternalReviewMode,
) -> String {
	match internal_review_mode {
		InternalReviewMode::Loop => format!(
			"- Resume the repo-native bounded review method from `WORKFLOW.md`: review the actual current diff and branch state, run both the requirements pass and the adversarial reviewer pass, fix only the smallest coherent owned batch, rerun verification, and re-read `HEAD` before deciding the next normalized review status.\n- After each bounded review result for the current head, call `{}` with the normalized status and current `HEAD` SHA.\n",
			ISSUE_REVIEW_CHECKPOINT_TOOL_NAME
		),
		InternalReviewMode::Prompt => format!("- {PROMPT_ONLY_INTERNAL_REVIEW_INSTRUCTION}\n"),
		InternalReviewMode::Off => format!(
			"- `codex.internal_review_mode = \"off\"` for this project, so continue without internal self-review and do not call `{}`.\n",
			ISSUE_REVIEW_CHECKPOINT_TOOL_NAME
		),
	}
}

fn build_repair_continuation_review_guidance(internal_review_mode: InternalReviewMode) -> String {
	match internal_review_mode {
		InternalReviewMode::Loop => format!(
			"- Resume the repo-native bounded review method from `WORKFLOW.md`: review the actual repaired branch state, run both the requirements pass and the adversarial reviewer pass, fix only the smallest coherent owned batch, rerun verification, and re-read `HEAD` before deciding the next normalized review status.\n- After each bounded review result for the repaired head, call `{}` with the normalized status and current `HEAD` SHA.\n",
			ISSUE_REVIEW_CHECKPOINT_TOOL_NAME
		),
		InternalReviewMode::Prompt => format!("- {PROMPT_ONLY_INTERNAL_REVIEW_INSTRUCTION}\n"),
		InternalReviewMode::Off => format!(
			"- `codex.internal_review_mode = \"off\"` for this project, so continue without internal self-review and do not call `{}`.\n",
			ISSUE_REVIEW_CHECKPOINT_TOOL_NAME
		),
	}
}

fn build_handoff_continuation_completion_guidance(
	internal_review_mode: InternalReviewMode,
	pr_title: &str,
) -> String {
	match internal_review_mode {
		InternalReviewMode::Loop => format!(
			"- If the implementation is review-ready, ensure the non-draft PR title is `{pr_title}` and finish the PR-backed tracker handoff only after the latest `{}` for the current `HEAD` is `clean`.\n",
			ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
		),
		InternalReviewMode::Prompt | InternalReviewMode::Off => format!(
			"- If the implementation is review-ready, ensure the non-draft PR title is `{pr_title}` and finish the PR-backed tracker handoff after required validation has passed.\n",
		),
	}
}

fn build_repair_continuation_completion_guidance(
	internal_review_mode: InternalReviewMode,
) -> String {
	match internal_review_mode {
		InternalReviewMode::Loop => format!(
			"- Call `{}` only after the latest `{}` for the current `HEAD` is `clean`, and then call `{}` with path `review_repair`.\n",
			ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME,
			ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
			ISSUE_TERMINAL_FINALIZE_TOOL_NAME
		),
		InternalReviewMode::Prompt | InternalReviewMode::Off => format!(
			"- Call `{}` after the repaired head is pushed and required validation has passed, and then call `{}` with path `review_repair`.\n",
			ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME, ISSUE_TERMINAL_FINALIZE_TOOL_NAME
		),
	}
}

fn allows_clean_continuation(
	workflow: &WorkflowDocument,
	dispatch_mode: IssueDispatchMode,
) -> bool {
	workflow.frontmatter().execution().max_turns() > 1
		&& dispatch_mode != IssueDispatchMode::Closeout
}

fn build_external_repair_architecture_guidance(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
) -> String
{
	let review_handoff = match state_store.review_handoff_marker(
		project.service_id(),
		&issue_run.issue.id,
		&issue_run.worktree.branch_name,
	) {
		Ok(Some(review_handoff)) => review_handoff,
		Ok(None) => return String::new(),
		Err(error) => {
			tracing::warn!(
				?error,
				issue = issue_run.issue.identifier,
				run_id = issue_run.run_id,
				worktree_path = %issue_run.worktree.path.display(),
				"Retained review prompt could not read the runtime handoff; omitting architecture guidance."
			);

			return String::new();
		},
	};
	let marker = match state_store.review_orchestration_marker(
		project.service_id(),
		&issue_run.issue.id,
		&review_handoff,
	) {
		Ok(Some(marker)) => marker,
		Ok(None) => return String::new(),
		Err(error) => {
			tracing::warn!(
				?error,
				issue = issue_run.issue.identifier,
				run_id = issue_run.run_id,
				worktree_path = %issue_run.worktree.path.display(),
				"Retained review prompt could not read runtime orchestration state; omitting architecture guidance."
			);

			return String::new();
		},
	};

	if marker.external_round_count() < 4 {
		return String::new();
	}

	format!(
		"- This retained repair is external review round {}. Before another patch-only cycle, decide whether the repeated churn points to an architectural or root-cause defect that local patching will not converge.\n- If it is architectural, take the manual-attention path instead of continuing patch-on-patch repair.\n- If it is not architectural and the findings are still normal retained review work, continue this repair normally; a successful `{}` will reset the external review-round budget.\n",
		marker.external_round_count(),
		ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME
	)
}

fn build_review_run_context(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
) -> Result<ReviewHandoffContext>
{
	match issue_run.dispatch_mode {
		IssueDispatchMode::ReviewRepair => {
			validate_review_repair_runtime(project, false)?;

			let review_handoff = read_retained_review_handoff(project, state_store, issue_run)?
				.ok_or_else(|| {
					eyre::eyre!(
						"Retained review-repair run `{}` for issue `{}` requires an existing runtime review handoff.",
						issue_run.run_id,
						issue_run.issue.identifier
					)
				})?;

			Ok(ReviewHandoffContext {
				attempt_number: issue_run.attempt_number,
				branch_name: issue_run.worktree.branch_name.clone(),
				run_id: issue_run.run_id.clone(),
				service_id: project.service_id().to_owned(),
				worktree_path: relative_worktree_path(project, &issue_run.worktree),
				cwd: issue_run.worktree.path.clone(),
				github_token_env_var: Some(project.github().token_env_var().to_owned()),
				internal_review_mode: project.codex().internal_review_mode(),
				mode: ReviewExecutionMode::Repair,
				recorded_pr_url: Some(review_handoff.pr_url().to_owned()),
			})
		},
		IssueDispatchMode::Closeout => {
			validate_closeout_runtime(project, false)?;

			let review_handoff = read_retained_review_handoff(project, state_store, issue_run)?
				.ok_or_else(|| {
					eyre::eyre!(
						"Retained closeout run `{}` for issue `{}` requires an existing runtime review handoff.",
						issue_run.run_id,
						issue_run.issue.identifier
					)
				})?;

			Ok(ReviewHandoffContext {
				attempt_number: issue_run.attempt_number,
				branch_name: issue_run.worktree.branch_name.clone(),
				run_id: issue_run.run_id.clone(),
				service_id: project.service_id().to_owned(),
				worktree_path: relative_worktree_path(project, &issue_run.worktree),
				cwd: issue_run.worktree.path.clone(),
				github_token_env_var: Some(project.github().token_env_var().to_owned()),
				internal_review_mode: project.codex().internal_review_mode(),
				mode: ReviewExecutionMode::Closeout,
				recorded_pr_url: Some(review_handoff.pr_url().to_owned()),
			})
		},
		_ => Ok(ReviewHandoffContext {
			attempt_number: issue_run.attempt_number,
			branch_name: issue_run.worktree.branch_name.clone(),
			run_id: issue_run.run_id.clone(),
			service_id: project.service_id().to_owned(),
			worktree_path: relative_worktree_path(project, &issue_run.worktree),
			cwd: issue_run.worktree.path.clone(),
			github_token_env_var: Some(project.github().token_env_var().to_owned()),
			internal_review_mode: project.codex().internal_review_mode(),
			mode: ReviewExecutionMode::Handoff,
			recorded_pr_url: None,
		}),
	}
}

fn read_retained_review_handoff(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
) -> Result<Option<ReviewHandoffMarker>>
{
	state_store.review_handoff_marker(
		project.service_id(),
		&issue_run.issue.id,
		&issue_run.worktree.branch_name,
	)
}
