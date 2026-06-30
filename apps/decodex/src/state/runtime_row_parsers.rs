use std::{cmp, path::PathBuf};

use rusqlite::{self, Row, types::Type};
use serde::de::DeserializeOwned;
use serde_json::Value;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
	autonomy_objective::AutonomyObjectiveContract,
	autonomy_proposal::AutonomyProposal,
	autonomy_signal::AutonomySignal,
	execution_program::ExecutionProgram,
	loop_contract::DecisionContract,
	prelude::{Result, eyre},
	tracker::records::LinearExecutionEventRecord,
};

use super::{
	AutonomyObjectiveRuntimeRecord, AutonomyObjectiveRuntimeRowParts,
	AutonomyProposalRuntimeRecord, AutonomyProposalRuntimeRowParts, AutonomySignalRuntimeRecord,
	AutonomySignalRuntimeRowParts, ChildAgentActivitySummary, ConnectorBackoff,
	DecisionContractRuntimeRecord, DecisionContractRuntimeRowParts, ExecutionProgramRuntimeRecord,
	ExecutionProgramRuntimeRowParts, LinearExecutionEventRuntimeRecord,
	PrivateExecutionEventRuntimeRecord, ProgramIntakePlanRecord, ProgramIssueMappingRecord,
	ProtocolEventRecord, ProtocolEventSummaryRecord, RunActivitySummaryRecord, RunAttemptRecord,
	TimestampParts, WorktreeMappingRecord,
};

pub(super) fn timestamp_parts() -> TimestampParts {
	let now = OffsetDateTime::now_utc();

	TimestampParts {
		text: now.format(&Rfc3339).expect("timestamp formatting should succeed"),
		unix: now.unix_timestamp(),
	}
}

pub(super) fn parse_linear_execution_event_unix(
	record: &LinearExecutionEventRecord,
) -> Option<i64> {
	OffsetDateTime::parse(&record.event_timestamp, &Rfc3339)
		.ok()
		.map(|timestamp| timestamp.unix_timestamp())
}

pub(super) fn validate_private_execution_event_inputs(
	project_id: &str,
	issue_id: &str,
	run_id: &str,
	attempt_number: i64,
	event_type: &str,
) -> Result<()> {
	if project_id.trim().is_empty() {
		eyre::bail!("Private execution event project_id must not be empty.");
	}
	if issue_id.trim().is_empty() {
		eyre::bail!("Private execution event issue_id must not be empty.");
	}
	if run_id.trim().is_empty() {
		eyre::bail!("Private execution event run_id must not be empty.");
	}
	if attempt_number < 1 {
		eyre::bail!("Private execution event attempt_number must be greater than zero.");
	}
	if event_type.trim().is_empty() {
		eyre::bail!("Private execution event event_type must not be empty.");
	}

	Ok(())
}

pub(super) fn protocol_event_summary_from_events(
	events: &[ProtocolEventRecord],
) -> ProtocolEventSummaryRecord {
	let mut summary = ProtocolEventSummaryRecord::default();

	for event in events {
		summary.record_event(event);
	}

	summary
}

pub(super) fn protocol_event_record_from_row(
	row: &Row<'_>,
) -> std::result::Result<ProtocolEventRecord, rusqlite::Error> {
	Ok(ProtocolEventRecord {
		sequence_number: row.get(0)?,
		event_type: row.get(1)?,
		payload_sha256: row.get(2)?,
		created_at: row.get(3)?,
		created_at_unix: row.get(4)?,
	})
}

pub(super) fn compare_attempt_records(
	left: &RunAttemptRecord,
	right: &RunAttemptRecord,
) -> cmp::Ordering {
	left.attempt_number
		.cmp(&right.attempt_number)
		.then_with(|| left.updated_at_unix.cmp(&right.updated_at_unix))
		.then_with(|| left.run_id.cmp(&right.run_id))
}

pub(super) fn run_attempt_record_from_row(
	row: &Row<'_>,
) -> std::result::Result<RunAttemptRecord, rusqlite::Error> {
	Ok(RunAttemptRecord {
		run_id: row.get(0)?,
		project_id: row.get(1)?,
		issue_id: row.get(2)?,
		attempt_number: row.get(3)?,
		status: row.get(4)?,
		thread_id: row.get(5)?,
		turn_id: row.get(6)?,
		updated_at: row.get(7)?,
		updated_at_unix: row.get(8)?,
	})
}

pub(super) fn run_activity_summary_record_from_row(
	row: &Row<'_>,
) -> std::result::Result<RunActivitySummaryRecord, rusqlite::Error> {
	Ok(RunActivitySummaryRecord {
		run_id: row.get(0)?,
		attempt_number: row.get(1)?,
		child_agent_activity: optional_json_from_row::<ChildAgentActivitySummary>(row, 2)?
			.map(ChildAgentActivitySummary::sealed_durable),
		protocol_activity: optional_json_from_row(row, 3)?,
		updated_at: row.get(4)?,
		updated_at_unix: row.get(5)?,
	})
}

fn optional_json_from_row<T>(
	row: &Row<'_>,
	index: usize,
) -> std::result::Result<Option<T>, rusqlite::Error>
where
	T: DeserializeOwned,
{
	let value: Option<String> = row.get(index)?;

	value
		.map(|value| {
			serde_json::from_str(&value).map_err(|error| {
				rusqlite::Error::FromSqlConversionFailure(index, Type::Text, Box::new(error))
			})
		})
		.transpose()
}

pub(super) fn worktree_mapping_record_from_row(
	row: &Row<'_>,
) -> std::result::Result<WorktreeMappingRecord, rusqlite::Error> {
	Ok(WorktreeMappingRecord {
		issue_id: row.get(0)?,
		project_id: row.get(1)?,
		branch_name: row.get(2)?,
		worktree_path: PathBuf::from(row.get::<_, String>(3)?),
		provenance_source: row.get(4)?,
		created_at_unix: row.get(5)?,
		updated_at_unix: row.get(6)?,
	})
}

pub(super) fn decision_contract_runtime_row_parts(
	row: &Row<'_>,
) -> std::result::Result<DecisionContractRuntimeRowParts, rusqlite::Error> {
	Ok(DecisionContractRuntimeRowParts {
		project_id: row.get(0)?,
		contract_id: row.get(1)?,
		source_issue_id: row.get(2)?,
		status: row.get(3)?,
		payload_json: row.get(4)?,
		created_at: row.get(5)?,
		created_at_unix: row.get(6)?,
		updated_at: row.get(7)?,
		updated_at_unix: row.get(8)?,
	})
}

pub(super) fn decision_contract_record_from_row_parts(
	parts: DecisionContractRuntimeRowParts,
) -> Result<DecisionContractRuntimeRecord> {
	let contract = serde_json::from_str::<DecisionContract>(&parts.payload_json)?;
	let contract_status = contract.status();

	contract.validate()?;

	if parts.contract_id != contract.contract_id() {
		eyre::bail!(
			"Decision contract row `{}` contained payload `{}`.",
			parts.contract_id,
			contract.contract_id()
		);
	}
	if parts.status != contract_status.as_str() {
		tracing::warn!(
			project_id = %parts.project_id,
			contract_id = %parts.contract_id,
			"decision contract status column differed from payload status"
		);
	}

	Ok(DecisionContractRuntimeRecord {
		project_id: parts.project_id,
		source_issue_id: parts.source_issue_id,
		status: contract_status,
		contract,
		created_at: parts.created_at,
		created_at_unix: parts.created_at_unix,
		updated_at: parts.updated_at,
		updated_at_unix: parts.updated_at_unix,
	})
}

pub(super) fn autonomy_objective_runtime_row_parts(
	row: &Row<'_>,
) -> std::result::Result<AutonomyObjectiveRuntimeRowParts, rusqlite::Error> {
	Ok(AutonomyObjectiveRuntimeRowParts {
		project_id: row.get(0)?,
		objective_id: row.get(1)?,
		version: row.get(2)?,
		state: row.get(3)?,
		payload_json: row.get(4)?,
		created_at: row.get(5)?,
		created_at_unix: row.get(6)?,
		updated_at: row.get(7)?,
		updated_at_unix: row.get(8)?,
	})
}

pub(super) fn autonomy_objective_record_from_row_parts(
	parts: AutonomyObjectiveRuntimeRowParts,
) -> Result<AutonomyObjectiveRuntimeRecord> {
	let objective = serde_json::from_str::<AutonomyObjectiveContract>(&parts.payload_json)?;
	let objective_state = objective.state();
	let version = u64::try_from(parts.version)
		.map_err(|_| eyre::eyre!("Autonomy objective row version must be greater than zero."))?;

	objective.validate()?;

	if parts.project_id != objective.project_id() {
		eyre::bail!(
			"Autonomy objective row project `{}` contained payload project `{}`.",
			parts.project_id,
			objective.project_id()
		);
	}
	if parts.objective_id != objective.id() {
		eyre::bail!(
			"Autonomy objective row `{}` contained payload `{}`.",
			parts.objective_id,
			objective.id()
		);
	}
	if version != objective.version() {
		eyre::bail!(
			"Autonomy objective row `{}` version {} contained payload version {}.",
			parts.objective_id,
			version,
			objective.version()
		);
	}
	if parts.state != objective_state.as_str() {
		eyre::bail!(
			"Autonomy objective row `{}` version {} state `{}` differed from payload state `{}`.",
			parts.objective_id,
			version,
			parts.state,
			objective_state.as_str()
		);
	}

	Ok(AutonomyObjectiveRuntimeRecord {
		project_id: parts.project_id,
		state: objective_state,
		objective,
		created_at: parts.created_at,
		created_at_unix: parts.created_at_unix,
		updated_at: parts.updated_at,
		updated_at_unix: parts.updated_at_unix,
	})
}

pub(super) fn autonomy_signal_runtime_row_parts(
	row: &Row<'_>,
) -> std::result::Result<AutonomySignalRuntimeRowParts, rusqlite::Error> {
	Ok(AutonomySignalRuntimeRowParts {
		project_id: row.get(0)?,
		signal_id: row.get(1)?,
		objective_id: row.get(2)?,
		objective_version: row.get(3)?,
		kind: row.get(4)?,
		fingerprint: row.get(5)?,
		freshness: row.get(6)?,
		evidence_class: row.get(7)?,
		confidence: row.get(8)?,
		privacy: row.get(9)?,
		payload_json: row.get(10)?,
		created_at: row.get(11)?,
		created_at_unix: row.get(12)?,
		updated_at: row.get(13)?,
		updated_at_unix: row.get(14)?,
	})
}

pub(super) fn autonomy_signal_record_from_row_parts(
	parts: AutonomySignalRuntimeRowParts,
) -> Result<AutonomySignalRuntimeRecord> {
	let signal = serde_json::from_str::<AutonomySignal>(&parts.payload_json)?;
	let version = u64::try_from(parts.objective_version).map_err(|_| {
		eyre::eyre!("Autonomy signal row objective_version must be greater than zero.")
	})?;

	signal.validate()?;

	if parts.project_id != signal.project_id() {
		eyre::bail!(
			"Autonomy signal row project `{}` contained payload project `{}`.",
			parts.project_id,
			signal.project_id()
		);
	}
	if parts.signal_id != signal.id() {
		eyre::bail!(
			"Autonomy signal row `{}` contained payload `{}`.",
			parts.signal_id,
			signal.id()
		);
	}
	if parts.objective_id != signal.objective_id() {
		eyre::bail!(
			"Autonomy signal row objective `{}` contained payload `{}`.",
			parts.objective_id,
			signal.objective_id()
		);
	}
	if version != signal.objective_version() {
		eyre::bail!(
			"Autonomy signal row `{}` objective version {} contained payload version {}.",
			parts.signal_id,
			version,
			signal.objective_version()
		);
	}
	if parts.kind != signal.kind().as_str()
		|| parts.fingerprint != signal.fingerprint()
		|| parts.freshness != signal.freshness().as_str()
		|| parts.evidence_class != signal.evidence_class().as_str()
		|| parts.confidence != signal.confidence().as_str()
		|| parts.privacy != signal.privacy().as_str()
	{
		eyre::bail!(
			"Autonomy signal row `{}` readback columns differed from payload.",
			parts.signal_id
		);
	}

	Ok(AutonomySignalRuntimeRecord {
		project_id: parts.project_id,
		signal,
		created_at: parts.created_at,
		created_at_unix: parts.created_at_unix,
		updated_at: parts.updated_at,
		updated_at_unix: parts.updated_at_unix,
	})
}

pub(super) fn autonomy_proposal_runtime_row_parts(
	row: &Row<'_>,
) -> std::result::Result<AutonomyProposalRuntimeRowParts, rusqlite::Error> {
	Ok(AutonomyProposalRuntimeRowParts {
		project_id: row.get(0)?,
		proposal_id: row.get(1)?,
		objective_id: row.get(2)?,
		objective_version: row.get(3)?,
		state: row.get(4)?,
		fingerprint: row.get(5)?,
		source_family: row.get(6)?,
		intended_surface: row.get(7)?,
		payload_json: row.get(8)?,
		created_at: row.get(9)?,
		created_at_unix: row.get(10)?,
		updated_at: row.get(11)?,
		updated_at_unix: row.get(12)?,
	})
}

pub(super) fn autonomy_proposal_record_from_row_parts(
	parts: AutonomyProposalRuntimeRowParts,
) -> Result<AutonomyProposalRuntimeRecord> {
	let proposal = serde_json::from_str::<AutonomyProposal>(&parts.payload_json)?;
	let version = u64::try_from(parts.objective_version).map_err(|_| {
		eyre::eyre!("Autonomy proposal row objective_version must be greater than zero.")
	})?;

	proposal.validate()?;

	if parts.project_id != proposal.project_id() {
		eyre::bail!(
			"Autonomy proposal row project `{}` contained payload project `{}`.",
			parts.project_id,
			proposal.project_id()
		);
	}
	if parts.proposal_id != proposal.id() {
		eyre::bail!(
			"Autonomy proposal row `{}` contained payload `{}`.",
			parts.proposal_id,
			proposal.id()
		);
	}
	if parts.objective_id != proposal.objective_id() {
		eyre::bail!(
			"Autonomy proposal row objective `{}` contained payload `{}`.",
			parts.objective_id,
			proposal.objective_id()
		);
	}
	if version != proposal.objective_version() {
		eyre::bail!(
			"Autonomy proposal row `{}` objective version {} contained payload version {}.",
			parts.proposal_id,
			version,
			proposal.objective_version()
		);
	}
	if parts.state != proposal.state().as_str()
		|| parts.fingerprint != proposal.fingerprint()
		|| parts.source_family != proposal.source_family()
		|| parts.intended_surface != proposal.intended_surface()
	{
		eyre::bail!(
			"Autonomy proposal row `{}` readback columns differed from payload.",
			parts.proposal_id
		);
	}

	Ok(AutonomyProposalRuntimeRecord {
		project_id: parts.project_id,
		state: proposal.state(),
		proposal,
		created_at: parts.created_at,
		created_at_unix: parts.created_at_unix,
		updated_at: parts.updated_at,
		updated_at_unix: parts.updated_at_unix,
	})
}

pub(super) fn migrate_legacy_decision_contract_payload(payload_json: &str) -> Result<String> {
	let mut payload = serde_json::from_str::<Value>(payload_json)?;
	let readiness = payload
		.get_mut("execution_readiness")
		.and_then(Value::as_object_mut)
		.ok_or_else(|| eyre::eyre!("Decision Contract payload missing execution_readiness."))?;
	let summaries = readiness.remove("proposed_issue_summaries");
	readiness.remove("queue_intent");

	let should_insert_issues =
		readiness.get("proposed_issues").and_then(Value::as_array).is_none_or(Vec::is_empty);

	if should_insert_issues {
		let summaries = legacy_issue_summary_values(summaries.as_ref());

		readiness.insert(
			String::from("proposed_issues"),
			Value::Array(
				summaries
					.iter()
					.enumerate()
					.map(|(index, summary)| legacy_issue_summary_to_proposed_issue(index, summary))
					.collect(),
			),
		);
	}

	let contract = serde_json::from_value::<DecisionContract>(payload.clone())?;

	contract.validate()?;

	Ok(serde_json::to_string(&payload)?)
}

fn legacy_issue_summary_values(value: Option<&Value>) -> Vec<String> {
	let summaries = match value {
		Some(Value::Array(values)) => values
			.iter()
			.map(|value| {
				value
					.as_str()
					.map(str::to_owned)
					.unwrap_or_else(|| value.to_string())
					.trim()
					.to_owned()
			})
			.filter(|value| !value.is_empty())
			.collect::<Vec<_>>(),
		Some(Value::String(value)) if !value.trim().is_empty() => vec![value.trim().to_owned()],
		Some(value) => vec![value.to_string()],
		None => Vec::new(),
	};

	if summaries.is_empty() {
		vec![String::from("Legacy proposed issue summary was empty.")]
	} else {
		summaries
	}
}

fn legacy_issue_summary_to_proposed_issue(index: usize, summary: &str) -> Value {
	let issue_number = index + 1;

	serde_json::json!({
		"key": format!("legacy-proposed-issue-{issue_number}"),
		"title": format!("Legacy proposed issue {issue_number}"),
		"objective": summary,
		"stage": "handoff",
		"dependencies": [],
		"conflict_domains": ["legacy_decision_contract_migration"],
		"acceptance": [
			format!("Review and preserve the migrated legacy proposed issue summary: {summary}")
		],
		"validation": [
			"Review the migrated legacy proposed issue before promotion or intake."
		],
		"risk": [
			"Migrated from removed proposed_issue_summaries; structured fields may be incomplete."
		],
		"queue_intent": "not_ready"
	})
}

pub(super) fn execution_program_runtime_row_parts(
	row: &Row<'_>,
) -> std::result::Result<ExecutionProgramRuntimeRowParts, rusqlite::Error> {
	Ok(ExecutionProgramRuntimeRowParts {
		project_id: row.get(0)?,
		program_id: row.get(1)?,
		source_contract_id: row.get(2)?,
		payload_json: row.get(3)?,
		created_at: row.get(4)?,
		created_at_unix: row.get(5)?,
		updated_at: row.get(6)?,
		updated_at_unix: row.get(7)?,
	})
}

pub(super) fn execution_program_record_from_row_parts(
	parts: ExecutionProgramRuntimeRowParts,
) -> Result<ExecutionProgramRuntimeRecord> {
	let program = serde_json::from_str::<ExecutionProgram>(&parts.payload_json)?;

	program.validate()?;

	if parts.program_id != program.program_id() {
		eyre::bail!(
			"Execution program row `{}` contained payload `{}`.",
			parts.program_id,
			program.program_id()
		);
	}
	if parts.source_contract_id.as_deref() != program.source_contract_id() {
		eyre::bail!(
			"Execution program row `{}` carried source contract `{}` but payload references `{}`.",
			parts.program_id,
			parts.source_contract_id.as_deref().unwrap_or("none"),
			program.source_contract_id().unwrap_or("none")
		);
	}

	Ok(ExecutionProgramRuntimeRecord {
		project_id: parts.project_id,
		source_contract_id: parts.source_contract_id,
		program,
		created_at: parts.created_at,
		created_at_unix: parts.created_at_unix,
		updated_at: parts.updated_at,
		updated_at_unix: parts.updated_at_unix,
	})
}

pub(super) fn program_intake_plan_row(
	row: &Row<'_>,
) -> std::result::Result<ProgramIntakePlanRecord, rusqlite::Error> {
	Ok(ProgramIntakePlanRecord {
		project_id: row.get(0)?,
		program_id: row.get(1)?,
		plan_id: row.get(2)?,
		intake_kind: row.get(3)?,
		source_contract_id: row.get(4)?,
		accepted_contract_fingerprint: row.get(5)?,
		public_summary: row.get(6)?,
		created_at: row.get(7)?,
		created_at_unix: row.get(8)?,
		updated_at: row.get(9)?,
		updated_at_unix: row.get(10)?,
	})
}

pub(super) fn program_issue_mapping_row(
	row: &Row<'_>,
) -> std::result::Result<ProgramIssueMappingRecord, rusqlite::Error> {
	Ok(ProgramIssueMappingRecord {
		project_id: row.get(0)?,
		program_id: row.get(1)?,
		node_id: row.get(2)?,
		issue_id: row.get(3)?,
		issue_identifier: row.get(4)?,
		issue_state: row.get(5)?,
		queue_intent: row.get(6)?,
		has_active_label: sqlite_bool(row, 7)?,
		has_opt_out_label: sqlite_bool(row, 8)?,
		has_needs_attention_label: sqlite_bool(row, 9)?,
		has_generic_dispatch_briefing: sqlite_bool(row, 10)?,
		created_at: row.get(11)?,
		created_at_unix: row.get(12)?,
		updated_at: row.get(13)?,
		updated_at_unix: row.get(14)?,
	})
}

fn sqlite_bool(row: &Row<'_>, index: usize) -> std::result::Result<bool, rusqlite::Error> {
	Ok(row.get::<_, i64>(index)? != 0)
}

pub(super) fn sqlite_bool_value(value: bool) -> i64 {
	if value { 1 } else { 0 }
}

pub(super) fn connector_backoff_from_row(
	row: &Row<'_>,
) -> std::result::Result<ConnectorBackoff, rusqlite::Error> {
	Ok(ConnectorBackoff {
		project_id: row.get(0)?,
		connector: row.get(1)?,
		sync_phase: row.get(2)?,
		quota_class: row.get(3)?,
		reset_unix_epoch: row.get(4)?,
		reset_source: row.get(5)?,
		warning: row.get(6)?,
		updated_at: row.get(7)?,
		updated_at_unix: row.get(8)?,
	})
}

pub(super) fn compare_linear_execution_event_runtime_records(
	left: &LinearExecutionEventRuntimeRecord,
	right: &LinearExecutionEventRuntimeRecord,
) -> cmp::Ordering {
	left.event_unix
		.cmp(&right.event_unix)
		.then_with(|| left.recorded_at_unix.cmp(&right.recorded_at_unix))
		.then_with(|| left.record.idempotency_key.cmp(&right.record.idempotency_key))
}

pub(super) fn compare_private_execution_event_runtime_records(
	left: &PrivateExecutionEventRuntimeRecord,
	right: &PrivateExecutionEventRuntimeRecord,
) -> cmp::Ordering {
	left.record_id.cmp(&right.record_id)
}

#[allow(dead_code)]
pub(super) fn compare_decision_contract_runtime_records(
	left: &DecisionContractRuntimeRecord,
	right: &DecisionContractRuntimeRecord,
) -> cmp::Ordering {
	left.updated_at_unix
		.cmp(&right.updated_at_unix)
		.then_with(|| left.contract.contract_id().cmp(right.contract.contract_id()))
}

#[allow(dead_code)]
pub(super) fn compare_autonomy_signal_runtime_records(
	left: &AutonomySignalRuntimeRecord,
	right: &AutonomySignalRuntimeRecord,
) -> cmp::Ordering {
	left.updated_at_unix
		.cmp(&right.updated_at_unix)
		.then_with(|| left.signal.id().cmp(right.signal.id()))
}

#[allow(dead_code)]
pub(super) fn compare_recent_autonomy_signal_runtime_records(
	left: &AutonomySignalRuntimeRecord,
	right: &AutonomySignalRuntimeRecord,
) -> cmp::Ordering {
	right
		.updated_at_unix
		.cmp(&left.updated_at_unix)
		.then_with(|| left.signal.id().cmp(right.signal.id()))
}

#[allow(dead_code)]
pub(super) fn compare_autonomy_proposal_runtime_records(
	left: &AutonomyProposalRuntimeRecord,
	right: &AutonomyProposalRuntimeRecord,
) -> cmp::Ordering {
	left.updated_at_unix
		.cmp(&right.updated_at_unix)
		.then_with(|| left.proposal.id().cmp(right.proposal.id()))
}

#[allow(dead_code)]
pub(super) fn compare_recent_autonomy_proposal_runtime_records(
	left: &AutonomyProposalRuntimeRecord,
	right: &AutonomyProposalRuntimeRecord,
) -> cmp::Ordering {
	right
		.updated_at_unix
		.cmp(&left.updated_at_unix)
		.then_with(|| left.proposal.id().cmp(right.proposal.id()))
}

#[allow(dead_code)]
pub(super) fn compare_execution_program_runtime_records(
	left: &ExecutionProgramRuntimeRecord,
	right: &ExecutionProgramRuntimeRecord,
) -> cmp::Ordering {
	left.updated_at_unix
		.cmp(&right.updated_at_unix)
		.then_with(|| left.program.program_id().cmp(right.program.program_id()))
}

pub(super) fn compare_program_intake_plan_records(
	left: &ProgramIntakePlanRecord,
	right: &ProgramIntakePlanRecord,
) -> cmp::Ordering {
	left.updated_at_unix
		.cmp(&right.updated_at_unix)
		.then_with(|| left.program_id.cmp(&right.program_id))
		.then_with(|| left.plan_id.cmp(&right.plan_id))
}

pub(super) fn compare_program_issue_mapping_records(
	left: &ProgramIssueMappingRecord,
	right: &ProgramIssueMappingRecord,
) -> cmp::Ordering {
	left.updated_at_unix
		.cmp(&right.updated_at_unix)
		.then_with(|| left.program_id.cmp(&right.program_id))
		.then_with(|| left.node_id.cmp(&right.node_id))
}
