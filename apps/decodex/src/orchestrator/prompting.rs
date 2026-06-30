pub(crate) const TRACKER_PUBLIC_TEXT_BOUNDARY_INSTRUCTION: &str =
	"Tracker public text boundary\n- Linear tracker text is public/team-visible. Do not include local host paths, routed identity details, account details, credential-like names, private config paths, tokens, or secrets in issue comments, progress checkpoints, review summaries, closeout summaries, blockers, evidence, verification, failed commands, or raw errors.\n- `issue_comment` accepts only allowlisted public comment kinds. For manual attention, call it with `kind: \"manual_attention\"` and structured public fields; do not send arbitrary comment bodies.\n- Use public collaboration identifiers when needed: PR URLs, issue identifiers, branch names, commit SHAs, and repository-relative paths.\n- Decodex may apply a local-only secondary privacy classifier to rendered public projections, but that classifier is not the privacy boundary; keep private evidence out of public fields before tool calls.";

const SELF_CHECK_INSTRUCTION: &str =
	"Review your work repeatedly and fix any logic bugs until no new issues are found.";
const DOCS_IMPACT_CONTRACT: &str =
	"Docs impact contract\n- Before any phase completion or terminal finalize path, classify docs impact as `none`, `update_required`, `research_required`, or `drift_required`, and record it in a current-HEAD `issue_progress_checkpoint` as `docs_impact`.\n- If behavior, commands, config, schemas, status, validation, workflow, or docs changed, update the owning OKF concept under `docs/` and any required `code_refs`, `drift_watch`, or `docs/evidence/` drift audit evidence.\n- If authority is missing or contradictory, use `research_required`, switch to Decodex `research*`, and keep any `docs/research/` output latent until explicit promotion.\n- Run the repository docs gate when one exists; in this repository use `decodex docs check` or the repo-native command that wraps it.\n- Treat docs-check or semantic-drift failure as a completion blocker. Fix issue-local docs failures in the lane. Do not route pre-existing, repo-wide, or global-baseline docs failures through `manual_attention`; record the blocker in private evidence and let Decodex retain or isolate the baseline lane.";

fn build_manual_attention_guidance(
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

fn build_phase_goal_runtime_contract() -> String {
	format!(
		"Phase goal runtime contract\n- Decodex may set an active phase goal that narrows the immediate turn below the full issue lifecycle checklist.\n- Treat the active phase goal as the authoritative current contract. For every phase, record a current-HEAD `{progress_checkpoint_tool}` with `docs_impact` before claiming the phase is satisfied.\n- For `implement_to_validation_ready`, `repair_validation_failures`, and `repair_accepted_review_findings`, stop at validated local work, then explicitly complete the active phase goal with the Codex goal completion mechanism so Decodex can run its repo gate and select the next phase.\n- Do not use `{progress_checkpoint_tool}`, final chat text, or an \"await next phase\" statement as a substitute for completing a satisfied phase goal.\n- The later `handoff_evidence` phase creates or updates the PR and records the normal review handoff terminal path.\n- The later `review_repair_evidence` phase pushes the retained PR repair head, records review-repair completion, and calls `{terminal_finalize_tool}` with path `review_repair`; it must not call `issue_review_handoff` or move the issue out of its retained review state.",
		progress_checkpoint_tool = ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME,
		terminal_finalize_tool = ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
	)
}

fn build_retry_recovery_context(dispatch_mode: IssueDispatchMode) -> Option<String> {
	(dispatch_mode == IssueDispatchMode::Retry).then(|| {
		String::from(
			"Recovery context\n- This is retry-style re-entry after a prior attempt stopped or could not prove live execution.\n- Treat the current worktree, tracker state, protocol events, and marker files as the durable source of truth. Do not assume in-memory model output or tool results survived.\n- Before editing, inspect the current branch, diff, and recent validation evidence, reconcile partial work already present, and continue from that state instead of restarting from scratch.",
		)
	})
}

fn build_architecture_recovery_context(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
) -> Option<String> {
	let events = match state_store
		.list_private_execution_events_for_issue(project.service_id(), &issue_run.issue.id)
	{
		Ok(events) => events,
		Err(error) => {
			tracing::warn!(
				?error,
				issue = issue_run.issue.identifier,
				run_id = issue_run.run_id,
				"Prompt could not read architecture recovery evidence."
			);

			return None;
		},
	};
	let event = events
		.iter()
		.rev()
		.find(|event| {
			matches!(
				event.event_type(),
				ARCHITECTURE_RECOVERY_PACKET_EVENT_TYPE
					| ARCHITECTURE_RECOVERY_STARTED_EVENT_TYPE
					| ARCHITECTURE_RECOVERY_TERMINAL_EVENT_TYPE
			)
		})?;

	if event.event_type() != ARCHITECTURE_RECOVERY_STARTED_EVENT_TYPE {
		return None;
	}

	let payload = event.payload();
	let guardrail_reason = payload
		.get("guardrail_reason")
		.and_then(Value::as_str)
		.unwrap_or("loop_guardrail");
	let recovery_attempt = payload
		.get("recovery_budget")
		.and_then(|budget| budget.get("attempt"))
		.and_then(Value::as_u64)
		.unwrap_or(1);
	let recovery_max = payload
		.get("recovery_budget")
		.and_then(|budget| budget.get("max_attempts"))
		.and_then(Value::as_u64)
		.unwrap_or(1);
	let policy_decision = payload
		.get("boundary_policy_decision")
		.and_then(Value::as_str)
		.unwrap_or("auto_continue");
	let requires_enhanced_evidence = payload
		.get("requires_enhanced_evidence")
		.and_then(Value::as_bool)
		.unwrap_or(matches!(
			policy_decision,
			"requires_enhanced_evidence" | "block_landing"
		));
	let blocks_landing = payload
		.get("blocks_landing")
		.and_then(Value::as_bool)
		.unwrap_or(policy_decision == "block_landing");
	let mut policy_guidance = format!("Authority policy `{policy_decision}` applies");

	if requires_enhanced_evidence {
		policy_guidance.push_str("; preserve enhanced evidence before review handoff or landing");
	}
	if blocks_landing {
		policy_guidance
			.push_str("; keep landing blocked until validation or review-policy evidence is restored");
	}

	policy_guidance.push('.');

	Some(format!(
		"Architecture recovery context\n- Decodex recorded `architecture_recovery_started` for guardrail `{guardrail_reason}` after an Authority Boundary Check returned policy `{policy_decision}`.\n- This is autonomous architecture recovery attempt {recovery_attempt} of {recovery_max}; start a materially different implementation strategy instead of repeating the ineffective repair.\n- {policy_guidance}\n- Preserve the accepted Decision Contract, public API/config behavior, and validation/review gates. Do not ask the user through chat while detached; use manual attention only if the next viable action crosses authority."
	))
}

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
		let contents = read_workflow_read_first_file(project, relative_path)?;

		sections.push(format!("File: {relative_path}\n{contents}"));
	}

	sections.push(String::from(
		"Execution discipline\n- Keep pre-edit discovery bounded to the smallest code surface that can satisfy the current issue.\n- Start with the implementation files directly implicated by the issue before reading broader docs or repo-wide guidance.\n- Do not browse upstream references or general repository documentation unless a concrete ambiguity blocks the change.\n- Once the relevant change surface is identified, patch code and run validation instead of continuing broad searches.",
	));
	sections.push(String::from(DOCS_IMPACT_CONTRACT));
	sections.push(String::from(
		"Commit contract\n- When you create a local commit for this lane, use a single-line `decodex/commit/1` JSON commit message.\n- Required fields: `schema`, `summary`, and `authority`.\n- `authority` must be the authoritative Linear issue identifier for this lane.\n- Optional fields: `related` and `breaking`.\n- Do not encode landing mode, CI status, closeout state, or other process-state fields in the commit message.",
	));
	sections.push(build_phase_goal_runtime_contract());
	sections.push(String::from(TRACKER_PUBLIC_TEXT_BOUNDARY_INSTRUCTION));

	if let Some(recovery_context) = build_retry_recovery_context(issue_run.dispatch_mode) {
		sections.push(recovery_context);
	}
	if let Some(recovery_context) =
		build_architecture_recovery_context(project, state_store, issue_run)
	{
		sections.push(recovery_context);
	}

	let repair_architecture_guidance =
		build_external_repair_architecture_guidance(project, state_store, issue_run);
	let completed_state = workflow
		.frontmatter()
		.tracker()
		.resolved_completed_state();
	let review_level = project.codex().review_level();
	let needs_attention = workflow.frontmatter().tracker().needs_attention_label();
	let repair_manual_attention_guidance = build_manual_attention_guidance(
		needs_attention,
		ISSUE_LABEL_ADD_TOOL_NAME,
		ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
		None,
	);
	let handoff_manual_attention_guidance = build_manual_attention_guidance(
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
			decodex_review_guidance = build_repair_review_guidance(review_level),
			github_review_guidance = build_repair_github_review_guidance(review_level, ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME),
			retained_tail_guidance = build_repair_retained_tail_guidance(review_level, workflow.frontmatter().tracker().success_state()),
			completion_guidance = build_repair_completion_guidance(review_level),
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
			decodex_review_guidance = build_handoff_review_guidance(
				review_level
			),
			completion_guidance = build_handoff_completion_guidance(review_level),
		),
	};

	sections.push(tracker_contract);

	Ok(sections.join("\n\n"))
}

fn validate_workflow_read_first_files(
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
) -> Result<()> {
	for relative_path in workflow.frontmatter().context().read_first() {
		read_workflow_read_first_file(project, relative_path)?;
	}

	Ok(())
}

fn read_workflow_read_first_file(
	project: &ServiceConfig,
	relative_path: &str,
) -> Result<String> {
	let absolute_path = project.repo_root().join(relative_path);

	fs::read_to_string(&absolute_path).map_err(|error| {
		if error.kind() == ErrorKind::NotFound {
			return eyre::eyre!(
				"Project `{}` workflow `{}` references missing `context.read_first` file `{}` at `{}`. Update the path or restore the file before dispatch.",
				project.service_id(),
				project.workflow_path().display(),
				relative_path,
				absolute_path.display()
			);
		}

		eyre::eyre!(
			"Failed to read project `{}` workflow `{}` `context.read_first` file `{}` at `{}`: {error}",
			project.service_id(),
			project.workflow_path().display(),
			relative_path,
			absolute_path.display()
		)
	})
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
	let review_level = project.codex().review_level();
	let needs_attention = workflow.frontmatter().tracker().needs_attention_label();
	let repair_manual_attention_guidance = build_manual_attention_guidance(
		needs_attention,
		ISSUE_LABEL_ADD_TOOL_NAME,
		ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
		None,
	);
	let handoff_manual_attention_guidance = build_manual_attention_guidance(
		needs_attention,
		ISSUE_LABEL_ADD_TOOL_NAME,
		ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
		Some(ISSUE_REVIEW_HANDOFF_TOOL_NAME),
	);
	let recovery_context = build_retry_recovery_context(issue_run.dispatch_mode)
		.into_iter()
		.chain(build_architecture_recovery_context(project, state_store, issue_run))
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
			decodex_review_guidance = build_repair_review_guidance(review_level),
			github_review_guidance = build_repair_github_review_guidance(review_level, ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME),
			completion_guidance = build_repair_completion_guidance(review_level),
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
			decodex_review_guidance = build_handoff_review_guidance(
				review_level
			),
			completion_guidance = build_handoff_completion_guidance(review_level),
		),
	}
}

fn build_continuation_user_input(
	issue: &TrackerIssue,
	workflow: &WorkflowDocument,
	dispatch_mode: IssueDispatchMode,
	recorded_pr_url: Option<&str>,
	success_state: &str,
	review_level: ReviewLevel,
) -> String {
	let completed_state = workflow
		.frontmatter()
		.tracker()
		.resolved_completed_state();
	let needs_attention = workflow.frontmatter().tracker().needs_attention_label();
	let repair_manual_attention_guidance = build_manual_attention_guidance(
		needs_attention,
		ISSUE_LABEL_ADD_TOOL_NAME,
		ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
		None,
	);
	let handoff_manual_attention_guidance = build_manual_attention_guidance(
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
			github_review_guidance = build_repair_github_review_guidance(review_level, ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME),
			decodex_review_guidance = build_repair_continuation_review_guidance(
				review_level
			),
			completion_guidance = build_repair_continuation_completion_guidance(
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
			decodex_review_guidance = build_handoff_continuation_review_guidance(
				review_level
			),
			completion_guidance = build_handoff_continuation_completion_guidance(
				review_level,
				&review_pull_request_title(issue),
			),
		),
	}
}

fn build_handoff_review_guidance(review_level: ReviewLevel) -> String {
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

fn build_repair_review_guidance(review_level: ReviewLevel) -> String {
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

fn review_signal_route_guidance() -> &'static str {
	"- Adjudicate every reviewer signal into `finding_routes` before repair: accepted current repair work must route to `current_blocker`; requests for evidence, follow-up, risk notes, reviewer rubric gaps, architecture signals, issue-contract gaps, landing blockers, and authority blockers must use the matching non-current or landing-blocking route.\n- Preserve reviewer and agent judgment: the reviewer may accept, reject, request evidence, mark follow-up/risk/rubric gaps, or identify architecture/authority blockers, but the runtime must receive structured route evidence before any repair loop uses the signal.\n- Non-current `finding_routes` such as `follow_up`, `risk_note`, `reviewer_rubric_gap`, and `invalid_or_unsubstantiated` are durable evidence and must not drive repair churn.\n"
}

fn build_handoff_completion_guidance(review_level: ReviewLevel) -> String {
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

fn build_repair_completion_guidance(review_level: ReviewLevel) -> String {
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

fn build_handoff_continuation_review_guidance(review_level: ReviewLevel) -> String {
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

fn build_repair_continuation_review_guidance(review_level: ReviewLevel) -> String {
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

fn build_handoff_continuation_completion_guidance(
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

fn build_repair_continuation_completion_guidance(
	review_level: ReviewLevel,
) -> String {
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

fn build_repair_github_review_guidance(review_level: ReviewLevel, repair_tool_name: &str) -> String {
	if review_level.uses_github_review() {
		return format!(
			"- Do not request GitHub Review yourself. Decodex will post the next runtime-owned GitHub Review request after `{repair_tool_name}` succeeds.\n",
		);
	}

	String::from(
		"- Do not request GitHub Review from this run; the configured review level does not use the runtime-owned GitHub Review path.\n",
	)
}

fn build_repair_retained_tail_guidance(review_level: ReviewLevel, success_state: &str) -> String {
	if review_level.uses_github_review() {
		return format!(
			"- Keep the tracker issue in `{success_state}`. Decodex will handle the later GitHub Review request or clean-path runtime landing, closeout, and cleanup lifecycle.\n",
		);
	}

	format!(
		"- Keep the tracker issue in `{success_state}`. Decodex will handle the clean-path runtime landing, closeout, and cleanup lifecycle.\n",
	)
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
		"- This retained repair is GitHub Review round {}. Before another patch-only cycle, decide whether the repeated churn points to an architectural or root-cause defect that local patching will not converge.\n- If it is architectural, take the manual-attention path instead of continuing patch-on-patch repair.\n- If it is not architectural and the findings are still normal retained review work, continue this repair normally; a successful `{}` will reset the GitHub Review round budget.\n",
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
				github_command_path: project.github().command_path().map(Path::to_path_buf),
				review_level: project.codex().review_level(),
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
				github_command_path: project.github().command_path().map(Path::to_path_buf),
				review_level: project.codex().review_level(),
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
			github_command_path: project.github().command_path().map(Path::to_path_buf),
			review_level: project.codex().review_level(),
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
