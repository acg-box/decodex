use crate::{
	config::ServiceConfig,
	orchestrator::{OperatorAutonomyObjectiveStatus, status_autonomy},
	state::ProjectLoopEvidenceSnapshot,
};

pub(super) fn operator_autonomy_objective_status(
	project: &ServiceConfig,
	loop_evidence: &ProjectLoopEvidenceSnapshot,
) -> Option<OperatorAutonomyObjectiveStatus> {
	if let Some(policy) = project.autonomy().runtime_policy() {
		let version = policy.accepted_objective_version().parse::<u64>().unwrap_or_default();
		let source_ref = status_autonomy::operator_autonomy_objective_ref(
			policy.accepted_objective_id(),
			version,
		);

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
				summary: status_autonomy::public_or_redacted_status_value(objective.summary()),
				source_ref,
				updated_at: record.updated_at().to_owned(),
				completeness: status_autonomy::operator_autonomy_completeness(&known_gaps),
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
			summary: status_autonomy::public_or_redacted_status_value(objective.summary()),
			source_ref: status_autonomy::operator_autonomy_objective_ref(
				objective.id(),
				objective.version(),
			),
			updated_at: record.updated_at().to_owned(),
			completeness: String::from("complete"),
			known_gaps: Vec::new(),
		}
	})
}
