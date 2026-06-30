use orchestrator::TRACKER_PUBLIC_TEXT_BOUNDARY_INSTRUCTION;

#[test]
fn daemon_workflow_reload_keeps_last_known_good_on_same_path_failure() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let mut workflow_cache = None;
	let initial = orchestrator::load_daemon_tick_workflow(&config, &mut workflow_cache)
		.expect("initial workflow load should succeed");

	assert_eq!(initial, workflow);

	fs::write(config.workflow_path(), "not valid workflow markdown")
		.expect("invalid workflow should be written");

	let fallback = orchestrator::load_daemon_tick_workflow(&config, &mut workflow_cache)
		.expect("invalid reload should keep the cached workflow");

	assert_eq!(fallback, workflow);
}

#[test]
fn daemon_workflow_reload_replaces_cached_document_after_valid_update() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let mut workflow_cache = None;

	orchestrator::load_daemon_tick_workflow(&config, &mut workflow_cache)
		.expect("initial workflow load should succeed");

	let updated_workflow = sample_workflow_markdown("pubfi", &[], "Updated workflow policy.\n", 1)
		.replace("max_attempts = 3", "max_attempts = 5");

	fs::write(config.workflow_path(), updated_workflow)
		.expect("updated workflow should be written");

	let reloaded = orchestrator::load_daemon_tick_workflow(&config, &mut workflow_cache)
		.expect("valid reload should replace the cached workflow");

	assert_ne!(reloaded, workflow);
	assert_eq!(reloaded.frontmatter().execution().max_attempts(), 5);
	assert_eq!(reloaded.body(), "Updated workflow policy.");
}

#[test]
fn configured_cycle_workflow_snapshot_overrides_invalid_disk_workflow() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let issue = sample_issue("Todo", &[]);
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let workflow_snapshot = workflow.to_markdown().expect("workflow markdown should render");

	fs::write(config.workflow_path(), "not valid workflow markdown")
		.expect("invalid workflow should be written");

	assert!(
		orchestrator::load_configured_cycle_workflow(&config, None).is_err(),
		"without an override the configured workflow load should fail"
	);

	let loaded = orchestrator::load_configured_cycle_workflow(&config, Some(&workflow_snapshot))
		.expect("configured workflow load should accept the supplied snapshot");
	let summary = orchestrator::run_target_issue_once(TargetIssueRunContext {
		tracker: &tracker,
		project: &config,
		workflow: &loaded,
		state_store: &state_store,
		issue_id: &issue.id,
		preferred_issue_state: None,
		preferred_initial_issue_state: None,
		dry_run: true,
		lease_preacquired: false,
		preferred_issue_claim_fd: None,
		preferred_dispatch_slot_fd: None,
		preferred_dispatch_slot_index: None,
		dispatch_mode: IssueDispatchMode::Normal,
		preferred_run_identity: None,
		preferred_retry_budget_base: None,
	})
	.expect("target issue dry run should succeed with the supplied snapshot");

	assert!(summary.is_some(), "the child path should still run off the cached snapshot");
}

#[test]
fn active_child_reconciliation_keeps_spawn_time_workflow_until_exit() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let active_workflow = WorkflowDocument::parse_markdown(
		&sample_workflow_markdown("pubfi", &[], "Spawn-time workflow policy.\n", 1)
			.replace("max_attempts = 3", "max_attempts = 5"),
	)
	.expect("workflow should parse");
	let current_workflow = WorkflowDocument::parse_markdown(
		&sample_workflow_markdown("pubfi", &[], "Current workflow policy.\n", 1)
			.replace("startable_states = [\"Todo\"]", "startable_states = [\"Backlog\"]"),
	)
	.expect("workflow should parse");
	let child_issue = sample_issue("Todo", &[]);
	let stale_issue = sample_issue_with_sort_fields(
		"issue-stale",
		"PUB-202",
		"Todo",
		&[],
		Some(3),
		"2026-03-13T04:16:17.133Z",
	);
	let tracker = FakeTracker::new(vec![child_issue.clone(), stale_issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");

	state_store
		.record_run_attempt("run-child", &child_issue.id, 1, "running")
		.expect("child run attempt should record");
	state_store
		.upsert_lease("pubfi", &child_issue.id, "run-child", "In Progress")
		.expect("child lease should record");
	state_store
		.record_run_attempt("run-stale", &stale_issue.id, 1, "running")
		.expect("stale run attempt should record");
	state_store
		.upsert_lease("pubfi", &stale_issue.id, "run-stale", "In Progress")
		.expect("stale lease should record");

	let actions = orchestrator::inspect_run_lease_reconciliation_at(
		&tracker,
		&config,
		&current_workflow,
		&state_store,
		Some(ActiveWorkflowOverride {
			child: ChildRunRef {
				issue_id: &child_issue.id,
				run_id: "run-child",
				attempt_number: 1,
			},
			workflow: &active_workflow,
		}),
		OffsetDateTime::now_utc().unix_timestamp() + 1,
	)
	.expect("run lease inspection should succeed");

	assert!(
		actions.iter().all(|action| action.issue.id != child_issue.id),
		"the current child should keep its spawn-time workflow snapshot"
	);
	assert!(actions.iter().any(|action| {
		action.issue.id == stale_issue.id
			&& matches!(action.disposition, orchestrator::RunLeaseDisposition::NotDispatchable)
	}));
}

fn expected_developer_instructions(
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

	sections.extend(
		read_first_files
			.iter()
			.map(|(relative_path, contents)| format!("File: {relative_path}\n{contents}")),
	);
	sections.push(String::from(
			"Execution discipline\n- Keep pre-edit discovery bounded to the smallest code surface that can satisfy the current issue.\n- Start with the implementation files directly implicated by the issue before reading broader docs or repo-wide guidance.\n- Do not browse upstream references or general repository documentation unless a concrete ambiguity blocks the change.\n- Once the relevant change surface is identified, patch code and run validation instead of continuing broad searches.",
		));
	sections.push(String::from(
		"Docs impact contract\n- Before any phase completion or terminal finalize path, classify docs impact as `none`, `update_required`, `research_required`, or `drift_required`, and record it in a current-HEAD `issue_progress_checkpoint` as `docs_impact`.\n- If behavior, commands, config, schemas, status, validation, workflow, or docs changed, update the owning OKF concept under `docs/` and any required `code_refs`, `drift_watch`, or `docs/evidence/` drift audit evidence.\n- If authority is missing or contradictory, use `research_required`, switch to Decodex `research*`, and keep any `docs/research/` output latent until explicit promotion.\n- Run the repository docs gate when one exists; in this repository use `decodex docs check` or the repo-native command that wraps it.\n- Treat docs-check or semantic-drift failure as a completion blocker. Fix issue-local docs failures in the lane. Do not route pre-existing, repo-wide, or global-baseline docs failures through `manual_attention`; record the blocker in private evidence and let Decodex retain or isolate the baseline lane.",
	));
	sections.push(String::from(
		"Commit contract\n- When you create a local commit for this lane, use a single-line `decodex/commit/1` JSON commit message.\n- Required fields: `schema`, `summary`, and `authority`.\n- `authority` must be the authoritative Linear issue identifier for this lane.\n- Optional fields: `related` and `breaking`.\n- Do not encode landing mode, CI status, closeout state, or other process-state fields in the commit message.",
	));
	sections.push(String::from(
			"Phase goal runtime contract\n- Decodex may set an active phase goal that narrows the immediate turn below the full issue lifecycle checklist.\n- Treat the active phase goal as the authoritative current contract. For every phase, record a current-HEAD `issue_progress_checkpoint` with `docs_impact` before claiming the phase is satisfied.\n- For `implement_to_validation_ready`, `repair_validation_failures`, and `repair_accepted_review_findings`, stop at validated local work, then explicitly complete the active phase goal with the Codex goal completion mechanism so Decodex can run its repo gate and select the next phase.\n- Do not use `issue_progress_checkpoint`, final chat text, or an \"await next phase\" statement as a substitute for completing a satisfied phase goal.\n- The later `handoff_evidence` phase creates or updates the PR and records the normal review handoff terminal path.\n- The later `review_repair_evidence` phase pushes the retained PR repair head, records review-repair completion, and calls `issue_terminal_finalize` with path `review_repair`; it must not call `issue_review_handoff` or move the issue out of its retained review state.",
		));
	sections.push(String::from(
		TRACKER_PUBLIC_TEXT_BOUNDARY_INSTRUCTION,
	));

		sections.push(format!(
				"Tracker tool contract\n- You own issue-scoped tracker writes for `{issue}`.\n- At the start of execution, call `{transition_tool}` to move the issue to `{in_progress}`. Decodex already records the run-start Linear ledger, so do not add a separate start comment.\n- Update `{progress_checkpoint_tool}` whenever the execution phase, docs impact, focus, next action, blockers, evidence, or verification state changes materially.\n- Self Check: Review your work repeatedly and fix any logic bugs until no new issues are found.\n- Before Decodex Review, commit the lane work, rerun required validation, and confirm review-blocking local changes are absent. Formal `{review_checkpoint_tool}` evidence is accepted only for a clean committed `HEAD`.\n- Decodex Review: request an independent fresh-context read-only review pass for the actual committed branch state. The reviewer must not edit files, push, land, or mutate tracker state.\n- Use the registered project workflow policy already injected above as the authoritative review policy source; do not look for or require a repo-local `WORKFLOW.md` unless it was explicitly listed in `context.read_first`.\n- Build an explicit `review_contract` for the checkpoint with `workflow_policy_source = \"registered_project_workflow\"`, `review_type = \"full_current_head_review\"`, the risk tier, objective, scope, non-goals, required checks, allowed expansion triggers, and validation evidence. Include expansion triggers for safety, authority-boundary, data-loss, security, live-mutation, public-API, migration, and operator-facing regressions when relevant.\n- Classify review cost with `review_cost_control`: `compact_current_head_review` is allowed only for low-risk small current-head, validation-backed, clean handoff review after both intended-behavior and adversarial checks; otherwise record `full_current_head_review` with `fallback_reason`. Full review is forced when high-risk surfaces, accepted findings or nonclean rounds, missing or stale validation, docs/config/API/security/data/privacy changes without sufficient evidence, weak evidence, repair review, or architecture risk exists. Compact review is not review skipping; it remains independent fresh-context current-head review.\n- Adjudicate every reviewer signal into `finding_routes` before repair: accepted current repair work must route to `current_blocker`; requests for evidence, follow-up, risk notes, reviewer rubric gaps, architecture signals, issue-contract gaps, landing blockers, and authority blockers must use the matching non-current or landing-blocking route.\n- Preserve reviewer and agent judgment: the reviewer may accept, reject, request evidence, mark follow-up/risk/rubric gaps, or identify architecture/authority blockers, but the runtime must receive structured route evidence before any repair loop uses the signal.\n- Non-current `finding_routes` such as `follow_up`, `risk_note`, `reviewer_rubric_gap`, and `invalid_or_unsubstantiated` are durable evidence and must not drive repair churn.\n- Validate reviewer comments before repair. Fix only accepted findings routed as `current_blocker`, keep the repair batch scoped to the smallest coherent owned set, rerun verification, and re-read `HEAD` before deciding the normalized review status.\n- Every time the Decodex Review pass produces a result for the current committed head, call `{review_checkpoint_tool}` with reviewer `independent_fresh_context`, that normalized status, the exact current `HEAD` SHA, the explicit `review_contract`, `review_cost_control`, concise evidence, checklist notes, structured accepted/rejected findings, and `finding_routes`.\n- Treat repo-native `canonicalize_commands` and `verify_commands` failures as continued repair: keep fixing the lane and rerun the gate. If the repo gate completes but leaves tracked rewrites, do not infer file semantics or widen scope; leave the retained worktree for operator review unless the issue-owned fix already makes the gate clean.\n- When the implementation is ready, commit the lane, push branch `{branch}`, and create or update a non-draft PR titled `{pr_title}` for that branch.\n- Call `{review_handoff_tool}` only after the latest `{review_checkpoint_tool}` for this handoff phase and current `HEAD` is `clean`. Then call `{terminal_finalize_tool}` with path `review_handoff`.\n- If you determine the issue needs human attention, request label `{needs_attention}` with `{label_tool}`; that records manual-attention label intent only. Then call `issue_comment` with kind `manual_attention` and structured public fields (`error_class`, `next_action`, `blockers`, `evidence`; include `failed_command` and `raw_error` only when public-safe). Decodex applies the actual label only after that manual_attention comment validates. Use a human-owned blocker class; do not use runtime-owned retry/repair classes such as app-server timeout, transport, turn, dynamic-tool, or usage-limit failures; stalled-run detection; phase-goal terminal-path misses; repo-gate canonicalize, verify, baseline, tracked-rewrite, or git-lock failures; or generic retryable execution failures. Then call `{terminal_finalize_tool}` with path `manual_attention`. Do not speculate about capabilities you did not directly verify. Do not call `{review_handoff_tool}` in that case; `decodex` will stop the lane as a human-required failure without automatic retry.\n- Do not move the issue directly to `{success}` with `{transition_tool}`. `decodex` will complete the success writeback only after its own validation passes.\n- Do not report the run as complete or treat `{progress_checkpoint_tool}` as terminal completion until `{terminal_finalize_tool}` succeeds.{continuation_guidance}\n- Never write to any other issue.",
			issue = issue_run.issue.identifier,
			transition_tool = ISSUE_TRANSITION_TOOL_NAME,
			label_tool = ISSUE_LABEL_ADD_TOOL_NAME,
			progress_checkpoint_tool = ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME,
			review_checkpoint_tool = ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
			review_handoff_tool = ISSUE_REVIEW_HANDOFF_TOOL_NAME,
			terminal_finalize_tool = ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
			in_progress = workflow.frontmatter().tracker().in_progress_state(),
			branch = issue_run.worktree.branch_name,
			pr_title = orchestrator::review_pull_request_title(&issue_run.issue),
		success = workflow.frontmatter().tracker().success_state(),
		needs_attention = workflow.frontmatter().tracker().needs_attention_label(),
		continuation_guidance = continuation_guidance,
	));

	sections.join("\n\n")
}
