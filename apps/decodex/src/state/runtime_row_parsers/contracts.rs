use rusqlite::{self, Error, Row};
use serde_json::Value;

use crate::{
	loop_contract::DecisionContract,
	prelude::eyre,
	state::{DecisionContractRuntimeRecord, DecisionContractRuntimeRowParts},
};

pub(in crate::state) fn decision_contract_runtime_row_parts(
	row: &Row<'_>,
) -> std::result::Result<DecisionContractRuntimeRowParts, Error> {
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

pub(in crate::state) fn decision_contract_record_from_row_parts(
	parts: DecisionContractRuntimeRowParts,
) -> crate::prelude::Result<DecisionContractRuntimeRecord> {
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

pub(in crate::state) fn migrate_removed_decision_contract_fields(
	payload_json: &str,
) -> crate::prelude::Result<String> {
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
		let summaries = removed_issue_summary_values(summaries.as_ref());

		readiness.insert(
			String::from("proposed_issues"),
			Value::Array(
				summaries
					.iter()
					.enumerate()
					.map(|(index, summary)| removed_issue_summary_to_proposed_issue(index, summary))
					.collect(),
			),
		);
	}

	let contract = serde_json::from_value::<DecisionContract>(payload.clone())?;

	contract.validate()?;

	Ok(serde_json::to_string(&payload)?)
}

fn removed_issue_summary_values(value: Option<&Value>) -> Vec<String> {
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
		vec![String::from("Migrated proposed issue summary was empty.")]
	} else {
		summaries
	}
}

fn removed_issue_summary_to_proposed_issue(index: usize, summary: &str) -> Value {
	let issue_number = index + 1;

	serde_json::json!({
		"key": format!("migrated-proposed-issue-{issue_number}"),
		"title": format!("Migrated proposed issue {issue_number}"),
		"objective": summary,
		"stage": "handoff",
		"dependencies": [],
		"conflict_domains": ["removed_decision_contract_field_migration"],
		"acceptance": [
			format!("Review and preserve the migrated proposed issue summary: {summary}")
		],
		"validation": [
			"Review the migrated proposed issue before promotion or intake."
		],
		"risk": [
			"Migrated from removed proposed_issue_summaries; structured fields may be incomplete."
		],
		"queue_intent": "not_ready"
	})
}
