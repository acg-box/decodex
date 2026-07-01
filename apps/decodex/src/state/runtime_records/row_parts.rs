pub(in crate::state) struct DecisionContractRuntimeRowParts {
	pub(in crate::state) project_id: String,
	pub(in crate::state) contract_id: String,
	pub(in crate::state) source_issue_id: Option<String>,
	pub(in crate::state) status: String,
	pub(in crate::state) payload_json: String,
	pub(in crate::state) created_at: String,
	pub(in crate::state) created_at_unix: i64,
	pub(in crate::state) updated_at: String,
	pub(in crate::state) updated_at_unix: i64,
}

pub(in crate::state) struct AutonomyObjectiveRuntimeRowParts {
	pub(in crate::state) project_id: String,
	pub(in crate::state) objective_id: String,
	pub(in crate::state) version: i64,
	pub(in crate::state) state: String,
	pub(in crate::state) payload_json: String,
	pub(in crate::state) created_at: String,
	pub(in crate::state) created_at_unix: i64,
	pub(in crate::state) updated_at: String,
	pub(in crate::state) updated_at_unix: i64,
}

pub(in crate::state) struct AutonomySignalRuntimeRowParts {
	pub(in crate::state) project_id: String,
	pub(in crate::state) signal_id: String,
	pub(in crate::state) objective_id: String,
	pub(in crate::state) objective_version: i64,
	pub(in crate::state) kind: String,
	pub(in crate::state) fingerprint: String,
	pub(in crate::state) freshness: String,
	pub(in crate::state) evidence_class: String,
	pub(in crate::state) confidence: String,
	pub(in crate::state) privacy: String,
	pub(in crate::state) payload_json: String,
	pub(in crate::state) created_at: String,
	pub(in crate::state) created_at_unix: i64,
	pub(in crate::state) updated_at: String,
	pub(in crate::state) updated_at_unix: i64,
}

pub(in crate::state) struct AutonomyProposalRuntimeRowParts {
	pub(in crate::state) project_id: String,
	pub(in crate::state) proposal_id: String,
	pub(in crate::state) objective_id: String,
	pub(in crate::state) objective_version: i64,
	pub(in crate::state) state: String,
	pub(in crate::state) fingerprint: String,
	pub(in crate::state) source_family: String,
	pub(in crate::state) intended_surface: String,
	pub(in crate::state) payload_json: String,
	pub(in crate::state) created_at: String,
	pub(in crate::state) created_at_unix: i64,
	pub(in crate::state) updated_at: String,
	pub(in crate::state) updated_at_unix: i64,
}

pub(in crate::state) struct ExecutionProgramRuntimeRowParts {
	pub(in crate::state) project_id: String,
	pub(in crate::state) program_id: String,
	pub(in crate::state) source_contract_id: Option<String>,
	pub(in crate::state) payload_json: String,
	pub(in crate::state) created_at: String,
	pub(in crate::state) created_at_unix: i64,
	pub(in crate::state) updated_at: String,
	pub(in crate::state) updated_at_unix: i64,
}
