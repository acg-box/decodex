use super::{
	AUTHORITY_BOUNDARY_CHECK_EVENT_TYPE, AuthorityDecisionOption, AuthorityDecisionOptionArgs,
	AuthorityDecisionRequestArgs, AuthorityDecisionRequestInput, COMMENT_KIND_MANUAL_ATTENTION,
	DynamicToolCallResponse, ISSUE_COMMENT_TOOL_NAME, ISSUE_LABEL_ADD_TOOL_NAME,
	LinearExecutionEventIdentity, LinearExecutionEventRecord, MANUAL_ATTENTION_TERMINAL_PATH,
	ReviewHandoffContext, StateStore, TrackerToolBridge, Value, orchestrator, public_text, records,
	serde_json, tracker, tracker_tool_bridge,
};
use crate::agent::tracker_tool_bridge::CommentArgs;

#[derive(Debug)]
struct NormalizedManualAttentionComment {
	error_class: String,
	next_action: String,
	blockers: Vec<String>,
	evidence: Vec<String>,
	failed_command: Option<String>,
	raw_error: Option<String>,
	summary: Option<String>,
	decision_request: Option<NormalizedAuthorityDecisionRequest>,
}

#[derive(Debug)]
struct NormalizedAuthorityDecisionRequest {
	boundary_check_id: i64,
	decision_request_id: String,
	reason_code: String,
	boundary_type: String,
	proposed_change: String,
	why_exceeds_authority: String,
	options: Vec<NormalizedAuthorityDecisionOption>,
	recommendation: String,
	resume_condition: String,
	retained_worktree_evidence: Vec<String>,
	retained_diff_evidence: Vec<String>,
	recovery_attempt_context: Vec<String>,
}

#[derive(Debug)]
struct NormalizedAuthorityDecisionOption {
	label: String,
	description: String,
}

impl<'a> TrackerToolBridge<'a> {
	pub(super) fn handle_comment(&self, arguments: Value) -> DynamicToolCallResponse {
		let parsed = match serde_json::from_value::<CommentArgs>(arguments) {
			Ok(parsed) => parsed,
			Err(error) => {
				return DynamicToolCallResponse::failure(format!(
					"Invalid `issue.comment` arguments: {error}"
				));
			},
		};

		if let Err(error) = self.ensure_issue_scope(&parsed.scope) {
			return DynamicToolCallResponse::failure(error);
		}

		match parsed.kind.as_str() {
			COMMENT_KIND_MANUAL_ATTENTION => self.handle_manual_attention_comment(parsed),
			other => DynamicToolCallResponse::failure(format!(
				"Unsupported `{ISSUE_COMMENT_TOOL_NAME}` kind `{other}`. Supported kinds: `{COMMENT_KIND_MANUAL_ATTENTION}`."
			)),
		}
	}

	fn handle_manual_attention_comment(&self, parsed: CommentArgs) -> DynamicToolCallResponse {
		if !*self.manual_attention_requested.borrow() {
			return DynamicToolCallResponse::failure(format!(
				"`{ISSUE_COMMENT_TOOL_NAME}` kind `{COMMENT_KIND_MANUAL_ATTENTION}` requires a successful `{ISSUE_LABEL_ADD_TOOL_NAME}` call for label `{}` before writing the explanatory comment.",
				self.workflow.frontmatter().tracker().needs_attention_label()
			));
		}

		let review_context = match self.review_context.as_ref() {
			Some(review_context) => review_context,
			None => {
				return DynamicToolCallResponse::failure(format!(
					"`{ISSUE_COMMENT_TOOL_NAME}` kind `{COMMENT_KIND_MANUAL_ATTENTION}` requires an active Decodex run context."
				));
			},
		};
		let state_store = match self.state_store {
			Some(state_store) => state_store,
			None => {
				return DynamicToolCallResponse::failure(format!(
					"`{ISSUE_COMMENT_TOOL_NAME}` kind `{COMMENT_KIND_MANUAL_ATTENTION}` requires the Decodex runtime state store for issue `{}`.",
					self.issue.identifier
				));
			},
		};
		let comment = match Self::normalize_manual_attention_comment(parsed) {
			Ok(comment) => comment,
			Err(error) => return DynamicToolCallResponse::failure(error),
		};

		if let Some(decision_request) = comment.decision_request.as_ref()
			&& let Err(error) = self.append_private_authority_decision_request(
				review_context,
				state_store,
				decision_request,
			) {
			return DynamicToolCallResponse::failure(error);
		}

		let record = self.manual_attention_execution_event(review_context, &comment);
		let body = format_manual_attention_comment(review_context, &comment);
		let projection = match tracker::prepare_linear_execution_event_comment(
			&body,
			&record,
			self.public_projection_privacy_classifier,
		) {
			Ok(projection) => projection,
			Err(error) => return DynamicToolCallResponse::failure(error.to_string()),
		};

		if let Err(error) = self.apply_manual_attention_label() {
			return DynamicToolCallResponse::failure(error);
		}

		match tracker::create_prepared_linear_execution_event_comment(
			self.tracker,
			&self.issue.id,
			&projection,
		) {
			Ok(created) => {
				if let Err(error) = state_store.record_linear_execution_event(&projection.record) {
					return DynamicToolCallResponse::failure(format!(
						"Failed to persist the public manual-attention summary for issue `{}`: {error}",
						self.issue.identifier
					));
				}

				self.manual_attention_comment_recorded.replace(true);
				self.manual_attention_error_class.replace(Some(comment.error_class.clone()));

				let verb = if created { "added" } else { "already existed for" };

				DynamicToolCallResponse::success(format!(
					"Manual-attention public summary {verb} issue `{}`.",
					self.issue.identifier
				))
			},
			Err(error) => DynamicToolCallResponse::failure(format!(
				"Failed to add a manual-attention public summary to issue `{}`: {error}",
				self.issue.identifier
			)),
		}
	}

	fn normalize_manual_attention_comment(
		parsed: CommentArgs,
	) -> Result<NormalizedManualAttentionComment, String> {
		let error_class = normalize_required_comment_field(parsed.error_class, "error_class")?;
		let next_action = normalize_required_comment_field(parsed.next_action, "next_action")?;
		let blockers = tracker_tool_bridge::normalize_progress_list(parsed.blockers);
		let evidence = tracker_tool_bridge::normalize_progress_list(parsed.evidence);
		let failed_command =
			tracker_tool_bridge::normalize_optional_progress_field(parsed.failed_command);
		let raw_error = tracker_tool_bridge::normalize_optional_progress_field(parsed.raw_error);
		let summary = tracker_tool_bridge::normalize_optional_progress_field(parsed.summary);
		let decision_request =
			parsed.decision_request.map(Self::normalize_authority_decision_request).transpose()?;

		validate_manual_attention_error_class(&error_class)?;

		if blockers.is_empty() {
			return Err(format!(
				"`{ISSUE_COMMENT_TOOL_NAME}` kind `{COMMENT_KIND_MANUAL_ATTENTION}` requires at least one public `blockers` item."
			));
		}
		if evidence.is_empty() {
			return Err(format!(
				"`{ISSUE_COMMENT_TOOL_NAME}` kind `{COMMENT_KIND_MANUAL_ATTENTION}` requires at least one public `evidence` item."
			));
		}

		Ok(NormalizedManualAttentionComment {
			error_class,
			next_action,
			blockers,
			evidence,
			failed_command,
			raw_error,
			summary,
			decision_request,
		})
	}

	fn apply_manual_attention_label(&self) -> Result<(), String> {
		let label = self.workflow.frontmatter().tracker().needs_attention_label();
		let current_issue = match self.refreshed_issue_snapshot() {
			Ok(Some(issue)) => issue,
			Ok(None) => {
				return Err(format!(
					"Failed to refresh issue `{}` before applying manual-attention label `{label}`: tracker returned no current snapshot.",
					self.issue.identifier
				));
			},
			Err(error) => {
				return Err(format!(
					"Failed to refresh issue `{}` before applying manual-attention label `{label}`: {error}",
					self.issue.identifier
				));
			},
		};

		tracker::set_issue_label_presence(self.tracker, &current_issue, label, true).map_err(
			|error| {
				format!(
					"Failed to add label `{label}` to issue `{}`: {error}",
					self.issue.identifier
				)
			},
		)?;

		Ok(())
	}

	fn normalize_authority_decision_request(
		parsed: AuthorityDecisionRequestArgs,
	) -> Result<NormalizedAuthorityDecisionRequest, String> {
		let decision_request_id = normalize_required_decision_request_field(
			Some(parsed.decision_request_id),
			"decision_request_id",
		)?;
		let reason_code =
			normalize_required_decision_request_field(Some(parsed.reason_code), "reason_code")?;
		let boundary_type =
			normalize_required_decision_request_field(Some(parsed.boundary_type), "boundary_type")?;
		let proposed_change = normalize_required_decision_request_field(
			Some(parsed.proposed_change),
			"proposed_change",
		)?;
		let why_exceeds_authority = normalize_required_decision_request_field(
			Some(parsed.why_exceeds_authority),
			"why_exceeds_authority",
		)?;
		let recommendation = normalize_required_decision_request_field(
			Some(parsed.recommendation),
			"recommendation",
		)?;
		let resume_condition = normalize_required_decision_request_field(
			Some(parsed.resume_condition),
			"resume_condition",
		)?;
		let options = parsed
			.options
			.into_iter()
			.map(Self::normalize_authority_decision_option)
			.collect::<Result<Vec<_>, _>>()?;
		let retained_worktree_evidence =
			tracker_tool_bridge::normalize_progress_list(parsed.retained_worktree_evidence);
		let retained_diff_evidence =
			tracker_tool_bridge::normalize_progress_list(parsed.retained_diff_evidence);
		let recovery_attempt_context =
			tracker_tool_bridge::normalize_progress_list(parsed.recovery_attempt_context);

		if parsed.boundary_check_id < 1 {
			return Err(String::from(
				"`decision_request.boundary_check_id` must be a positive private evidence record id.",
			));
		}

		validate_public_error_class(&reason_code)?;
		validate_public_error_class(&boundary_type)?;

		if options.is_empty() {
			return Err(String::from(
				"`decision_request.options` must include at least one public option.",
			));
		}

		validate_public_decision_request_text(
			&decision_request_id,
			&proposed_change,
			&why_exceeds_authority,
			&options,
			&recommendation,
			&resume_condition,
		)?;

		Ok(NormalizedAuthorityDecisionRequest {
			boundary_check_id: parsed.boundary_check_id,
			decision_request_id,
			reason_code,
			boundary_type,
			proposed_change,
			why_exceeds_authority,
			options,
			recommendation,
			resume_condition,
			retained_worktree_evidence,
			retained_diff_evidence,
			recovery_attempt_context,
		})
	}

	fn normalize_authority_decision_option(
		parsed: AuthorityDecisionOptionArgs,
	) -> Result<NormalizedAuthorityDecisionOption, String> {
		let label = normalize_required_decision_request_field(Some(parsed.label), "option.label")?;
		let description = normalize_required_decision_request_field(
			Some(parsed.description),
			"option.description",
		)?;

		validate_public_decision_request_field("decision_request.option.label", &label)?;
		validate_public_decision_request_field(
			"decision_request.option.description",
			&description,
		)?;

		Ok(NormalizedAuthorityDecisionOption { label, description })
	}

	fn append_private_authority_decision_request(
		&self,
		review_context: &ReviewHandoffContext,
		state_store: &StateStore,
		decision_request: &NormalizedAuthorityDecisionRequest,
	) -> Result<(), String> {
		let boundary_events = state_store
			.list_private_execution_events(
				&review_context.service_id,
				&self.issue.id,
				&review_context.run_id,
				review_context.attempt_number,
			)
			.map_err(|error| {
				format!(
					"Failed to inspect authority boundary evidence for issue `{}`: {error}",
					self.issue.identifier
				)
			})?;
		let Some(boundary_event) = boundary_events
			.iter()
			.find(|event| event.record_id() == decision_request.boundary_check_id)
		else {
			return Err(format!(
				"`decision_request.boundary_check_id` {} does not reference a private event for issue `{}` run `{}` attempt {}.",
				decision_request.boundary_check_id,
				self.issue.identifier,
				review_context.run_id,
				review_context.attempt_number
			));
		};

		if boundary_event.event_type() != AUTHORITY_BOUNDARY_CHECK_EVENT_TYPE {
			return Err(format!(
				"`decision_request.boundary_check_id` {} references `{}` instead of an authority boundary check.",
				decision_request.boundary_check_id,
				boundary_event.event_type()
			));
		}

		let disposition = boundary_event.payload().get("disposition").and_then(Value::as_str);

		if disposition != Some("requires_human") {
			return Err(format!(
				"`decision_request.boundary_check_id` {} must reference a `requires_human` authority boundary check.",
				decision_request.boundary_check_id
			));
		}

		let options = decision_request
			.options
			.iter()
			.map(|option| AuthorityDecisionOption {
				label: option.label.as_str(),
				description: option.description.as_str(),
			})
			.collect::<Vec<_>>();
		let retained_worktree_evidence = decision_request
			.retained_worktree_evidence
			.iter()
			.map(String::as_str)
			.collect::<Vec<_>>();
		let retained_diff_evidence =
			decision_request.retained_diff_evidence.iter().map(String::as_str).collect::<Vec<_>>();
		let recovery_attempt_context = decision_request
			.recovery_attempt_context
			.iter()
			.map(String::as_str)
			.collect::<Vec<_>>();

		orchestrator::record_authority_decision_request_private_event(
			state_store,
			AuthorityDecisionRequestInput {
				project_id: &review_context.service_id,
				issue_id: &self.issue.id,
				issue_identifier: &self.issue.identifier,
				run_id: &review_context.run_id,
				attempt_number: review_context.attempt_number,
				boundary_check_record_id: decision_request.boundary_check_id,
				decision_request_id: &decision_request.decision_request_id,
				reason_code: &decision_request.reason_code,
				boundary_type: &decision_request.boundary_type,
				proposed_change: &decision_request.proposed_change,
				why_exceeds_authority: &decision_request.why_exceeds_authority,
				options,
				recommendation: &decision_request.recommendation,
				resume_condition: &decision_request.resume_condition,
				retained_worktree_evidence,
				retained_diff_evidence,
				recovery_attempt_context,
			},
		)
		.map(|_| ())
		.map_err(|error| {
			format!(
				"Failed to persist authority decision request `{}` for issue `{}`: {error}",
				decision_request.decision_request_id, self.issue.identifier
			)
		})
	}

	fn manual_attention_execution_event(
		&self,
		review_context: &ReviewHandoffContext,
		comment: &NormalizedManualAttentionComment,
	) -> LinearExecutionEventRecord {
		let decision_request_id = comment
			.decision_request
			.as_ref()
			.map(|request| request.decision_request_id.as_str())
			.unwrap_or_default();
		let anchor = records::stable_event_anchor(&[
			COMMENT_KIND_MANUAL_ATTENTION,
			comment.error_class.as_str(),
			comment.next_action.as_str(),
			comment.failed_command.as_deref().unwrap_or_default(),
			comment.raw_error.as_deref().unwrap_or_default(),
			decision_request_id,
		]);
		let mut record = LinearExecutionEventRecord::new(
			LinearExecutionEventIdentity {
				service_id: &review_context.service_id,
				issue_id: &self.issue.id,
				issue_identifier: &self.issue.identifier,
				run_id: &review_context.run_id,
				attempt_number: review_context.attempt_number,
			},
			"needs_attention",
			tracker_tool_bridge::current_timestamp(),
			&anchor,
		);

		record.branch = Some(review_context.branch_name.clone());
		record.worktree_path = Some(review_context.worktree_path.clone());
		record.pr_url = review_context.recorded_pr_url.clone();
		record.summary = Some(
			comment
				.summary
				.clone()
				.unwrap_or_else(|| format!("Manual attention required: {}.", comment.error_class)),
		);
		record.error_class = Some(comment.error_class.clone());
		record.next_action = Some(comment.next_action.clone());
		record.blockers = Some(comment.blockers.clone());
		record.evidence = Some(comment.evidence.clone());
		record.terminal_path = Some(String::from(MANUAL_ATTENTION_TERMINAL_PATH));
		record.failed_command = comment.failed_command.clone();
		record.raw_error = comment.raw_error.clone();

		record
	}
}

fn normalize_required_comment_field(
	value: Option<String>,
	field_name: &str,
) -> Result<String, String> {
	let value = tracker_tool_bridge::normalize_optional_progress_field(value).ok_or_else(|| {
		format!(
			"`{ISSUE_COMMENT_TOOL_NAME}` kind `{COMMENT_KIND_MANUAL_ATTENTION}` requires `{field_name}`."
		)
	})?;

	Ok(value)
}

fn normalize_required_decision_request_field(
	value: Option<String>,
	field_name: &str,
) -> Result<String, String> {
	tracker_tool_bridge::normalize_optional_progress_field(value)
		.ok_or_else(|| format!("`decision_request.{field_name}` must be present and non-empty."))
}

fn validate_public_decision_request_text(
	decision_request_id: &str,
	proposed_change: &str,
	why_exceeds_authority: &str,
	options: &[NormalizedAuthorityDecisionOption],
	recommendation: &str,
	resume_condition: &str,
) -> Result<(), String> {
	validate_public_decision_request_field(
		"decision_request.decision_request_id",
		decision_request_id,
	)?;
	validate_public_decision_request_field("decision_request.proposed_change", proposed_change)?;
	validate_public_decision_request_field(
		"decision_request.why_exceeds_authority",
		why_exceeds_authority,
	)?;
	validate_public_decision_request_field("decision_request.recommendation", recommendation)?;
	validate_public_decision_request_field("decision_request.resume_condition", resume_condition)?;

	for option in options {
		validate_public_decision_request_field("decision_request.option.label", &option.label)?;
		validate_public_decision_request_field(
			"decision_request.option.description",
			&option.description,
		)?;
	}

	Ok(())
}

fn validate_public_decision_request_field(field_name: &str, value: &str) -> Result<(), String> {
	public_text::validate_public_text_field(field_name, value).map_err(|error| error.to_string())
}

fn validate_public_error_class(error_class: &str) -> Result<(), String> {
	let mut chars = error_class.chars();
	let Some(first) = chars.next() else {
		return Err(String::from("`error_class` must be a public snake_case identifier."));
	};

	if !first.is_ascii_lowercase()
		|| !chars.all(|character| {
			character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_'
		}) {
		return Err(String::from("`error_class` must be a public snake_case identifier."));
	}

	Ok(())
}

fn validate_manual_attention_error_class(error_class: &str) -> Result<(), String> {
	validate_public_error_class(error_class)?;

	if is_runtime_owned_manual_attention_error_class(error_class) {
		return Err(format!(
			"`{ISSUE_COMMENT_TOOL_NAME}` kind `{COMMENT_KIND_MANUAL_ATTENTION}` cannot use runtime-owned error class `{error_class}`; keep repairing, retrying, or letting Decodex retain the lane, and use a human-owned blocker class only when automation cannot clear the blocker."
		));
	}

	Ok(())
}

fn is_runtime_owned_manual_attention_error_class(error_class: &str) -> bool {
	matches!(
		error_class,
		"retryable_execution_failure"
			| "repo_gate_canonicalize_failed"
			| "repo_gate_verify_failed"
			| "repo_gate_baseline_failed"
			| "repo_gate_preexisting_baseline_failed"
			| "repo_gate_global_baseline_failed"
			| "repo_gate_tracked_rewrites_left"
			| "repo_gate_git_lock_contention"
			| "stalled_run_detected"
			| "app_server_zero_evidence_start_failed"
			| "app_server_plugin_list_timeout"
			| "app_server_preflight_timeout"
			| "app_server_transport_disconnected"
			| "phase_goal_terminal_path_missing"
			| "app_server_dynamic_tool_protocol_failure"
			| "app_server_dynamic_tool_failed"
			| "app_server_turn_failed"
			| "app_server_usage_limit_exceeded"
	) || runtime_owned_baseline_error_class(error_class)
}

fn runtime_owned_baseline_error_class(error_class: &str) -> bool {
	[
		"baseline",
		"preexisting",
		"pre_existing",
		"repo_wide",
		"repository_wide",
		"global_baseline",
		"docs_okf",
	]
	.iter()
	.any(|pattern| error_class.contains(pattern))
}

fn format_manual_attention_comment(
	review_context: &ReviewHandoffContext,
	comment: &NormalizedManualAttentionComment,
) -> String {
	let mut lines = vec![
		String::from("decodex run needs manual attention"),
		String::new(),
		format!("- run_id: `{}`", review_context.run_id),
		format!(
			"- run_sequence_attempt: `{}` (not retry-budget count)",
			review_context.attempt_number
		),
		format!("- reported_at: `{}`", tracker_tool_bridge::current_timestamp()),
		format!("- branch: `{}`", review_context.branch_name),
		format!("- worktree_path: `{}`", review_context.worktree_path),
		format!("- comment_kind: `{COMMENT_KIND_MANUAL_ATTENTION}`"),
		format!("- error_class: `{}`", comment.error_class),
		format!("- next_action: {}", comment.next_action),
	];

	if let Some(summary) = comment.summary.as_deref() {
		lines.push(format!("- summary: {summary}"));
	}

	for blocker in &comment.blockers {
		lines.push(format!("- blocker: {blocker}"));
	}
	for evidence in &comment.evidence {
		lines.push(format!("- evidence: {evidence}"));
	}

	if let Some(request) = comment.decision_request.as_ref() {
		lines.push(String::from("- decision_request: authority_boundary"));
		lines.push(format!("- decision_request_id: `{}`", request.decision_request_id));
		lines.push(format!("- decision_reason: `{}`", request.reason_code));
		lines.push(format!("- boundary: `{}`", request.boundary_type));
		lines.push(format!("- proposed_change: {}", request.proposed_change));
		lines.push(format!("- why_exceeds_authority: {}", request.why_exceeds_authority));

		for option in &request.options {
			lines.push(format!("- decision_option: `{}` - {}", option.label, option.description));
		}

		lines.push(format!("- recommendation: {}", request.recommendation));
		lines.push(format!("- resume_condition: {}", request.resume_condition));
	}
	if let Some(failed_command) = comment.failed_command.as_deref() {
		lines.push(format!("- failed_command: {failed_command}"));
	}
	if let Some(raw_error) = comment.raw_error.as_deref() {
		lines.push(format!("- raw_error: {raw_error}"));
	}

	lines.join("\n")
}
