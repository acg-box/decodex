mod comment_body;
mod normalize;

use serde_json::{self, Value};

use crate::{
	agent::tracker_tool_bridge::{
		self, CommentArgs, DynamicToolCallResponse, ISSUE_COMMENT_TOOL_NAME,
		ISSUE_LABEL_ADD_TOOL_NAME, ReviewHandoffContext, TrackerToolBridge,
		tools::{COMMENT_KIND_MANUAL_ATTENTION, MANUAL_ATTENTION_TERMINAL_PATH},
	},
	orchestrator::{
		self, AUTHORITY_BOUNDARY_CHECK_EVENT_TYPE, AuthorityDecisionOption,
		AuthorityDecisionRequestInput,
	},
	state::StateStore,
	tracker::{
		self,
		records::{self, LinearExecutionEventIdentity, LinearExecutionEventRecord},
	},
};

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
		let comment = match normalize::normalize_manual_attention_comment(parsed) {
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
		let body = comment_body::format_manual_attention_comment(review_context, &comment);
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
