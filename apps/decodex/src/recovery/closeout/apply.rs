use std::path::Path;

use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
	github,
	orchestrator::{
		self, PostReviewLifecycleFactsInput, PullRequestReviewState,
		kernel::{
			lifecycle,
			lifecycle::{
				LifecycleDecisionInput, LifecycleEvidenceKind, LifecycleOutcome,
				PreviousLifecycleAuthority,
			},
		},
	},
	prelude::Result,
	recovery::{
		closeout::{
			LegacyCloseoutValidation, MergedCloseoutValidation, SupersededCloseoutValidation,
			apply::lifecycle::decide_lifecycle_transition, events,
		},
		context::RecoveryContext,
		pull_request_inspection,
	},
	state::StateStore,
	tracker::{
		self, IssueTracker,
		privacy_classifier::{
			ConfiguredPublicProjectionPrivacyClassifier, PublicProjectionPrivacyClassifier,
		},
		records::{self, LinearExecutionEventRecord},
	},
};

pub(super) fn write_legacy_closeout_audit(
	context: &RecoveryContext,
	validation: &LegacyCloseoutValidation,
	event: &LinearExecutionEventRecord,
) -> Result<bool> {
	let audit_body = format!(
		"Decodex legacy manual closeout audit: verified merged PR `{}` for `{}`. Runtime provenance was `{}`, so this records the manual fallback before local cleanup.",
		pull_request_inspection::landing_url(&validation.landing_state),
		validation.issue.identifier,
		validation.worktree.provenance().source()
	);
	let retry_budget_attempt_count =
		context.state_store.retry_budget_attempt_count(&validation.issue.id)?;
	let retry_budget_attempt_count =
		(retry_budget_attempt_count > 0).then_some(retry_budget_attempt_count);
	let body = format!(
		"{audit_body}\n\n{}",
		records::render_linear_execution_event_comment_body(event, retry_budget_attempt_count)
	);
	let privacy_classifier = ConfiguredPublicProjectionPrivacyClassifier::from_config(
		context.config.privacy_classifier(),
	)?;
	let projection =
		tracker::prepare_linear_execution_event_comment(&body, event, &privacy_classifier)?;
	let recorded = context.state_store.record_linear_execution_event(&projection.record)?;

	if !recorded {
		return Ok(false);
	}

	if let Err(error) = tracker::create_linear_execution_event_comment_direct(
		&context.tracker,
		&validation.issue.id,
		&projection,
	) {
		context.state_store.forget_linear_execution_event(&projection.record.idempotency_key)?;

		return Err(error);
	}

	Ok(true)
}

pub(super) fn apply_merged_closeout_recovery(
	context: &RecoveryContext,
	validation: &MergedCloseoutValidation,
) -> Result<(bool, bool)> {
	let mut operations = MergedCloseoutRecoveryOperations { context, validation };

	apply_merged_closeout_recovery_sequence(&mut operations)
}

trait MergedCloseoutRecoveryOperationsRunner {
	fn record_lifecycle_authority(&mut self) -> Result<()>;
	fn write_closeout_event(&mut self) -> Result<bool>;
	fn write_cleanup_event(&mut self) -> Result<bool>;
	fn clear_worktree_if_present(&mut self) -> Result<()>;
	fn update_run_status(&mut self) -> Result<()>;
}

fn apply_merged_closeout_recovery_sequence(
	operations: &mut impl MergedCloseoutRecoveryOperationsRunner,
) -> Result<(bool, bool)> {
	operations.record_lifecycle_authority()?;
	let closeout_recorded = operations.write_closeout_event()?;
	let cleanup_recorded = operations.write_cleanup_event()?;
	operations.clear_worktree_if_present()?;
	operations.update_run_status()?;

	Ok((closeout_recorded, cleanup_recorded))
}

struct MergedCloseoutRecoveryOperations<'a> {
	context: &'a RecoveryContext,
	validation: &'a MergedCloseoutValidation,
}

impl MergedCloseoutRecoveryOperationsRunner for MergedCloseoutRecoveryOperations<'_> {
	fn record_lifecycle_authority(&mut self) -> Result<()> {
		record_merged_closeout_lifecycle_authority(self.context, self.validation)
	}

	fn write_closeout_event(&mut self) -> Result<bool> {
		let closeout_event = events::merged_closeout_event(self.context, self.validation);

		write_merged_closeout_event(
			self.context,
			self.validation,
			&closeout_event,
			"Decodex merged closeout recovery: verified the PR was merged into the current default branch and reconciled the stale retained attention closeout ledger.",
		)
	}

	fn write_cleanup_event(&mut self) -> Result<bool> {
		let cleanup_event = events::merged_closeout_cleanup_event(self.context, self.validation);

		write_merged_closeout_event(
			self.context,
			self.validation,
			&cleanup_event,
			"Decodex merged closeout recovery: verified retained lane cleanup is already complete and recorded cleanup_complete.",
		)
	}

	fn clear_worktree_if_present(&mut self) -> Result<()> {
		if self.validation.worktree_mapping.is_some() {
			self.context.state_store.clear_worktree(&self.validation.issue.id)?;

			if self.validation.issue.identifier != self.validation.issue.id {
				self.context.state_store.clear_worktree(&self.validation.issue.identifier)?;
			}
		}

		Ok(())
	}

	fn update_run_status(&mut self) -> Result<()> {
		self.context.state_store.update_run_status(&self.validation.run_id, "succeeded")
	}
}

pub(super) fn apply_superseded_closeout_recovery(
	context: &RecoveryContext,
	validation: &SupersededCloseoutValidation,
) -> Result<(bool, bool, bool)> {
	let mut operations = SupersededCloseoutRecoveryOperations { context, validation };

	apply_superseded_closeout_recovery_sequence(&mut operations)
}

trait SupersededCloseoutRecoveryOperationsRunner {
	fn ensure_terminalizable(&mut self) -> Result<()>;
	fn ensure_run_attempt_recorded(&mut self) -> Result<()>;
	fn revalidate_obsolete_pull_request(&mut self) -> Result<()>;
	fn confirm_obsolete_pull_request_closed(&mut self) -> Result<()>;
	fn update_issue_state(&mut self) -> Result<()>;
	fn write_closeout_event(&mut self) -> Result<bool>;
	fn write_cleanup_event(&mut self) -> Result<bool>;
	fn record_lifecycle_authority(&mut self, cleanup_state: &'static str) -> Result<()>;
	fn update_run_status(&mut self) -> Result<()>;
	fn clear_worktree(&mut self) -> Result<()>;
	fn post_pull_request_comment(&mut self) -> Result<()>;
	fn close_pull_request_if_open(&mut self) -> Result<bool>;
}

fn apply_superseded_closeout_recovery_sequence(
	operations: &mut impl SupersededCloseoutRecoveryOperationsRunner,
) -> Result<(bool, bool, bool)> {
	operations.ensure_terminalizable()?;
	operations.ensure_run_attempt_recorded()?;
	operations.record_lifecycle_authority("pending")?;
	operations.revalidate_obsolete_pull_request()?;
	let closeout_recorded = operations.write_closeout_event()?;
	operations.revalidate_obsolete_pull_request()?;
	operations.post_pull_request_comment()?;
	operations.revalidate_obsolete_pull_request()?;
	let pr_closed = operations.close_pull_request_if_open()?;
	operations.confirm_obsolete_pull_request_closed()?;
	operations.record_lifecycle_authority("completed")?;
	operations.confirm_obsolete_pull_request_closed()?;
	operations.update_issue_state()?;
	operations.confirm_obsolete_pull_request_closed()?;
	let cleanup_recorded = operations.write_cleanup_event()?;
	operations.update_run_status()?;
	operations.confirm_obsolete_pull_request_closed()?;
	operations.clear_worktree()?;

	Ok((closeout_recorded, cleanup_recorded, pr_closed))
}

struct SupersededCloseoutRecoveryOperations<'a> {
	context: &'a RecoveryContext,
	validation: &'a SupersededCloseoutValidation,
}

impl SupersededCloseoutRecoveryOperationsRunner for SupersededCloseoutRecoveryOperations<'_> {
	fn ensure_terminalizable(&mut self) -> Result<()> {
		super::validation::ensure_superseded_issue_terminalizable(self.context, self.validation)
	}

	fn ensure_run_attempt_recorded(&mut self) -> Result<()> {
		ensure_superseded_closeout_run_attempt(
			&self.context.state_store,
			&self.validation.issue.id,
			&self.validation.run_id,
			self.validation.attempt_number,
		)
	}

	fn revalidate_obsolete_pull_request(&mut self) -> Result<()> {
		let obsolete_pr_url =
			pull_request_inspection::landing_url(&self.validation.obsolete_landing_state);
		let (current, default_branch) =
			pull_request_inspection::inspect_project_pull_request(self.context, obsolete_pr_url)?;

		super::validation::validate_obsolete_pull_request_unchanged(
			&self.validation.obsolete_landing_state,
			&current,
			&default_branch,
		)
	}

	fn confirm_obsolete_pull_request_closed(&mut self) -> Result<()> {
		let obsolete_pr_url =
			pull_request_inspection::landing_url(&self.validation.obsolete_landing_state);
		let (current, default_branch) =
			pull_request_inspection::inspect_project_pull_request(self.context, obsolete_pr_url)?;

		super::validation::validate_obsolete_pull_request_closed(
			&self.validation.obsolete_landing_state,
			&current,
			&default_branch,
		)
	}

	fn update_issue_state(&mut self) -> Result<()> {
		self.context
			.tracker
			.update_issue_state(&self.validation.issue.id, &self.validation.completed_state_id)
	}

	fn write_closeout_event(&mut self) -> Result<bool> {
		let closeout_event = events::superseded_closeout_event(self.context, self.validation);

		write_superseded_closeout_event(
			self.context,
			self.validation,
			&closeout_event,
			"Decodex superseded closeout recovery: verified a successor PR landed the retained repair lineage and authorized closure of the obsolete PR.",
		)
	}

	fn write_cleanup_event(&mut self) -> Result<bool> {
		let cleanup_event =
			events::superseded_closeout_cleanup_event(self.context, self.validation);

		write_superseded_closeout_event(
			self.context,
			self.validation,
			&cleanup_event,
			"Decodex superseded closeout recovery: verified the obsolete retained lane has no remaining unique unlanded work and recorded cleanup_complete.",
		)
	}

	fn record_lifecycle_authority(&mut self, cleanup_state: &'static str) -> Result<()> {
		record_superseded_closeout_lifecycle_authority(self.context, self.validation, cleanup_state)
	}

	fn update_run_status(&mut self) -> Result<()> {
		self.context.state_store.update_run_status(&self.validation.run_id, "succeeded")
	}

	fn clear_worktree(&mut self) -> Result<()> {
		self.context.state_store.clear_worktree(&self.validation.issue.id)?;

		if self.validation.issue.identifier != self.validation.issue.id {
			self.context.state_store.clear_worktree(&self.validation.issue.identifier)?;
		}

		Ok(())
	}

	fn post_pull_request_comment(&mut self) -> Result<()> {
		let obsolete_pr_url =
			pull_request_inspection::landing_url(&self.validation.obsolete_landing_state);
		let successor_pr_url =
			pull_request_inspection::landing_url(&self.validation.successor_landing_state);
		let github_token = self.context.config.github().resolve_token()?;
		let pr_comment = format!(
			"Decodex superseded closeout: closing this retained PR because successor PR {successor_pr_url} for issue {} landed the accepted repair. Original issue {} is terminalized as superseded and should not be landed from this PR.",
			self.validation.successor_issue.identifier, self.validation.issue.identifier
		);

		github::post_pull_request_issue_comment(
			self.context.config.repo_root(),
			obsolete_pr_url,
			&pr_comment,
			&github_token,
			self.context.config.github().command_path(),
		)?;

		Ok(())
	}

	fn close_pull_request_if_open(&mut self) -> Result<bool> {
		if self.validation.obsolete_landing_state.state != "OPEN" {
			return Ok(false);
		}

		let obsolete_pr_url =
			pull_request_inspection::landing_url(&self.validation.obsolete_landing_state);
		let github_token = self.context.config.github().resolve_token()?;

		github::close_pull_request(
			self.context.config.repo_root(),
			obsolete_pr_url,
			&github_token,
			self.context.config.github().command_path(),
		)?;

		Ok(true)
	}
}

fn ensure_superseded_closeout_run_attempt(
	state_store: &StateStore,
	issue_id: &str,
	run_id: &str,
	attempt_number: i64,
) -> Result<()> {
	if !super::validation::ensure_superseded_closeout_run_attempt_compatible(
		state_store,
		issue_id,
		run_id,
		attempt_number,
	)? {
		state_store.record_run_attempt(run_id, issue_id, attempt_number, "terminated")?;
	}

	Ok(())
}

#[cfg(test)]
mod tests {
	use std::cell::RefCell;

	use crate::{
		prelude::{Result, eyre},
		state::StateStore,
		tracker::{
			IssueTracker, TrackerComment, TrackerIssue,
			privacy_classifier::DISABLED_PUBLIC_PROJECTION_PRIVACY_CLASSIFIER,
			records::{self, LinearExecutionEventIdentity, LinearExecutionEventRecord},
		},
	};

	use super::{
		MergedCloseoutRecoveryOperationsRunner, SupersededCloseoutRecoveryOperationsRunner,
		apply_merged_closeout_recovery_sequence, apply_superseded_closeout_recovery_sequence,
		ensure_superseded_closeout_run_attempt, write_recovery_closeout_event,
	};

	#[derive(Default)]
	struct RecordingMergedCloseoutOperations {
		steps: RefCell<Vec<&'static str>>,
		fail_at: Option<&'static str>,
	}
	impl RecordingMergedCloseoutOperations {
		fn record(&self, step: &'static str) -> Result<()> {
			self.steps.borrow_mut().push(step);

			if self.fail_at == Some(step) {
				eyre::bail!("{step} failed");
			}

			Ok(())
		}
	}

	impl MergedCloseoutRecoveryOperationsRunner for RecordingMergedCloseoutOperations {
		fn record_lifecycle_authority(&mut self) -> Result<()> {
			self.record("record_lifecycle_authority")
		}

		fn write_closeout_event(&mut self) -> Result<bool> {
			self.record("write_closeout_event")?;

			Ok(true)
		}

		fn write_cleanup_event(&mut self) -> Result<bool> {
			self.record("write_cleanup_event")?;

			Ok(true)
		}

		fn clear_worktree_if_present(&mut self) -> Result<()> {
			self.record("clear_worktree_if_present")
		}

		fn update_run_status(&mut self) -> Result<()> {
			self.record("update_run_status")
		}
	}

	#[derive(Default)]
	struct RecordingSupersededCloseoutOperations {
		steps: RefCell<Vec<&'static str>>,
		fail_at: Option<&'static str>,
		fail_on_occurrence: Option<(&'static str, usize)>,
	}
	impl RecordingSupersededCloseoutOperations {
		fn record(&self, step: &'static str) -> Result<()> {
			let occurrence = {
				let mut steps = self.steps.borrow_mut();
				steps.push(step);
				steps.iter().filter(|recorded| **recorded == step).count()
			};

			if self.fail_at == Some(step) || self.fail_on_occurrence == Some((step, occurrence)) {
				eyre::bail!("{step} failed");
			}

			Ok(())
		}
	}

	impl SupersededCloseoutRecoveryOperationsRunner for RecordingSupersededCloseoutOperations {
		fn ensure_terminalizable(&mut self) -> Result<()> {
			self.record("ensure_terminalizable")
		}

		fn ensure_run_attempt_recorded(&mut self) -> Result<()> {
			self.record("ensure_run_attempt_recorded")
		}

		fn revalidate_obsolete_pull_request(&mut self) -> Result<()> {
			self.record("revalidate_obsolete_pull_request")
		}

		fn confirm_obsolete_pull_request_closed(&mut self) -> Result<()> {
			self.record("confirm_obsolete_pull_request_closed")
		}

		fn update_issue_state(&mut self) -> Result<()> {
			self.record("update_issue_state")
		}

		fn write_closeout_event(&mut self) -> Result<bool> {
			self.record("write_closeout_event")?;

			Ok(true)
		}

		fn write_cleanup_event(&mut self) -> Result<bool> {
			self.record("write_cleanup_event")?;

			Ok(true)
		}

		fn record_lifecycle_authority(&mut self, cleanup_state: &'static str) -> Result<()> {
			match cleanup_state {
				"pending" => self.record("record_lifecycle_authority_pending"),
				"completed" => self.record("record_lifecycle_authority_completed"),
				_ => unreachable!("unsupported test cleanup state"),
			}
		}

		fn update_run_status(&mut self) -> Result<()> {
			self.record("update_run_status")
		}

		fn clear_worktree(&mut self) -> Result<()> {
			self.record("clear_worktree")
		}

		fn post_pull_request_comment(&mut self) -> Result<()> {
			self.record("post_pull_request_comment")
		}

		fn close_pull_request_if_open(&mut self) -> Result<bool> {
			self.record("close_pull_request_if_open")?;

			Ok(true)
		}
	}

	#[derive(Default)]
	struct ProjectionTracker {
		comments: RefCell<Vec<TrackerComment>>,
		created_comments: RefCell<Vec<String>>,
		list_error: Option<&'static str>,
		create_error: Option<&'static str>,
	}

	impl ProjectionTracker {
		fn with_comments(comments: Vec<TrackerComment>) -> Self {
			Self {
				comments: RefCell::new(comments),
				created_comments: RefCell::new(Vec::new()),
				list_error: None,
				create_error: None,
			}
		}
	}

	impl IssueTracker for ProjectionTracker {
		fn list_issues_with_label(&self, _label_name: &str) -> Result<Vec<TrackerIssue>> {
			Ok(Vec::new())
		}

		fn find_team_label_id(&self, _team_id: &str, _label_name: &str) -> Result<Option<String>> {
			Ok(None)
		}

		fn get_issue_by_identifier(&self, _issue_identifier: &str) -> Result<Option<TrackerIssue>> {
			Ok(None)
		}

		fn refresh_issues(&self, _issue_ids: &[String]) -> Result<Vec<TrackerIssue>> {
			Ok(Vec::new())
		}

		fn list_comments(&self, _issue_id: &str) -> Result<Vec<TrackerComment>> {
			if let Some(error) = self.list_error {
				eyre::bail!(error);
			}

			Ok(self.comments.borrow().clone())
		}

		fn update_issue_state(&self, _issue_id: &str, _state_id: &str) -> Result<()> {
			Ok(())
		}

		fn add_issue_labels(&self, _issue_id: &str, _label_ids: &[String]) -> Result<()> {
			Ok(())
		}

		fn remove_issue_labels(&self, _issue_id: &str, _label_ids: &[String]) -> Result<()> {
			Ok(())
		}

		fn create_comment(&self, _issue_id: &str, body: &str) -> Result<()> {
			if let Some(error) = self.create_error {
				eyre::bail!(error);
			}

			self.created_comments.borrow_mut().push(body.to_owned());
			self.comments.borrow_mut().push(TrackerComment {
				body: body.to_owned(),
				created_at: String::from("2026-07-09T00:00:00Z"),
			});

			Ok(())
		}
	}

	fn closeout_record(anchor: &str) -> LinearExecutionEventRecord {
		let mut record = LinearExecutionEventRecord::new(
			LinearExecutionEventIdentity {
				service_id: "decodex",
				issue_id: "issue-id",
				issue_identifier: "XY-1248",
				run_id: "run-id",
				attempt_number: 1,
			},
			"closeout",
			String::from("2026-07-09T00:00:00Z"),
			anchor,
		);
		record.pr_url = Some(String::from("https://github.com/hack-ink/decodex/pull/1073"));
		record.commit_sha = Some(String::from("0123456789abcdef0123456789abcdef01234567"));
		record.summary = Some(String::from("Closeout recovery projection test record."));

		record
	}

	fn tracker_comment_for_record(record: &LinearExecutionEventRecord) -> TrackerComment {
		let body = records::render_linear_execution_event_comment_body(record, None);
		let body = records::append_structured_comment_record(&body, record)
			.expect("structured comment should render");

		TrackerComment { body, created_at: String::from("2026-07-09T00:00:00Z") }
	}

	#[test]
	fn merged_closeout_records_lifecycle_authority_before_public_projection() {
		let mut operations = RecordingMergedCloseoutOperations::default();

		let result = apply_merged_closeout_recovery_sequence(&mut operations)
			.expect("merged closeout sequence should succeed");

		assert_eq!(result, (true, true));
		assert_eq!(
			operations.steps.into_inner(),
			vec![
				"record_lifecycle_authority",
				"write_closeout_event",
				"write_cleanup_event",
				"clear_worktree_if_present",
				"update_run_status",
			]
		);
	}

	#[test]
	fn merged_closeout_does_not_write_public_projection_when_lifecycle_authority_fails() {
		let mut operations = RecordingMergedCloseoutOperations {
			fail_at: Some("record_lifecycle_authority"),
			..RecordingMergedCloseoutOperations::default()
		};

		let error = apply_merged_closeout_recovery_sequence(&mut operations)
			.expect_err("lifecycle authority failure should stop recovery");

		assert!(error.to_string().contains("record_lifecycle_authority failed"));
		assert_eq!(operations.steps.into_inner(), vec!["record_lifecycle_authority"]);
	}

	#[test]
	fn merged_closeout_does_not_clear_or_succeed_run_when_projection_fails() {
		let mut operations = RecordingMergedCloseoutOperations {
			fail_at: Some("write_closeout_event"),
			..RecordingMergedCloseoutOperations::default()
		};

		let error = apply_merged_closeout_recovery_sequence(&mut operations)
			.expect_err("public projection failure should stop recovery");

		assert!(error.to_string().contains("write_closeout_event failed"));
		assert_eq!(
			operations.steps.into_inner(),
			vec!["record_lifecycle_authority", "write_closeout_event",]
		);
	}

	#[test]
	fn closeout_projection_retries_remote_comment_when_local_record_is_duplicate() {
		let state_store = StateStore::open_in_memory().expect("state store should open");
		let tracker = ProjectionTracker::default();
		let event = closeout_record("duplicate-local-missing-remote");
		state_store.record_linear_execution_event(&event).expect("local event should record");

		let written = write_recovery_closeout_event(
			&tracker,
			&state_store,
			"issue-id",
			&event,
			"Recovery closeout projection.",
			None,
			&DISABLED_PUBLIC_PROJECTION_PRIVACY_CLASSIFIER,
		)
		.expect("duplicate local record should still post missing remote projection");

		assert!(written);
		assert_eq!(tracker.created_comments.borrow().len(), 1);
	}

	#[test]
	fn closeout_projection_skips_duplicate_only_when_remote_comment_exists() {
		let state_store = StateStore::open_in_memory().expect("state store should open");
		let event = closeout_record("duplicate-local-and-remote");
		state_store.record_linear_execution_event(&event).expect("local event should record");
		let tracker = ProjectionTracker::with_comments(vec![tracker_comment_for_record(&event)]);

		let written = write_recovery_closeout_event(
			&tracker,
			&state_store,
			"issue-id",
			&event,
			"Recovery closeout projection.",
			None,
			&DISABLED_PUBLIC_PROJECTION_PRIVACY_CLASSIFIER,
		)
		.expect("remote duplicate should be accepted");

		assert!(!written);
		assert!(tracker.created_comments.borrow().is_empty());
	}

	#[test]
	fn superseded_closeout_records_durable_state_before_github_side_effects() {
		let mut operations = RecordingSupersededCloseoutOperations::default();

		let result = apply_superseded_closeout_recovery_sequence(&mut operations)
			.expect("superseded closeout sequence should succeed");

		assert_eq!(result, (true, true, true));
		assert_eq!(
			operations.steps.into_inner(),
			vec![
				"ensure_terminalizable",
				"ensure_run_attempt_recorded",
				"record_lifecycle_authority_pending",
				"revalidate_obsolete_pull_request",
				"write_closeout_event",
				"revalidate_obsolete_pull_request",
				"post_pull_request_comment",
				"revalidate_obsolete_pull_request",
				"close_pull_request_if_open",
				"confirm_obsolete_pull_request_closed",
				"record_lifecycle_authority_completed",
				"confirm_obsolete_pull_request_closed",
				"update_issue_state",
				"confirm_obsolete_pull_request_closed",
				"write_cleanup_event",
				"update_run_status",
				"confirm_obsolete_pull_request_closed",
				"clear_worktree",
			]
		);
	}

	#[test]
	fn superseded_closeout_confirms_closed_pr_before_issue_terminalization() {
		let mut operations = RecordingSupersededCloseoutOperations {
			fail_on_occurrence: Some(("confirm_obsolete_pull_request_closed", 2)),
			..RecordingSupersededCloseoutOperations::default()
		};

		let error = apply_superseded_closeout_recovery_sequence(&mut operations)
			.expect_err("reopened PR should stop issue terminalization");

		assert!(error.to_string().contains("confirm_obsolete_pull_request_closed failed"));
		assert!(!operations.steps.borrow().contains(&"update_issue_state"));
		assert!(!operations.steps.borrow().contains(&"write_cleanup_event"));
	}

	#[test]
	fn superseded_closeout_confirms_closed_pr_before_cleanup_projection() {
		let mut operations = RecordingSupersededCloseoutOperations {
			fail_on_occurrence: Some(("confirm_obsolete_pull_request_closed", 3)),
			..RecordingSupersededCloseoutOperations::default()
		};

		let error = apply_superseded_closeout_recovery_sequence(&mut operations)
			.expect_err("reopened PR should stop cleanup projection");

		assert!(error.to_string().contains("confirm_obsolete_pull_request_closed failed"));
		assert!(operations.steps.borrow().contains(&"update_issue_state"));
		assert!(!operations.steps.borrow().contains(&"write_cleanup_event"));
		assert!(!operations.steps.borrow().contains(&"update_run_status"));
		assert!(!operations.steps.borrow().contains(&"clear_worktree"));
	}

	#[test]
	fn superseded_closeout_confirms_closed_pr_before_worktree_clear() {
		let mut operations = RecordingSupersededCloseoutOperations {
			fail_on_occurrence: Some(("confirm_obsolete_pull_request_closed", 4)),
			..RecordingSupersededCloseoutOperations::default()
		};

		let error = apply_superseded_closeout_recovery_sequence(&mut operations)
			.expect_err("reopened PR should preserve retained worktree");

		assert!(error.to_string().contains("confirm_obsolete_pull_request_closed failed"));
		assert!(operations.steps.borrow().contains(&"write_cleanup_event"));
		assert!(operations.steps.borrow().contains(&"update_run_status"));
		assert!(!operations.steps.borrow().contains(&"clear_worktree"));
	}

	#[test]
	fn superseded_closeout_revalidates_before_public_projection() {
		let mut operations = RecordingSupersededCloseoutOperations {
			fail_on_occurrence: Some(("revalidate_obsolete_pull_request", 1)),
			..RecordingSupersededCloseoutOperations::default()
		};

		let error = apply_superseded_closeout_recovery_sequence(&mut operations)
			.expect_err("changed PR should stop before public projection");

		assert!(error.to_string().contains("revalidate_obsolete_pull_request failed"));
		assert!(!operations.steps.borrow().contains(&"write_closeout_event"));
		assert!(!operations.steps.borrow().contains(&"post_pull_request_comment"));
	}

	#[test]
	fn superseded_closeout_revalidates_again_before_pr_close() {
		let mut operations = RecordingSupersededCloseoutOperations {
			fail_on_occurrence: Some(("revalidate_obsolete_pull_request", 3)),
			..RecordingSupersededCloseoutOperations::default()
		};

		let error = apply_superseded_closeout_recovery_sequence(&mut operations)
			.expect_err("changed PR after comment should stop before close");

		assert!(error.to_string().contains("revalidate_obsolete_pull_request failed"));
		assert!(operations.steps.borrow().contains(&"post_pull_request_comment"));
		assert!(!operations.steps.borrow().contains(&"close_pull_request_if_open"));
		assert!(!operations.steps.borrow().contains(&"record_lifecycle_authority_completed"));
	}

	#[test]
	fn superseded_closeout_does_not_terminalize_issue_when_lifecycle_authority_fails() {
		let mut operations = RecordingSupersededCloseoutOperations {
			fail_at: Some("record_lifecycle_authority_pending"),
			..RecordingSupersededCloseoutOperations::default()
		};

		let error = apply_superseded_closeout_recovery_sequence(&mut operations)
			.expect_err("lifecycle authority failure should stop recovery");

		assert!(error.to_string().contains("record_lifecycle_authority_pending failed"));
		assert_eq!(
			operations.steps.into_inner(),
			vec![
				"ensure_terminalizable",
				"ensure_run_attempt_recorded",
				"record_lifecycle_authority_pending",
			]
		);
	}

	#[test]
	fn superseded_closeout_does_not_write_public_closeout_when_lifecycle_authority_fails() {
		let mut operations = RecordingSupersededCloseoutOperations {
			fail_at: Some("record_lifecycle_authority_pending"),
			..RecordingSupersededCloseoutOperations::default()
		};

		let error = apply_superseded_closeout_recovery_sequence(&mut operations)
			.expect_err("lifecycle authority failure should stop recovery");

		assert!(error.to_string().contains("record_lifecycle_authority_pending failed"));
		assert!(!operations.steps.borrow().contains(&"write_closeout_event"));
	}

	#[test]
	fn superseded_closeout_does_not_write_public_cleanup_when_lifecycle_authority_fails() {
		let mut operations = RecordingSupersededCloseoutOperations {
			fail_at: Some("record_lifecycle_authority_completed"),
			..RecordingSupersededCloseoutOperations::default()
		};

		let error = apply_superseded_closeout_recovery_sequence(&mut operations)
			.expect_err("completed lifecycle authority failure should stop recovery");

		assert!(error.to_string().contains("record_lifecycle_authority_completed failed"));
		assert!(!operations.steps.borrow().contains(&"write_cleanup_event"));
	}

	#[test]
	fn superseded_closeout_keeps_cleanup_retryable_when_github_comment_fails() {
		let mut operations = RecordingSupersededCloseoutOperations {
			fail_at: Some("post_pull_request_comment"),
			..RecordingSupersededCloseoutOperations::default()
		};

		let error = apply_superseded_closeout_recovery_sequence(&mut operations)
			.expect_err("GitHub comment failure should stop cleanup");

		assert!(error.to_string().contains("post_pull_request_comment failed"));
		assert_eq!(
			operations.steps.into_inner(),
			vec![
				"ensure_terminalizable",
				"ensure_run_attempt_recorded",
				"record_lifecycle_authority_pending",
				"revalidate_obsolete_pull_request",
				"write_closeout_event",
				"revalidate_obsolete_pull_request",
				"post_pull_request_comment",
			]
		);
	}

	#[test]
	fn superseded_closeout_records_completed_authority_before_terminal_issue_state() {
		let mut operations = RecordingSupersededCloseoutOperations {
			fail_at: Some("record_lifecycle_authority_completed"),
			..RecordingSupersededCloseoutOperations::default()
		};

		let error = apply_superseded_closeout_recovery_sequence(&mut operations)
			.expect_err("completed lifecycle authority failure should stop terminalization");

		assert!(error.to_string().contains("record_lifecycle_authority_completed failed"));
		assert!(!operations.steps.borrow().contains(&"update_issue_state"));
	}

	#[test]
	fn superseded_closeout_records_missing_recovery_run_attempt() {
		let state_store = StateStore::open_in_memory().expect("state store should open");

		ensure_superseded_closeout_run_attempt(
			&state_store,
			"issue-id",
			"superseded-closeout-xy-1248",
			1,
		)
		.expect("missing recovery run attempt should be recorded");

		let attempt = state_store
			.run_attempt("superseded-closeout-xy-1248")
			.expect("run attempt read should succeed")
			.expect("run attempt should exist");
		assert_eq!(attempt.issue_id(), "issue-id");
		assert_eq!(attempt.attempt_number(), 1);
		assert_eq!(attempt.status(), "terminated");
	}

	#[test]
	fn superseded_closeout_rejects_conflicting_recovery_run_attempt() {
		let state_store = StateStore::open_in_memory().expect("state store should open");
		state_store
			.record_run_attempt("superseded-closeout-xy-1248", "other-issue", 2, "running")
			.expect("conflicting run attempt should record");

		let error = ensure_superseded_closeout_run_attempt(
			&state_store,
			"issue-id",
			"superseded-closeout-xy-1248",
			1,
		)
		.expect_err("conflicting run attempt should fail closed");

		assert!(error.to_string().contains("conflicts with issue"));
		assert!(error.to_string().contains("other-issue"));
	}
}

fn write_merged_closeout_event(
	context: &RecoveryContext,
	validation: &MergedCloseoutValidation,
	event: &LinearExecutionEventRecord,
	body: &str,
) -> Result<bool> {
	let privacy_classifier = ConfiguredPublicProjectionPrivacyClassifier::from_config(
		context.config.privacy_classifier(),
	)?;
	let retry_budget_attempt_count =
		context.state_store.retry_budget_attempt_count(&validation.issue.id)?;
	let retry_budget_attempt_count =
		(retry_budget_attempt_count > 0).then_some(retry_budget_attempt_count);

	write_recovery_closeout_event(
		&context.tracker,
		&context.state_store,
		&validation.issue.id,
		event,
		body,
		retry_budget_attempt_count,
		&privacy_classifier,
	)
}

fn write_superseded_closeout_event(
	context: &RecoveryContext,
	validation: &SupersededCloseoutValidation,
	event: &LinearExecutionEventRecord,
	body: &str,
) -> Result<bool> {
	let privacy_classifier = ConfiguredPublicProjectionPrivacyClassifier::from_config(
		context.config.privacy_classifier(),
	)?;
	let retry_budget_attempt_count =
		context.state_store.retry_budget_attempt_count(&validation.issue.id)?;
	let retry_budget_attempt_count =
		(retry_budget_attempt_count > 0).then_some(retry_budget_attempt_count);
	write_recovery_closeout_event(
		&context.tracker,
		&context.state_store,
		&validation.issue.id,
		event,
		body,
		retry_budget_attempt_count,
		&privacy_classifier,
	)
}

fn write_recovery_closeout_event<T>(
	tracker: &T,
	state_store: &StateStore,
	issue_id: &str,
	event: &LinearExecutionEventRecord,
	body: &str,
	retry_budget_attempt_count: Option<i64>,
	privacy_classifier: &dyn PublicProjectionPrivacyClassifier,
) -> Result<bool>
where
	T: IssueTracker + ?Sized,
{
	let body = format!(
		"{body}\n\n{}",
		records::render_linear_execution_event_comment_body(event, retry_budget_attempt_count)
	);
	let projection =
		tracker::prepare_linear_execution_event_comment(&body, event, privacy_classifier)?;
	let recorded = state_store.record_linear_execution_event(&projection.record)?;

	match tracker::create_prepared_linear_execution_event_comment(tracker, issue_id, &projection) {
		Ok(comment_created) => Ok(recorded || comment_created),
		Err(error) => {
			if recorded {
				state_store.forget_linear_execution_event(&projection.record.idempotency_key)?;
			}

			Err(error)
		},
	}
}

fn record_merged_closeout_lifecycle_authority(
	context: &RecoveryContext,
	validation: &MergedCloseoutValidation,
) -> Result<()> {
	record_merged_closeout_lifecycle_decision(
		context,
		validation,
		LifecycleEvidenceKind::LandingReadback,
		LifecycleOutcome::Succeeded,
		"landed",
		"not_started",
		"not_started",
		"merged_closeout_recovery_landed_readback",
	)?;

	record_merged_closeout_lifecycle_decision(
		context,
		validation,
		LifecycleEvidenceKind::CloseoutCompletion,
		LifecycleOutcome::Succeeded,
		"landed",
		"completed",
		"completed",
		"merged_closeout_recovery_closeout_complete",
	)
}

#[allow(clippy::too_many_arguments)]
fn record_merged_closeout_lifecycle_decision(
	context: &RecoveryContext,
	validation: &MergedCloseoutValidation,
	evidence_kind: LifecycleEvidenceKind,
	outcome: LifecycleOutcome,
	landing_state: &str,
	closeout_state: &str,
	cleanup_state: &str,
	causation_id: &str,
) -> Result<()> {
	let review_level = context.config.codex().review_level();
	let checkpoint = orchestrator::runtime_review_checkpoint_status_for_head(
		&context.state_store,
		context.config.service_id(),
		&validation.issue.id,
		review_level,
		&validation.landing_state.head_ref_oid,
	)?;
	let review_state = merged_closeout_review_state(validation);
	let facts = orchestrator::build_post_review_lifecycle_facts(PostReviewLifecycleFactsInput {
		project_id: context.config.service_id(),
		issue_id: &validation.issue.id,
		review_lifecycle: None,
		review_state: &review_state,
		worktree_path: Path::new(&validation.worktree_path_for_event),
		review_level,
		phase: "merged_closeout_recovery",
		landing_state: Some(landing_state),
		closeout_state: Some(closeout_state),
		validated_head_sha: Some(&validation.landing_state.head_ref_oid),
		review_checkpoint_phase: checkpoint.as_ref().map(|checkpoint| checkpoint.phase),
		review_checkpoint_status: checkpoint.as_ref().map(|checkpoint| checkpoint.status.as_str()),
	});
	let previous_record = context.state_store.review_lifecycle_record(
		context.config.service_id(),
		&validation.issue.id,
		&validation.branch_name,
	)?;
	let previous = previous_record.as_ref().map(|record| PreviousLifecycleAuthority {
		sequence: record.sequence(),
		next_state: record.next_state(),
	});
	let idempotency_key = format!(
		"{}:{}:{}:{}:{}",
		context.config.service_id(),
		validation.issue.id,
		validation.landing_state.head_ref_oid,
		evidence_kind.as_str(),
		causation_id
	);
	let decided_at = current_timestamp();
	let decision = self::decide_lifecycle_transition(LifecycleDecisionInput {
		facts: &facts,
		previous,
		evidence_kind,
		outcome,
		merge_commit: Some(&validation.merge_commit),
		cleanup_state: Some(cleanup_state),
		authority: "issue_authority",
		actor: "merged_closeout_recovery",
		idempotency_key: &idempotency_key,
		correlation_id: &validation.run_id,
		causation_id: Some(causation_id),
		decided_at: &decided_at,
	});

	context.state_store.record_lifecycle_decision(
		&validation.run_id,
		validation.attempt_number,
		&decision,
	)?;

	Ok(())
}

fn record_superseded_closeout_lifecycle_authority(
	context: &RecoveryContext,
	validation: &SupersededCloseoutValidation,
	cleanup_state: &'static str,
) -> Result<()> {
	let review_level = context.config.codex().review_level();
	let checkpoint = orchestrator::runtime_review_checkpoint_status_for_head(
		&context.state_store,
		context.config.service_id(),
		&validation.issue.id,
		review_level,
		&validation.obsolete_landing_state.head_ref_oid,
	)?;
	let review_state = superseded_closeout_review_state(validation);
	let facts = orchestrator::build_post_review_lifecycle_facts(PostReviewLifecycleFactsInput {
		project_id: context.config.service_id(),
		issue_id: &validation.issue.id,
		review_lifecycle: None,
		review_state: &review_state,
		worktree_path: Path::new(&validation.worktree_path_for_event),
		review_level,
		phase: "superseded_closeout_recovery",
		landing_state: Some("superseded"),
		closeout_state: Some(superseded_closeout_fact_closeout_state(cleanup_state)),
		validated_head_sha: Some(&validation.obsolete_landing_state.head_ref_oid),
		review_checkpoint_phase: checkpoint.as_ref().map(|checkpoint| checkpoint.phase),
		review_checkpoint_status: checkpoint.as_ref().map(|checkpoint| checkpoint.status.as_str()),
	});
	let previous_record = context.state_store.review_lifecycle_record(
		context.config.service_id(),
		&validation.issue.id,
		&validation.branch_name,
	)?;
	let previous = previous_record.as_ref().map(|record| PreviousLifecycleAuthority {
		sequence: record.sequence(),
		next_state: record.next_state(),
	});
	let idempotency_key = format!(
		"{}:{}:{}:{}:{}",
		context.config.service_id(),
		validation.issue.id,
		validation.successor_merge_commit,
		"superseded_closeout_recovery",
		cleanup_state
	);
	let causation_id = match cleanup_state {
		"pending" => "superseded_closeout_recovery_pr_close_authorized",
		"completed" => "superseded_closeout_recovery_closeout_complete",
		_ => "superseded_closeout_recovery_closeout_state",
	};
	let (evidence_kind, outcome) = superseded_closeout_lifecycle_evidence(cleanup_state);
	let decided_at = current_timestamp();
	let decision = self::decide_lifecycle_transition(LifecycleDecisionInput {
		facts: &facts,
		previous,
		evidence_kind,
		outcome,
		merge_commit: Some(&validation.successor_merge_commit),
		cleanup_state: Some(cleanup_state),
		authority: "issue_authority",
		actor: "superseded_closeout_recovery",
		idempotency_key: &idempotency_key,
		correlation_id: &validation.run_id,
		causation_id: Some(causation_id),
		decided_at: &decided_at,
	});

	context.state_store.record_lifecycle_decision(
		&validation.run_id,
		validation.attempt_number,
		&decision,
	)?;

	Ok(())
}

fn superseded_closeout_fact_closeout_state(cleanup_state: &str) -> &'static str {
	match cleanup_state {
		"completed" => "completed",
		_ => "not_started",
	}
}

fn superseded_closeout_lifecycle_evidence(
	cleanup_state: &str,
) -> (LifecycleEvidenceKind, LifecycleOutcome) {
	match cleanup_state {
		"completed" => (LifecycleEvidenceKind::CloseoutCompletion, LifecycleOutcome::Succeeded),
		_ => (LifecycleEvidenceKind::CloseoutIntent, LifecycleOutcome::Intent),
	}
}

fn superseded_closeout_review_state(
	validation: &SupersededCloseoutValidation,
) -> PullRequestReviewState {
	PullRequestReviewState {
		url: pull_request_inspection::landing_url(&validation.obsolete_landing_state).to_owned(),
		state: validation.obsolete_landing_state.state.clone(),
		is_draft: validation.obsolete_landing_state.is_draft,
		review_decision: validation.obsolete_landing_state.review_decision.clone(),
		merge_commit_allowed: false,
		pending_review_requests: validation.obsolete_landing_state.pending_review_requests,
		mergeable: validation.obsolete_landing_state.mergeable.clone(),
		merge_state_status: validation.obsolete_landing_state.merge_state_status.clone(),
		base_ref_oid: validation.obsolete_landing_state.base_ref_oid.clone(),
		head_ref_name: validation.obsolete_landing_state.head_ref_name.clone(),
		head_ref_oid: validation.obsolete_landing_state.head_ref_oid.clone(),
		merge_commit_oid: Some(validation.successor_merge_commit.clone()),
		head_repository_name: None,
		head_repository_owner: None,
		status_check_rollup_state: validation
			.obsolete_landing_state
			.status_check_rollup_state
			.clone(),
		required_status_contexts: validation
			.obsolete_landing_state
			.required_status_contexts
			.clone(),
		unresolved_review_threads: validation.obsolete_landing_state.unresolved_review_threads,
		issue_description_external_review_thumbs_up_count: 0,
		issue_comments: Vec::new(),
		reviews: Vec::new(),
	}
}

fn merged_closeout_review_state(validation: &MergedCloseoutValidation) -> PullRequestReviewState {
	PullRequestReviewState {
		url: pull_request_inspection::landing_url(&validation.landing_state).to_owned(),
		state: validation.landing_state.state.clone(),
		is_draft: validation.landing_state.is_draft,
		review_decision: validation.landing_state.review_decision.clone(),
		merge_commit_allowed: false,
		pending_review_requests: validation.landing_state.pending_review_requests,
		mergeable: validation.landing_state.mergeable.clone(),
		merge_state_status: validation.landing_state.merge_state_status.clone(),
		base_ref_oid: validation.landing_state.base_ref_oid.clone(),
		head_ref_name: validation.landing_state.head_ref_name.clone(),
		head_ref_oid: validation.landing_state.head_ref_oid.clone(),
		merge_commit_oid: Some(validation.merge_commit.clone()),
		head_repository_name: None,
		head_repository_owner: None,
		status_check_rollup_state: validation.landing_state.status_check_rollup_state.clone(),
		required_status_contexts: validation.landing_state.required_status_contexts.clone(),
		unresolved_review_threads: validation.landing_state.unresolved_review_threads,
		issue_description_external_review_thumbs_up_count: 0,
		issue_comments: Vec::new(),
		reviews: Vec::new(),
	}
}

fn current_timestamp() -> String {
	OffsetDateTime::now_utc().format(&Rfc3339).expect("timestamp formatting should succeed")
}
