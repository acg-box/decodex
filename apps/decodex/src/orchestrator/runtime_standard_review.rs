use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use crate::{
	agent::{
		self, AppServerProcessEnv, AppServerRunRequest, RUN_LEASE_IDLE_TIMEOUT, ReviewExecutionMode,
	},
	orchestrator::{
		IssueTracker, Result, RetainedReviewLane, ServiceConfig, StateStore, TrackerToolBridge,
		WorkflowDocument, configured_public_projection_privacy_classifier,
	},
	prelude::eyre,
};

pub(crate) trait RuntimeStandardReviewRunner {
	fn run_runtime_standard_review(
		&self,
		request: RuntimeStandardReviewRunRequest<'_>,
	) -> Result<String>;
}

pub(crate) struct RuntimeStandardReviewRunRequest<'a> {
	project: &'a ServiceConfig,
	workflow: &'a WorkflowDocument,
	lane: &'a RetainedReviewLane,
	review_mode: ReviewExecutionMode,
	review_run_id: &'a str,
	head_sha: &'a str,
}
#[cfg(test)]
impl RuntimeStandardReviewRunRequest<'_> {
	pub(crate) fn review_mode(&self) -> ReviewExecutionMode {
		self.review_mode
	}

	pub(crate) fn head_sha(&self) -> &str {
		self.head_sha
	}
}

pub(crate) struct AppServerRuntimeStandardReviewRunner<'a> {
	state_store: &'a StateStore,
}
impl<'a> AppServerRuntimeStandardReviewRunner<'a> {
	pub(crate) fn new(state_store: &'a StateStore) -> Self {
		Self { state_store }
	}
}

impl RuntimeStandardReviewRunner for AppServerRuntimeStandardReviewRunner<'_> {
	fn run_runtime_standard_review(
		&self,
		request: RuntimeStandardReviewRunRequest<'_>,
	) -> Result<String> {
		let snapshot = request.lane.snapshot();
		let marker = request.lane.lifecycle_record();
		let run_result = agent::execute_app_server_run(
			&AppServerRunRequest {
				project_id: request.project.service_id().to_owned(),
				run_id: request.review_run_id.to_owned(),
				issue_id: snapshot.issue.id.clone(),
				attempt_number: marker.attempt_number(),
				listen: request.workflow.frontmatter().agent().transport().to_owned(),
				cwd: snapshot.worktree.worktree_path().display().to_string(),
				developer_instructions: runtime_standard_review_developer_instructions(
					request.workflow,
					request.review_mode,
				)?,
				user_input: runtime_standard_review_user_input(
					request.project,
					request.lane,
					request.review_mode,
					request.head_sha,
				),
				max_turns: 1,
				timeout: RUN_LEASE_IDLE_TIMEOUT,
				process_env: AppServerProcessEnv::default(),
				continuation_user_input: None,
				activity_marker_path: None,
				resume_thread_id: None,
				ephemeral_thread: true,
				command_exec_health_check: None,
				dynamic_tool_handler: None,
				continuation_guard: None,
				phase_goal_controller: None,
				codex_account_provider: None,
			},
			self.state_store,
		)?;

		Ok(run_result.final_output)
	}
}

pub(crate) fn ensure_runtime_standard_review_checkpoint_with_runner<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	lane: &RetainedReviewLane,
	runner: &impl RuntimeStandardReviewRunner,
) -> Result<()>
where
	T: IssueTracker,
{
	let snapshot = lane.snapshot();
	let marker = lane.lifecycle_record();
	let Some(head_sha) = snapshot.local_head_oid.as_deref() else {
		return Ok(());
	};
	let review_mode = runtime_review_execution_mode(project, state_store, lane)?;
	let review_run_id = runtime_review_run_id(marker.run_id(), review_mode, head_sha);
	let privacy_classifier = configured_public_projection_privacy_classifier(project)?;
	let review_context = crate::agent::ReviewHandoffContext {
		attempt_number: marker.attempt_number(),
		branch_name: marker.branch_name().to_owned(),
		run_id: review_run_id.clone(),
		service_id: project.service_id().to_owned(),
		worktree_path: snapshot.worktree.worktree_path().display().to_string(),
		cwd: PathBuf::from(snapshot.worktree.worktree_path()),
		github_token_env_var: Some(project.github().token_env_var().to_owned()),
		github_command_path: project.github().command_path().map(Path::to_path_buf),
		review_level: project.codex().review_level(),
		mode: review_mode,
		recorded_pr_url: Some(marker.pr_url().to_owned()),
	};
	let tracker_tool_bridge =
		TrackerToolBridge::with_run_context_state_store_and_privacy_classifier(
			tracker,
			&snapshot.issue,
			workflow,
			review_context,
			state_store,
			&privacy_classifier,
		);
	let final_output = runner.run_runtime_standard_review(RuntimeStandardReviewRunRequest {
		project,
		workflow,
		lane,
		review_mode,
		review_run_id: &review_run_id,
		head_sha,
	})?;

	record_runtime_standard_review_checkpoint_from_output(
		&tracker_tool_bridge,
		&snapshot.issue.id,
		&snapshot.issue.identifier,
		head_sha,
		review_mode,
		&final_output,
	)
}

pub(crate) fn runtime_review_execution_mode(
	project: &ServiceConfig,
	state_store: &StateStore,
	lane: &RetainedReviewLane,
) -> Result<ReviewExecutionMode> {
	if lane.lifecycle_record().external_round_count() > 0 {
		return Ok(ReviewExecutionMode::Repair);
	}

	for phase in ["repair", "handoff"] {
		if state_store.has_nonclean_review_checkpoint_artifact(
			project.service_id(),
			&lane.snapshot().issue.id,
			phase,
		)? {
			return Ok(ReviewExecutionMode::Repair);
		}
	}

	Ok(ReviewExecutionMode::Handoff)
}

fn runtime_review_run_id(
	source_run_id: &str,
	review_mode: ReviewExecutionMode,
	head_sha: &str,
) -> String {
	let short_head = head_sha.chars().take(12).collect::<String>();

	format!("{}:runtime-review:{}:{}", source_run_id, review_mode.as_str(), short_head)
}

fn runtime_standard_review_developer_instructions(
	workflow: &WorkflowDocument,
	review_mode: ReviewExecutionMode,
) -> Result<String> {
	let workflow_markdown = workflow.to_markdown()?;
	let review_type = runtime_review_type(review_mode);

	Ok(format!(
		"Runtime-owned Decodex Review\n\
		- You are an independent fresh-context reviewer for the current committed HEAD.\n\
		- Work read-only: do not edit files, commit, push, merge, land, transition tracker state, or write comments.\n\
		- Inspect the issue contract, repository workflow policy, current diff, current HEAD, and relevant tests.\n\
		- Return exactly one JSON object and no surrounding prose.\n\
		- The runtime will inject issue scope, reviewer identity, reviewed head, and review_contract; do not include or override those fields.\n\
		- Use status `clean` only when no current landing-blocking repair remains.\n\
		- Use status `findings` for actionable current-head implementation repairs and include accepted_findings plus finding_routes routed to `current_blocker`.\n\
		- Use status `needs_architecture_review` or `blocked` only when the runtime should fail closed for human architecture or external unblocker input.\n\
		- This checkpoint review_type is `{review_type}`.\n\n\
		Required JSON shape:\n\
		{{\n\
		  \"status\": \"clean|findings|needs_architecture_review|blocked\",\n\
		  \"checks\": {{\n\
		    \"intended_behavior\": \"...\",\n\
		    \"regression_risk\": \"...\",\n\
		    \"missing_tests\": \"...\",\n\
		    \"docs_config_drift\": \"...\",\n\
		    \"migration_fallout\": \"...\",\n\
		    \"operator_facing_fallout\": \"...\",\n\
		    \"loop_decision_contract\": \"...\"\n\
		  }},\n\
		  \"evidence\": [\"...\"],\n\
		  \"accepted_findings\": [],\n\
		  \"rejected_findings\": [],\n\
		  \"finding_routes\": [],\n\
		  \"review_cost_control\": {{\n\
		    \"review_class\": \"full_current_head_review\",\n\
		    \"risk_class\": \"localized\",\n\
		    \"changed_surface_count\": 1,\n\
		    \"changed_surface_summary\": [\"...\"],\n\
		    \"high_risk_surfaces\": [],\n\
		    \"current_head_evidence\": true,\n\
		    \"validation_backed\": true,\n\
		    \"validation_current\": true,\n\
		    \"evidence_sufficient\": true,\n\
		    \"reviewer_judgment\": \"...\",\n\
		    \"fallback_reason\": \"runtime_full_review\"\n\
		  }}\n\
		}}\n\n\
		Workflow policy snapshot:\n{workflow_markdown}"
	))
}

fn runtime_standard_review_user_input(
	project: &ServiceConfig,
	lane: &RetainedReviewLane,
	review_mode: ReviewExecutionMode,
	head_sha: &str,
) -> String {
	format!(
		"Review the retained Decodex lane.\n\n\
		- project_id: `{}`\n\
		- issue_id: `{}`\n\
		- issue_identifier: `{}`\n\
		- issue_title: `{}`\n\
		- pr_url: `{}`\n\
		- branch: `{}`\n\
		- head_sha: `{}`\n\
		- review_phase: `{}`\n\
		- review_type: `{}`\n\n\
		Return only the checkpoint JSON object.",
		project.service_id(),
		lane.snapshot().issue.id,
		lane.snapshot().issue.identifier,
		lane.snapshot().issue.title,
		lane.lifecycle_record().pr_url(),
		lane.lifecycle_record().branch_name(),
		head_sha,
		review_mode.as_str(),
		runtime_review_type(review_mode),
	)
}

pub(crate) fn record_runtime_standard_review_checkpoint_from_output(
	tracker_tool_bridge: &TrackerToolBridge<'_>,
	issue_id: &str,
	issue_identifier: &str,
	head_sha: &str,
	review_mode: ReviewExecutionMode,
	final_output: &str,
) -> Result<()> {
	let checkpoint = checkpoint_json_from_reviewer_output(final_output)?;
	let arguments = runtime_review_checkpoint_arguments(
		checkpoint,
		issue_id,
		issue_identifier,
		head_sha,
		review_mode,
	)?;

	tracker_tool_bridge.record_runtime_review_checkpoint(arguments)
}

fn checkpoint_json_from_reviewer_output(final_output: &str) -> Result<Value> {
	let trimmed = final_output.trim();

	if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
		return Ok(value);
	}

	let without_fence = trimmed
		.strip_prefix("```json")
		.or_else(|| trimmed.strip_prefix("```"))
		.and_then(|body| body.strip_suffix("```"))
		.map(str::trim);

	if let Some(without_fence) = without_fence
		&& let Ok(value) = serde_json::from_str::<Value>(without_fence)
	{
		return Ok(value);
	}

	let Some(start) = trimmed.find('{') else {
		eyre::bail!("Runtime Decodex Review did not return a JSON object.")
	};
	let Some(end) = trimmed.rfind('}') else {
		eyre::bail!("Runtime Decodex Review returned incomplete JSON.")
	};

	serde_json::from_str::<Value>(&trimmed[start..=end])
		.map_err(|error| eyre::eyre!("Runtime Decodex Review returned invalid JSON: {error}"))
}

fn runtime_review_checkpoint_arguments(
	checkpoint: Value,
	issue_id: &str,
	issue_identifier: &str,
	head_sha: &str,
	review_mode: ReviewExecutionMode,
) -> Result<Value> {
	let checkpoint = checkpoint.get("checkpoint").cloned().unwrap_or(checkpoint);
	let Value::Object(mut object) = checkpoint else {
		eyre::bail!("Runtime Decodex Review JSON must be an object.")
	};

	insert_runtime_owned_checkpoint_fields(
		&mut object,
		issue_id,
		issue_identifier,
		head_sha,
		review_mode,
	);

	Ok(Value::Object(object))
}

fn insert_runtime_owned_checkpoint_fields(
	object: &mut Map<String, Value>,
	issue_id: &str,
	issue_identifier: &str,
	head_sha: &str,
	review_mode: ReviewExecutionMode,
) {
	object.insert(String::from("issue_id"), Value::String(issue_id.to_owned()));
	object.insert(String::from("issue_identifier"), Value::String(issue_identifier.to_owned()));
	object
		.insert(String::from("reviewer"), Value::String(String::from("independent_fresh_context")));
	object.insert(String::from("head_sha"), Value::String(head_sha.to_owned()));
	object.insert(String::from("review_contract"), runtime_review_contract_json(review_mode));
}

fn runtime_review_contract_json(review_mode: ReviewExecutionMode) -> Value {
	json!({
		"workflow_policy_source": "registered_project_workflow",
		"review_type": runtime_review_type(review_mode),
		"risk_tier": "localized",
		"objective": "Review the current committed PR head against the registered Decodex workflow and issue objective.",
		"scope": [
			"Current committed lane HEAD",
			"PR head lineage and review-blocking changed surface",
			"Validation, docs/config drift, operator-facing fallout, and lifecycle impact"
		],
		"non_goals": [
			"Do not edit files",
			"Do not transition tracker state",
			"Do not merge or land the PR"
		],
		"required_checks": [
			"Current HEAD matches the reviewed checkpoint head",
			"Review-blocking worktree changes are absent before checkpoint writeback",
			"Findings are routed before they can drive repair or landing"
		],
		"allowed_expansion_triggers": [
			"Evidence of current-head regression",
			"Missing validation for touched behavior",
			"Docs/config/runtime lifecycle drift"
		],
		"validation_evidence": [
			"Reviewer inspected current HEAD and repository evidence",
			"Runtime checkpoint normalization validates evidence and route structure"
		]
	})
}

fn runtime_review_type(review_mode: ReviewExecutionMode) -> &'static str {
	match review_mode {
		ReviewExecutionMode::Handoff => "full_current_head_review",
		ReviewExecutionMode::Repair => "repair_verification",
		ReviewExecutionMode::Closeout => "full_current_head_review",
	}
}

#[cfg(test)]
mod tests {
	use crate::orchestrator::runtime_standard_review::{
		checkpoint_json_from_reviewer_output, runtime_review_checkpoint_arguments,
	};

	#[test]
	fn runtime_review_output_parser_accepts_fenced_json() {
		let parsed = checkpoint_json_from_reviewer_output(
			r#"```json
{"status":"clean","checks":{"intended_behavior":"ok","regression_risk":"low","missing_tests":"none","docs_config_drift":"none","migration_fallout":"none","operator_facing_fallout":"none","loop_decision_contract":"ok"},"evidence":["read current HEAD"]}
```"#,
		)
		.expect("fenced json should parse");

		assert_eq!(parsed["status"], "clean");
	}

	#[test]
	fn runtime_review_arguments_override_authority_binding_fields() {
		let arguments = runtime_review_checkpoint_arguments(
			serde_json::json!({
				"issue_id": "wrong",
				"issue_identifier": "WRONG-1",
				"reviewer": "implementing_agent",
				"head_sha": "old",
				"status": "clean",
				"checks": {
					"intended_behavior": "ok",
					"regression_risk": "low",
					"missing_tests": "none",
					"docs_config_drift": "none",
					"migration_fallout": "none",
					"operator_facing_fallout": "none",
					"loop_decision_contract": "ok"
				},
				"evidence": ["read current HEAD"]
			}),
			"issue-1",
			"PUB-101",
			"head-sha",
			crate::agent::ReviewExecutionMode::Handoff,
		)
		.expect("arguments should build");

		assert_eq!(arguments["issue_id"], "issue-1");
		assert_eq!(arguments["issue_identifier"], "PUB-101");
		assert_eq!(arguments["reviewer"], "independent_fresh_context");
		assert_eq!(arguments["head_sha"], "head-sha");
		assert_eq!(arguments["review_contract"]["review_type"], "full_current_head_review");
	}
}
