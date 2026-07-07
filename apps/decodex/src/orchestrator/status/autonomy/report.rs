use std::collections::BTreeSet;

use crate::orchestrator::{
	OperatorAutonomyLineageStatus, OperatorAutonomyObjectiveStatus, OperatorAutonomyProposalStatus,
	OperatorAutonomyReportReadbackStatus, OperatorAutonomySignalStatus, status_autonomy,
};

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
		completeness: status_autonomy::operator_autonomy_completeness(&known_gaps),
		known_gaps,
	})
}

fn operator_autonomy_max_redaction_level(left: &str, right: &str) -> &'static str {
	match (operator_autonomy_redaction_rank(left), operator_autonomy_redaction_rank(right)) {
		(left_rank, right_rank) if left_rank >= right_rank => {
			operator_autonomy_redaction_label(left)
		},
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
