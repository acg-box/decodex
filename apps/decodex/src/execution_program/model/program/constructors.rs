use crate::{
	execution_program::{
		ExecutionProgramNode, contract,
		intake::ProgramIntakePlan,
		model::{EXECUTION_PROGRAM_RECORD_VERSION, ExecutionProgram},
		validation,
	},
	loop_contract::DecisionContract,
	prelude::Result,
};

impl ExecutionProgram {
	/// Build an internal Execution Program from an accepted Decision Contract.
	pub(crate) fn from_accepted_contract(
		program_id: impl Into<String>,
		service_id: impl Into<String>,
		contract: &DecisionContract,
		mut nodes: Vec<ExecutionProgramNode>,
	) -> Result<Self> {
		contract::ensure_accepted_contract(contract)?;

		let program_id = program_id.into();
		let service_id = service_id.into();
		let fingerprint = contract::decision_contract_fingerprint(contract)?;

		for node in &mut nodes {
			node.bind_contract_fingerprint(&fingerprint);
		}

		let program = Self {
			schema: validation::execution_program_schema(),
			record_version: EXECUTION_PROGRAM_RECORD_VERSION,
			program_id: program_id.clone(),
			service_id: service_id.clone(),
			source_contract_id: Some(contract.contract_id().to_owned()),
			accepted_contract_fingerprint: fingerprint.clone(),
			program_intake_plan: Some(ProgramIntakePlan::goal_intake(
				program_id,
				service_id,
				contract,
				fingerprint.clone(),
			)?),
			nodes,
		};

		program.validate()?;

		Ok(program)
	}

	/// Build an internal Execution Program from an accepted issue-batch intake boundary.
	pub(crate) fn from_issue_batch_intake(
		program_id: impl Into<String>,
		service_id: impl Into<String>,
		accepted_batch_fingerprint: impl Into<String>,
		public_summary: impl Into<String>,
		mut nodes: Vec<ExecutionProgramNode>,
	) -> Result<Self> {
		let program_id = program_id.into();
		let service_id = service_id.into();
		let fingerprint = accepted_batch_fingerprint.into();

		for node in &mut nodes {
			node.bind_contract_fingerprint(&fingerprint);
		}

		let program = Self {
			schema: validation::execution_program_schema(),
			record_version: EXECUTION_PROGRAM_RECORD_VERSION,
			program_id: program_id.clone(),
			service_id: service_id.clone(),
			source_contract_id: None,
			accepted_contract_fingerprint: fingerprint.clone(),
			program_intake_plan: Some(ProgramIntakePlan::issue_batch_intake(
				program_id,
				service_id,
				fingerprint,
				public_summary,
			)?),
			nodes,
		};

		program.validate()?;

		Ok(program)
	}
}
