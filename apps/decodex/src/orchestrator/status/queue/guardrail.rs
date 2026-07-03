use crate::{
	orchestrator::{
		kernel::command::{CommandFact, CommandIntent, CommandIntentKind},
		status::{
			self, LOOP_GUARDRAIL_CONVERGENCE_BUDGET, LoopGuardrailCheckpoint,
			LoopGuardrailCheckpointInput, LoopGuardrailReason, ServiceConfig, StateStore,
			TrackerIssue, WorkflowDocument,
			queue::{
				candidates::queued_issue_blocker_identifiers,
				models::{QueuedGuardrailCommand, QueuedGuardrailCommandAction},
			},
		},
	},
	prelude::Result,
};

pub(crate) fn observe_dependency_program_stale_guardrail(
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue: &TrackerIssue,
) -> Result<LoopGuardrailCheckpoint> {
	let blocker_fingerprint = dependency_blocker_fingerprint(issue, workflow);
	let checkpoint =
		state_store.observe_loop_guardrail_checkpoint(LoopGuardrailCheckpointInput {
			project_id: project.service_id(),
			issue_id: &issue.id,
			reason: LoopGuardrailReason::DependencyProgramStale.error_class(),
			fingerprint: &blocker_fingerprint,
			run_id: "queued-dependency-blocker",
			attempt_number: 0,
			details_json: &status::json!({
				"schema": "decodex.loop_guardrail_checkpoint/1",
				"reason": LoopGuardrailReason::DependencyProgramStale.error_class(),
				"blockers": queued_issue_blocker_identifiers(
					issue,
					workflow,
					"open_tracker_blockers",
				),
				"threshold": LOOP_GUARDRAIL_CONVERGENCE_BUDGET,
			})
			.to_string(),
		})?;

	Ok(checkpoint)
}

pub(crate) fn dependency_blocker_fingerprint(
	issue: &TrackerIssue,
	workflow: &WorkflowDocument,
) -> String {
	let mut blockers = issue
		.blockers
		.iter()
		.filter(|blocker| !status::state_name_is_terminal(&blocker.state.name, workflow))
		.map(|blocker| format!("{}:{}", blocker.identifier, blocker.state.name))
		.collect::<Vec<_>>();

	blockers.sort();

	status::loop_guardrail_text_hash(&blockers.join("|"))
}

pub(crate) fn apply_queued_candidate_guardrail_commands(
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	commands: &[QueuedGuardrailCommand],
) -> Result<()> {
	for command in commands {
		match (command.action, command.intent.kind) {
			(
				QueuedGuardrailCommandAction::ObserveDependencyProgramStale,
				CommandIntentKind::ObserveLoopGuardrailCheckpoint,
			) => {
				observe_dependency_program_stale_guardrail(
					project,
					workflow,
					state_store,
					&command.issue,
				)?;
			},
			(
				QueuedGuardrailCommandAction::ClearDependencyProgramStale,
				CommandIntentKind::ClearLoopGuardrailCheckpoint,
			) => {
				state_store.clear_loop_guardrail_checkpoint(
					project.service_id(),
					&command.issue.id,
					LoopGuardrailReason::DependencyProgramStale.error_class(),
				)?;
			},
			_ => {
				color_eyre::eyre::bail!(
					"queued guardrail command action `{}` does not match intent `{}`",
					match command.action {
						QueuedGuardrailCommandAction::ObserveDependencyProgramStale =>
							"observe_dependency_program_stale",
						QueuedGuardrailCommandAction::ClearDependencyProgramStale =>
							"clear_dependency_program_stale",
					},
					command.intent.kind.as_str()
				);
			},
		}
	}

	Ok(())
}

pub(super) fn current_dependency_program_stale_count(
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue: &TrackerIssue,
) -> Result<i64> {
	let blocker_fingerprint = dependency_blocker_fingerprint(issue, workflow);

	Ok(state_store
		.loop_guardrail_checkpoint(
			project.service_id(),
			&issue.id,
			LoopGuardrailReason::DependencyProgramStale.error_class(),
		)?
		.filter(|checkpoint| checkpoint.fingerprint() == blocker_fingerprint)
		.map_or(0, |checkpoint| checkpoint.consecutive_count()))
}

pub(super) fn dependency_program_stale_checkpoint_exists(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue: &TrackerIssue,
) -> Result<bool> {
	Ok(state_store
		.loop_guardrail_checkpoint(
			project.service_id(),
			&issue.id,
			LoopGuardrailReason::DependencyProgramStale.error_class(),
		)?
		.is_some())
}

pub(super) fn observe_dependency_program_stale_guardrail_command(
	issue: &TrackerIssue,
) -> QueuedGuardrailCommand {
	QueuedGuardrailCommand {
		intent: CommandIntent::new(
			CommandIntentKind::ObserveLoopGuardrailCheckpoint,
			format!("{}:dependency_program_stale:observe", issue.id),
			vec![CommandFact::OpenTrackerBlockersPresent],
			vec![CommandFact::LoopGuardrailCheckpointObserved],
		),
		action: QueuedGuardrailCommandAction::ObserveDependencyProgramStale,
		issue: issue.clone(),
	}
}

pub(super) fn clear_dependency_program_stale_guardrail_command(
	issue: &TrackerIssue,
) -> QueuedGuardrailCommand {
	QueuedGuardrailCommand {
		intent: CommandIntent::new(
			CommandIntentKind::ClearLoopGuardrailCheckpoint,
			format!("{}:dependency_program_stale:clear", issue.id),
			vec![CommandFact::OpenTrackerBlockersResolved],
			vec![CommandFact::LoopGuardrailCheckpointCleared],
		),
		action: QueuedGuardrailCommandAction::ClearDependencyProgramStale,
		issue: issue.clone(),
	}
}
