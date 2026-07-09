mod intake_run_prompting_continuation;
mod intake_run_prompting_dispatch;
mod intake_run_prompting_program_dispatch;
mod intake_run_prompting_prompts;

use crate::{
	config::ServiceConfig,
	orchestrator::{
		self, ISSUE_LABEL_ADD_TOOL_NAME, ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
		ISSUE_REVIEW_HANDOFF_TOOL_NAME, ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME, IssueDispatchMode,
		IssueRunPlan, TargetIssueRunContext,
		tests::{self, FakeTracker, TEST_SERVICE_ID},
	},
	state::StateStore,
	tracker::{self, TrackerIssue},
	workflow::WorkflowDocument,
	worktree::WorktreeSpec,
};

struct PromptSurfaces {
	developer_instructions: String,
	user_input: String,
	continuation_input: String,
}
impl PromptSurfaces {
	fn all(&self) -> [&str; 3] {
		[
			self.developer_instructions.as_str(),
			self.user_input.as_str(),
			self.continuation_input.as_str(),
		]
	}
}

fn assert_manual_attention_prompt_guidance(prompt: &str, expects_handoff_guard: bool) {
	assert!(prompt.contains(&format!(
		"request label `decodex:needs-attention` with `{ISSUE_LABEL_ADD_TOOL_NAME}`"
	)));
	assert!(prompt.contains("records manual-attention label intent only"));
	assert!(prompt.contains(
		"Decodex applies the actual label only after that manual_attention comment validates"
	));
	assert!(prompt.contains("do not use runtime-owned retry/repair classes"));
	assert!(prompt.contains("app-server timeout, transport, turn, dynamic-tool, or usage-limit"));
	assert!(prompt.contains("stalled-run detection"));
	assert!(prompt.contains("phase-goal terminal-path misses"));
	assert!(prompt.contains(
		"repo-gate canonicalize, verify, baseline, tracked-rewrite, or git-lock failures"
	));
	assert!(prompt.contains("generic retryable execution failures"));
	assert!(!prompt.contains("add label `decodex:needs-attention`"));
	assert!(!prompt.contains("add the needs-attention label"));

	if expects_handoff_guard {
		assert!(
			prompt
				.contains(&format!("Do not call `{ISSUE_REVIEW_HANDOFF_TOOL_NAME}` in that case"))
		);
	}
}

fn assert_runtime_owned_review_prompt_guidance(prompt: &str) {
	assert!(prompt.contains(ISSUE_REVIEW_CHECKPOINT_TOOL_NAME));
	assert!(prompt.contains("Do not request Decodex Review yourself"));
	assert!(prompt.contains("do not call `issue_review_checkpoint`"));
	assert!(prompt.contains("Decodex owns the independent current-head"));
}

fn assert_review_repair_developer_prompt(prompt: &str) {
	assert!(prompt.contains(ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME));
	assert!(prompt.contains("Do not move the issue back to `In Progress`"));
	assert!(prompt.contains("do not call `issue_review_handoff`"));

	assert_runtime_owned_review_prompt_guidance(prompt);

	assert!(prompt.contains("registered project workflow policy"));
	assert!(prompt.contains(
		"including non-thread review summaries, validate the claim against the codebase, tests, and requirements"
	));
	assert!(prompt.contains(
		"After the repaired head is pushed, reply in-thread for every addressed comment"
	));
	assert!(prompt.contains("retained landing fallback"));
	assert!(prompt.contains("Do not merge or land the PR yourself"));
}

fn assert_review_repair_user_prompt(prompt: &str, pr_url: &str) {
	assert!(prompt.contains(pr_url));

	assert_runtime_owned_review_prompt_guidance(prompt);

	assert!(prompt.contains(
		"Read the current review feedback on `https://github.com/hack-ink/decodex/pull/77`, including non-thread review summaries"
	));
	assert!(
		prompt.contains(
			"validate each actionable claim against the codebase, tests, and requirements"
		)
	);
	assert!(prompt.contains("Leave pushback or clarification threads open"));
	assert!(prompt.contains("because retained landing was not a deterministic clean path"));
	assert!(prompt.contains("Do not merge or land the PR yourself"));
	assert!(prompt.contains(
		"resolve only the GitHub review threads whose fixes landed and verified on the repaired head"
	));
}

fn assert_review_repair_continuation_prompt(prompt: &str) {
	assert!(prompt.contains("Resume by committing any review-blocking repair edits"));

	assert_runtime_owned_review_prompt_guidance(prompt);

	assert!(prompt.contains(
		"Validate each actionable review claim against the codebase, tests, and requirements before changing code"
	));
	assert!(
		prompt.contains(
			"keep pushback or clarification threads open until the repaired head is ready"
		)
	);
	assert!(prompt.contains("retained landing fallback"));
	assert!(prompt.contains("do not merge or land the PR yourself"));
	assert!(prompt.contains("Do not request GitHub Review yourself"));
	assert!(prompt.contains("In Review"));
	assert!(prompt.contains("review_repair"));
}

fn run_and_prompting_service_owned_issue(state_name: &str) -> TrackerIssue {
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);

	tests::sample_issue(state_name, &[active_label.as_str()])
}

fn run_and_prompting_target_context<'a, T>(
	tracker: &'a T,
	config: &'a ServiceConfig,
	workflow: &'a WorkflowDocument,
	state_store: &'a StateStore,
	issue_identifier: &'a str,
	dispatch_mode: IssueDispatchMode,
) -> TargetIssueRunContext<'a, T> {
	TargetIssueRunContext {
		tracker,
		project: config,
		workflow,
		state_store,
		issue_id: issue_identifier,
		preferred_issue_state: None,
		preferred_initial_issue_state: None,
		dry_run: true,
		lease_preacquired: false,
		preferred_issue_claim_fd: None,
		preferred_dispatch_slot_fd: None,
		preferred_dispatch_slot_index: None,
		dispatch_mode,
		preferred_run_identity: None,
		preferred_retry_budget_base: None,
	}
}

fn assert_prompt_orders_thread_replies_after_push(prompt: &str, push_requirement: &str) {
	let push_index =
		prompt.find(push_requirement).expect("prompt should require push before thread resolution");
	let reply_index = prompt
		.find("After the repaired head is pushed, reply in-thread for every addressed comment")
		.expect("prompt should place thread replies after push");

	assert!(push_index < reply_index);
}

fn build_normal_prompt_surfaces(
	config: &ServiceConfig,
	workflow: &WorkflowDocument,
) -> PromptSurfaces {
	let issue = tests::sample_issue("Todo", &[]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: String::from("In Progress"),
		initial_issue_state: String::from("Todo"),
		worktree: WorktreeSpec {
			branch_name: String::from("x/pubfi-pub-101"),
			issue_identifier: String::from("PUB-101"),
			path: config.worktree_root().join("PUB-101"),
			reused_existing: false,
		},
		retry_project_slug: String::from("pubfi"),
		dispatch_mode: IssueDispatchMode::Normal,
		attempt_number: 1,
		run_id: String::from("pub-101-attempt-1-123"),
		retry_budget_base: 0,
	};
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let developer_instructions = orchestrator::build_developer_instructions(
		&tracker,
		config,
		workflow,
		&issue_run,
		&state_store,
		None,
	)
	.expect("developer instructions should build");
	let user_input = orchestrator::build_user_input(
		&tracker,
		config,
		&issue,
		workflow,
		&issue_run,
		&state_store,
		None,
	);
	let continuation_input = orchestrator::build_continuation_user_input(
		&issue,
		workflow,
		IssueDispatchMode::Normal,
		None,
		workflow.frontmatter().tracker().success_state(),
		config.codex().review_level(),
	);

	PromptSurfaces { developer_instructions, user_input, continuation_input }
}

fn normal_prompt_issue_run(config: &ServiceConfig, issue: TrackerIssue) -> IssueRunPlan {
	IssueRunPlan {
		issue: issue.clone(),
		issue_state: String::from("In Progress"),
		initial_issue_state: String::from("Todo"),
		worktree: WorktreeSpec {
			branch_name: String::from("x/pubfi-pub-101"),
			issue_identifier: String::from("PUB-101"),
			path: config.worktree_root().join("PUB-101"),
			reused_existing: false,
		},
		retry_project_slug: String::from("pubfi"),
		dispatch_mode: IssueDispatchMode::Normal,
		attempt_number: 1,
		run_id: String::from("pub-101-attempt-1-123"),
		retry_budget_base: 0,
	}
}
