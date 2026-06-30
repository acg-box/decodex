use std::collections::BTreeSet;

use serde_json::Value;

use crate::{
	config::ServiceConfig,
	state::{
		DecisionContractRecord, PrivateExecutionEvent, ProjectLoopEvidenceSnapshot,
		ReviewLifecycleRecord,
	},
	tracker::public_text,
};

use super::{
	OperatorAutonomyDecisionContractStatus, OperatorAutonomyExecutionEvidenceStatus,
	OperatorAutonomyLineageStatus, OperatorAutonomyObjectiveStatus,
	OperatorAutonomyProgramIntakeStatus, OperatorAutonomyProposalRefusalStatus,
	OperatorAutonomyProposalStatus, OperatorAutonomyReportReadbackStatus,
	OperatorAutonomySignalStatus,
};

const AUTONOMY_REPLAY_EVIDENCE_SCHEMA: &str = "decodex.autonomy_replay_evidence/1";

pub(super) fn operator_autonomy_objective_status(
	project: &ServiceConfig,
	loop_evidence: &ProjectLoopEvidenceSnapshot,
) -> Option<OperatorAutonomyObjectiveStatus> {
	if let Some(policy) = project.autonomy().runtime_policy() {
		let version = policy.accepted_objective_version().parse::<u64>().unwrap_or_default();
		let source_ref = operator_autonomy_objective_ref(policy.accepted_objective_id(), version);

		if let Some(record) =
			loop_evidence.autonomy_objective(policy.accepted_objective_id(), version)
		{
			let objective = record.objective();
			let mut known_gaps = Vec::new();

			if record.state().as_str() != "accepted" {
				known_gaps.push(format!("objective_state_{}", record.state().as_str()));
			}

			return Some(OperatorAutonomyObjectiveStatus {
				objective_id: objective.id().to_owned(),
				objective_version: objective.version(),
				state: objective.state().as_str().to_owned(),
				summary: public_or_redacted_status_value(objective.summary()),
				source_ref,
				updated_at: record.updated_at().to_owned(),
				completeness: operator_autonomy_completeness(&known_gaps),
				known_gaps,
			});
		}

		let mut known_gaps = vec![String::from("objective_runtime_record_missing")];

		if version == 0 {
			known_gaps.push(String::from("objective_version_unparseable"));
		}

		return Some(OperatorAutonomyObjectiveStatus {
			objective_id: policy.accepted_objective_id().to_owned(),
			objective_version: version,
			state: String::from("missing_runtime_record"),
			summary: String::from(
				"Accepted runtime policy references an Objective Contract that is not in local readback.",
			),
			source_ref,
			updated_at: String::from("none"),
			completeness: String::from("partial"),
			known_gaps,
		});
	}

	loop_evidence.accepted_autonomy_objectives().into_iter().next().map(|record| {
		let objective = record.objective();

		OperatorAutonomyObjectiveStatus {
			objective_id: objective.id().to_owned(),
			objective_version: objective.version(),
			state: objective.state().as_str().to_owned(),
			summary: public_or_redacted_status_value(objective.summary()),
			source_ref: operator_autonomy_objective_ref(objective.id(), objective.version()),
			updated_at: record.updated_at().to_owned(),
			completeness: String::from("complete"),
			known_gaps: Vec::new(),
		}
	})
}

pub(super) fn operator_autonomy_signal_statuses(
	loop_evidence: &ProjectLoopEvidenceSnapshot,
) -> Vec<OperatorAutonomySignalStatus> {
	loop_evidence
		.recent_autonomy_signals(5)
		.into_iter()
		.map(|record| {
			let signal = record.signal();
			let (source_refs, source_refs_redacted) = public_autonomy_refs(signal.source_refs());
			let (primary_source_refs, primary_source_refs_redacted) =
				public_autonomy_refs(signal.primary_source_refs());
			let (gaps, gaps_redacted) = public_status_values(signal.gaps());
			let (contradictions, contradictions_redacted) =
				public_status_values(signal.contradictions());
			let mut known_gaps = gaps.clone();

			if source_refs.is_empty() {
				known_gaps.push(String::from("source_refs_missing_or_redacted"));
			}
			if source_refs_redacted || primary_source_refs_redacted {
				known_gaps.push(String::from("source_refs_redacted"));
			}
			if gaps_redacted || contradictions_redacted {
				known_gaps.push(String::from("gap_or_contradiction_redacted"));
			}
			if signal.freshness().as_str() != "fresh" {
				known_gaps.push(format!("freshness_{}", signal.freshness().as_str()));
			}

			known_gaps.sort();
			known_gaps.dedup();
			OperatorAutonomySignalStatus {
				signal_id: signal.id().to_owned(),
				objective_id: signal.objective_id().to_owned(),
				objective_version: signal.objective_version(),
				kind: signal.kind().as_str().to_owned(),
				source_type: signal.source_type().as_str().to_owned(),
				source_refs,
				primary_source_refs,
				freshness: signal.freshness().as_str().to_owned(),
				evidence_class: signal.evidence_class().as_str().to_owned(),
				confidence: signal.confidence().as_str().to_owned(),
				privacy: signal.privacy().as_str().to_owned(),
				redaction_level: signal.privacy().as_str().to_owned(),
				completeness: operator_autonomy_completeness(&known_gaps),
				gaps,
				known_gaps,
				contradictions,
				updated_at: record.updated_at().to_owned(),
			}
		})
		.collect()
}

pub(super) fn operator_autonomy_proposal_statuses(
	loop_evidence: &ProjectLoopEvidenceSnapshot,
) -> Vec<OperatorAutonomyProposalStatus> {
	loop_evidence
		.recent_autonomy_proposals(5)
		.into_iter()
		.map(|record| {
			let proposal = record.proposal();
			let (source_family, source_family_redacted) =
				public_status_value(proposal.source_family());
			let (intended_surface, intended_surface_redacted) =
				public_status_value(proposal.intended_surface());
			let (affected_identifiers, affected_identifiers_redacted) =
				public_status_values(proposal.affected_identifiers());
			let (gaps, gaps_redacted) = public_status_values(proposal.gaps());
			let (contradictions, contradictions_redacted) =
				public_status_values(proposal.contradictions());
			let refusals = proposal
				.refusal_reasons()
				.iter()
				.map(|refusal| {
					let (evidence_refs, _) = public_autonomy_refs(refusal.evidence_refs());

					OperatorAutonomyProposalRefusalStatus {
						reason: refusal.reason().as_str().to_owned(),
						detail: public_or_redacted_status_value(refusal.detail()),
						evidence_refs,
					}
				})
				.collect::<Vec<_>>();
			let mut known_gaps = gaps.clone();

			if proposal.source_signal_ids().is_empty() {
				known_gaps.push(String::from("source_signal_ids_missing"));
			}
			if !proposal.refusal_reasons().is_empty() {
				known_gaps.push(String::from("proposal_refused"));
			}
			if source_family_redacted
				|| intended_surface_redacted
				|| affected_identifiers_redacted
				|| gaps_redacted
				|| contradictions_redacted
			{
				known_gaps.push(String::from("proposal_public_fields_redacted"));
			}

			known_gaps.sort();
			known_gaps.dedup();
			OperatorAutonomyProposalStatus {
				proposal_id: proposal.id().to_owned(),
				objective_id: proposal.objective_id().to_owned(),
				objective_version: proposal.objective_version(),
				state: proposal.state().as_str().to_owned(),
				summary: public_or_redacted_status_value(proposal.summary()),
				source_family,
				intended_surface,
				affected_identifiers,
				source_signal_ids: proposal.source_signal_ids().to_vec(),
				refusal_reasons: proposal
					.refusal_reasons()
					.iter()
					.map(|refusal| refusal.reason().as_str().to_owned())
					.collect(),
				refusals,
				completeness: operator_autonomy_completeness(&known_gaps),
				known_gaps,
				gaps,
				contradictions,
				challenge_evidence_count: proposal.challenge_evidence().len(),
				updated_at: record.updated_at().to_owned(),
			}
		})
		.collect()
}

pub(super) fn operator_autonomy_lineage_statuses(
	loop_evidence: &ProjectLoopEvidenceSnapshot,
) -> Vec<OperatorAutonomyLineageStatus> {
	loop_evidence
		.recent_autonomy_proposals(5)
		.into_iter()
		.map(|record| {
			let proposal = record.proposal();
			let contract_records =
				loop_evidence.decision_contracts_for_autonomy_proposal(proposal.id());
			let decision_contracts = contract_records
				.iter()
				.map(|record| OperatorAutonomyDecisionContractStatus {
					contract_id: record.contract_id().to_owned(),
					status: record.status().as_str().to_owned(),
					updated_at: record.updated_at().to_owned(),
					generated_issue_identifiers: record
						.contract()
						.links()
						.generated_issue_identifiers()
						.to_vec(),
				})
				.collect::<Vec<_>>();
			let execution_evidence = operator_autonomy_execution_evidence_statuses(
				loop_evidence,
				proposal.id(),
				&contract_records,
			);
			let program_intake = decision_contracts
				.iter()
				.flat_map(|contract| {
					loop_evidence
						.program_intake_plans_for_contract(&contract.contract_id)
						.into_iter()
						.map(|plan| OperatorAutonomyProgramIntakeStatus {
							program_id: plan.program_id().to_owned(),
							plan_id: plan.plan_id().to_owned(),
							intake_kind: plan.intake_kind().to_owned(),
							source_contract_id: plan
								.source_contract_id()
								.unwrap_or("none")
								.to_owned(),
							public_summary: public_or_redacted_status_value(plan.public_summary()),
							updated_at: plan.updated_at().to_owned(),
						})
						.collect::<Vec<_>>()
				})
				.collect::<Vec<_>>();
			let mut known_gaps = Vec::new();

			if proposal.source_signal_ids().is_empty() {
				known_gaps.push(String::from("signal_lineage_missing"));
			}
			if decision_contracts.is_empty() {
				known_gaps.push(String::from("decision_contract_not_materialized"));
			}
			if program_intake.is_empty() {
				known_gaps.push(String::from("program_intake_not_materialized"));
			}
			if !program_intake.is_empty() {
				let evidence_kinds = execution_evidence
					.iter()
					.map(|evidence| evidence.kind.as_str())
					.collect::<BTreeSet<_>>();

				for (kind, gap) in [
					("pr", "pr_evidence_missing"),
					("validation", "validation_evidence_missing"),
					("post_land", "post_land_evidence_missing"),
				] {
					if !evidence_kinds.contains(kind) {
						known_gaps.push(String::from(gap));
					}
				}

				known_gaps.extend(
					execution_evidence
						.iter()
						.flat_map(|evidence| evidence.known_gaps.iter().cloned()),
				);
			}

			let (proposal_gaps, proposal_gaps_redacted) = public_status_values(proposal.gaps());

			known_gaps.extend(proposal_gaps);

			if proposal_gaps_redacted {
				known_gaps.push(String::from("proposal_gaps_redacted"));
			}

			known_gaps.sort();
			known_gaps.dedup();
			OperatorAutonomyLineageStatus {
				objective_ref: operator_autonomy_objective_ref(
					proposal.objective_id(),
					proposal.objective_version(),
				),
				signal_ids: proposal.source_signal_ids().to_vec(),
				proposal_id: Some(proposal.id().to_owned()),
				proposal_state: Some(proposal.state().as_str().to_owned()),
				decision_contracts,
				program_intake,
				execution_evidence,
				completeness: operator_autonomy_completeness(&known_gaps),
				known_gaps,
			}
		})
		.collect()
}

fn operator_autonomy_execution_evidence_statuses(
	loop_evidence: &ProjectLoopEvidenceSnapshot,
	proposal_id: &str,
	contracts: &[&DecisionContractRecord],
) -> Vec<OperatorAutonomyExecutionEvidenceStatus> {
	let contract_ids = contracts.iter().map(|record| record.contract_id()).collect::<BTreeSet<_>>();
	let mut evidence = Vec::new();

	for (issue_id, issue_identifier) in operator_autonomy_generated_issue_pairs(contracts) {
		let review_lifecycle_records = loop_evidence.review_lifecycle_records_for_issue(&issue_id);

		for event in loop_evidence.private_events_for_issue(&issue_id) {
			if let Some(status) = operator_autonomy_replay_evidence_status_from_event(
				event,
				proposal_id,
				&contract_ids,
				issue_identifier.as_deref(),
				&review_lifecycle_records,
			) {
				evidence.push(status);
			}
		}
	}

	evidence.sort_by(|left, right| {
		left.kind
			.cmp(&right.kind)
			.then_with(|| left.issue_identifier.cmp(&right.issue_identifier))
			.then_with(|| left.source_refs.cmp(&right.source_refs))
			.then_with(|| {
				operator_autonomy_evidence_completeness_rank(&right.completeness)
					.cmp(&operator_autonomy_evidence_completeness_rank(&left.completeness))
			})
			.then_with(|| right.updated_at.cmp(&left.updated_at))
			.then_with(|| left.summary.cmp(&right.summary))
	});
	evidence.dedup_by(|left, right| {
		left.kind == right.kind
			&& left.issue_identifier == right.issue_identifier
			&& left.source_refs == right.source_refs
	});

	evidence
}

fn operator_autonomy_generated_issue_pairs(
	contracts: &[&DecisionContractRecord],
) -> Vec<(String, Option<String>)> {
	let mut pairs = contracts
		.iter()
		.flat_map(|record| {
			let links = record.contract().links();

			links
				.generated_issue_ids()
				.iter()
				.enumerate()
				.map(|(index, issue_id)| {
					(issue_id.clone(), links.generated_issue_identifiers().get(index).cloned())
				})
				.collect::<Vec<_>>()
		})
		.collect::<Vec<_>>();

	pairs.sort();
	pairs.dedup();

	pairs
}

fn operator_autonomy_pr_evidence_status_from_event(
	event: &PrivateExecutionEvent,
	review: &ReviewLifecycleRecord,
	issue_identifier: Option<&str>,
	summary: String,
	summary_redacted: bool,
) -> OperatorAutonomyExecutionEvidenceStatus {
	let (source_refs, refs_redacted) = public_autonomy_refs(&[review.pr_url().to_owned()]);
	let mut known_gaps = Vec::new();

	if source_refs.is_empty() {
		known_gaps.push(String::from("source_refs_missing_or_redacted"));
	}
	if refs_redacted {
		known_gaps.push(String::from("source_refs_redacted"));
	}
	if summary_redacted {
		known_gaps.push(String::from("summary_redacted"));
	}

	OperatorAutonomyExecutionEvidenceStatus {
		kind: String::from("pr"),
		issue_identifier: issue_identifier.map(str::to_owned),
		source_refs,
		summary,
		updated_at: [review.updated_at(), event.recorded_at()]
			.into_iter()
			.max()
			.unwrap_or_else(|| event.recorded_at())
			.to_owned(),
		completeness: operator_autonomy_completeness(&known_gaps),
		known_gaps,
	}
}

fn operator_autonomy_replay_evidence_status_from_event(
	event: &PrivateExecutionEvent,
	proposal_id: &str,
	contract_ids: &BTreeSet<&str>,
	issue_identifier: Option<&str>,
	review_lifecycle_records: &[&ReviewLifecycleRecord],
) -> Option<OperatorAutonomyExecutionEvidenceStatus> {
	let payload = event.payload();

	if payload.get("schema").and_then(Value::as_str) != Some(AUTONOMY_REPLAY_EVIDENCE_SCHEMA) {
		return None;
	}
	if !operator_autonomy_replay_evidence_matches(payload, proposal_id, contract_ids) {
		return None;
	}

	let kind = match payload.get("kind").and_then(Value::as_str) {
		Some(kind @ ("pr" | "validation" | "post_land")) => kind.to_owned(),
		_ => return None,
	};
	let raw_source_refs = payload
		.get("source_refs")
		.and_then(Value::as_array)
		.into_iter()
		.flatten()
		.filter_map(Value::as_str)
		.map(str::to_owned)
		.collect::<Vec<_>>();
	let (source_refs, refs_redacted) = public_autonomy_refs(&raw_source_refs);
	let (summary, summary_redacted) = public_status_value(
		payload
			.get("summary")
			.and_then(Value::as_str)
			.unwrap_or("Dogfood replay evidence recorded."),
	);
	let mut known_gaps = Vec::new();

	if kind == "pr" {
		let pr_head_ref = payload
			.get("pr_head_ref")
			.and_then(Value::as_str)
			.map(str::trim)
			.filter(|value| !value.is_empty());
		let pr_head_oid = payload
			.get("pr_head_oid")
			.and_then(Value::as_str)
			.map(str::trim)
			.filter(|value| !value.is_empty());

		return Some(
			match operator_autonomy_matching_pr_review(
				event,
				&raw_source_refs,
				pr_head_ref,
				pr_head_oid,
				review_lifecycle_records,
			) {
				Some(review) => operator_autonomy_pr_evidence_status_from_event(
					event,
					review,
					issue_identifier,
					summary,
					summary_redacted,
				),
				None => {
					if source_refs.is_empty() {
						known_gaps.push(String::from("source_refs_missing_or_redacted"));
					}
					if refs_redacted {
						known_gaps.push(String::from("source_refs_redacted"));
					}
					if summary_redacted {
						known_gaps.push(String::from("summary_redacted"));
					}
					if pr_head_ref.is_none() || pr_head_oid.is_none() {
						known_gaps.push(String::from("pr_head_identity_missing"));
					} else if operator_autonomy_pr_review_candidate_exists(
						event,
						&raw_source_refs,
						review_lifecycle_records,
					) {
						known_gaps.push(String::from("review_lifecycle_stale_or_mismatched"));
					} else {
						known_gaps.push(String::from("review_lifecycle_missing"));
					}

					OperatorAutonomyExecutionEvidenceStatus {
						kind,
						issue_identifier: issue_identifier.map(str::to_owned),
						source_refs,
						summary,
						updated_at: event.recorded_at().to_owned(),
						completeness: operator_autonomy_completeness(&known_gaps),
						known_gaps,
					}
				},
			},
		);
	}
	if source_refs.is_empty() {
		known_gaps.push(String::from("source_refs_missing_or_redacted"));
	}
	if refs_redacted {
		known_gaps.push(String::from("source_refs_redacted"));
	}
	if summary_redacted {
		known_gaps.push(String::from("summary_redacted"));
	}

	Some(OperatorAutonomyExecutionEvidenceStatus {
		kind,
		issue_identifier: issue_identifier.map(str::to_owned),
		source_refs,
		summary,
		updated_at: event.recorded_at().to_owned(),
		completeness: operator_autonomy_completeness(&known_gaps),
		known_gaps,
	})
}

fn operator_autonomy_matching_pr_review<'a>(
	event: &PrivateExecutionEvent,
	raw_source_refs: &[String],
	pr_head_ref: Option<&str>,
	pr_head_oid: Option<&str>,
	review_lifecycle_records: &'a [&'a ReviewLifecycleRecord],
) -> Option<&'a ReviewLifecycleRecord> {
	let pr_head_ref = pr_head_ref?;
	let pr_head_oid = pr_head_oid?;
	let raw_source_refs =
		raw_source_refs.iter().map(|source_ref| source_ref.trim()).collect::<BTreeSet<_>>();

	review_lifecycle_records
		.iter()
		.copied()
		.filter(|review| {
			review.run_id() == event.run_id()
				&& review.attempt_number() == event.attempt_number()
				&& raw_source_refs.contains(review.pr_url())
				&& review.branch_name() == pr_head_ref
				&& review.pr_head_ref_name() == pr_head_ref
				&& review.pr_head_oid() == pr_head_oid
				&& review.head_sha() == pr_head_oid
		})
		.max_by(|left, right| {
			left.updated_at_unix()
				.cmp(&right.updated_at_unix())
				.then_with(|| left.branch_name().cmp(right.branch_name()))
		})
}

fn operator_autonomy_pr_review_candidate_exists(
	event: &PrivateExecutionEvent,
	raw_source_refs: &[String],
	review_lifecycle_records: &[&ReviewLifecycleRecord],
) -> bool {
	let raw_source_refs =
		raw_source_refs.iter().map(|source_ref| source_ref.trim()).collect::<BTreeSet<_>>();

	review_lifecycle_records.iter().any(|review| {
		review.run_id() == event.run_id()
			&& review.attempt_number() == event.attempt_number()
			&& raw_source_refs.contains(review.pr_url())
	})
}

fn operator_autonomy_replay_evidence_matches(
	payload: &Value,
	proposal_id: &str,
	contract_ids: &BTreeSet<&str>,
) -> bool {
	payload.get("proposal_id").and_then(Value::as_str) == Some(proposal_id)
		|| payload
			.get("contract_id")
			.and_then(Value::as_str)
			.is_some_and(|contract_id| contract_ids.contains(contract_id))
}

pub(super) fn operator_autonomy_report_status(
	objective: Option<&OperatorAutonomyObjectiveStatus>,
	signals: &[OperatorAutonomySignalStatus],
	proposals: &[OperatorAutonomyProposalStatus],
	lineage: &[OperatorAutonomyLineageStatus],
) -> Option<OperatorAutonomyReportReadbackStatus> {
	if objective.is_none() && signals.is_empty() && proposals.is_empty() && lineage.is_empty() {
		return None;
	}

	let mut source_refs = BTreeSet::new();
	let mut known_gaps = BTreeSet::new();
	let mut redaction_level = "public";

	if let Some(objective) = objective {
		source_refs.insert(objective.source_ref.clone());

		for gap in &objective.known_gaps {
			known_gaps.insert(gap.clone());
		}
	}

	for signal in signals {
		for source_ref in &signal.source_refs {
			source_refs.insert(source_ref.clone());
		}
		for primary_source_ref in &signal.primary_source_refs {
			source_refs.insert(primary_source_ref.clone());
		}
		for gap in &signal.known_gaps {
			known_gaps.insert(gap.clone());
		}

		redaction_level = operator_autonomy_max_redaction_level(redaction_level, &signal.privacy);
	}
	for proposal in proposals {
		for gap in &proposal.known_gaps {
			known_gaps.insert(gap.clone());
		}
	}
	for item in lineage {
		for evidence in &item.execution_evidence {
			for source_ref in &evidence.source_refs {
				source_refs.insert(source_ref.clone());
			}
			for gap in &evidence.known_gaps {
				known_gaps.insert(gap.clone());
			}
		}
		for gap in &item.known_gaps {
			known_gaps.insert(gap.clone());
		}
	}

	if source_refs.is_empty() {
		known_gaps.insert(String::from("source_refs_missing_or_redacted"));
	}

	let known_gaps = known_gaps.into_iter().collect::<Vec<_>>();

	Some(OperatorAutonomyReportReadbackStatus {
		surface: String::from("operator_status_autonomy"),
		authority: String::from("derived_query_view"),
		audit_authority: false,
		source_refs: source_refs.into_iter().collect(),
		redaction_level: redaction_level.to_owned(),
		completeness: operator_autonomy_completeness(&known_gaps),
		known_gaps,
	})
}

fn operator_autonomy_objective_ref(objective_id: &str, objective_version: u64) -> String {
	format!("{objective_id}@v{objective_version}")
}

fn operator_autonomy_completeness(known_gaps: &[String]) -> String {
	if known_gaps.is_empty() { String::from("complete") } else { String::from("partial") }
}

fn operator_autonomy_evidence_completeness_rank(value: &str) -> u8 {
	match value {
		"complete" => 1,
		_ => 0,
	}
}

fn operator_autonomy_max_redaction_level(left: &str, right: &str) -> &'static str {
	match (operator_autonomy_redaction_rank(left), operator_autonomy_redaction_rank(right)) {
		(left_rank, right_rank) if left_rank >= right_rank =>
			operator_autonomy_redaction_label(left),
		_ => operator_autonomy_redaction_label(right),
	}
}

fn operator_autonomy_redaction_rank(value: &str) -> u8 {
	match value {
		"local_private" => 2,
		"team" => 1,
		_ => 0,
	}
}

fn operator_autonomy_redaction_label(value: &str) -> &'static str {
	match value {
		"local_private" => "local_private",
		"team" => "team",
		_ => "public",
	}
}

fn public_autonomy_refs(refs: &[String]) -> (Vec<String>, bool) {
	let mut redacted = false;
	let refs = refs
		.iter()
		.filter_map(|value| {
			let Some(value) = public_autonomy_ref(value) else {
				redacted = true;

				return None;
			};

			Some(value)
		})
		.collect::<BTreeSet<_>>()
		.into_iter()
		.collect::<Vec<_>>();

	(refs, redacted)
}

fn public_autonomy_ref(value: &str) -> Option<String> {
	let value = value.trim();

	if value.is_empty()
		|| public_text::validate_public_text_field("autonomy source_ref", value).is_err()
	{
		return None;
	}

	Some(value.to_owned())
}

fn public_status_values(values: &[String]) -> (Vec<String>, bool) {
	let mut redacted = false;
	let values = values
		.iter()
		.map(|value| {
			let (value, value_redacted) = public_status_value(value);

			redacted |= value_redacted;

			value
		})
		.collect();

	(values, redacted)
}

fn public_or_redacted_status_value(value: &str) -> String {
	public_status_value(value).0
}

fn public_status_value(value: &str) -> (String, bool) {
	let value = value.trim();

	if value.is_empty() {
		return (String::from("none"), false);
	}
	if public_text::validate_public_text_field("autonomy status value", value).is_err() {
		return (String::from("redacted_sensitive_detail"), true);
	}

	(value.to_owned(), false)
}
