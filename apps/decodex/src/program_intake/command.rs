use std::{fs, path::Path};

use sha2::{Digest as _, Sha256};

use crate::{
	autonomy_runtime_policy,
	config::ServiceConfig,
	prelude::{Result, eyre},
	program_intake::{
		self, GoalIntakeCommandRequest, GoalIntakeReport, GoalIntakeRunRequest,
		IssueBatchIntakeCommandRequest, IssueBatchIntakeReport,
	},
	runtime,
	state::ProgramIntakeAttemptClaim,
	tracker::linear::LinearClient,
	workflow::WorkflowDocument,
};

/// Run issue-batch intake through the configured Linear tracker.
pub(crate) fn run_issue_batch_intake_command(
	request: IssueBatchIntakeCommandRequest<'_>,
) -> Result<IssueBatchIntakeReport> {
	if request.dry_run == request.persist {
		eyre::bail!("Issue-batch intake requires exactly one of --dry-run or --apply.");
	}

	let state_store = runtime::open_runtime_store()?;
	let config_path = program_intake::resolve_intake_project_config_path(
		request.config_path,
		request.project_id,
		&state_store,
	)?;
	let config = ServiceConfig::from_path(&config_path)?;
	let workflow = WorkflowDocument::from_path(config.workflow_path())?;

	program_intake::register_intake_project_config_for_persist(
		&state_store,
		&config_path,
		request.persist,
	)?;

	let tracker = LinearClient::new(config.tracker().resolve_api_key()?)?;

	program_intake::run_issue_batch_intake(
		&state_store,
		&tracker,
		&config,
		&workflow,
		request.issue_identifiers,
		request.dry_run,
		request.persist,
	)
}

/// Run promoted-goal intake through the configured Linear tracker.
pub(crate) fn run_goal_intake_command(
	request: GoalIntakeCommandRequest<'_>,
) -> Result<GoalIntakeReport> {
	if request.dry_run == request.apply {
		eyre::bail!("Goal intake requires exactly one of --dry-run or --apply.");
	}

	let state_store = runtime::open_runtime_store()?;
	let config_path = program_intake::resolve_intake_project_config_path(
		request.config_path,
		request.project_id,
		&state_store,
	)?;
	let config = ServiceConfig::from_path(&config_path)?;
	let workflow = WorkflowDocument::from_path(config.workflow_path())?;
	let _authority_lock =
		autonomy_runtime_policy::acquire_autonomy_project_authority_lock(config.service_id())?;

	program_intake::register_intake_project_config_for_persist(
		&state_store,
		&config_path,
		request.apply,
	)?;
	autonomy_runtime_policy::ensure_contract_proposal_still_eligible(
		&config,
		&state_store,
		config.service_id(),
		request.contract_id,
	)?;

	let request_digest = goal_intake_request_digest(
		config.service_id(),
		request.contract_id,
		&config_path,
		config.workflow_path(),
		request.team_issue_identifier,
	)?;

	if request.apply {
		match state_store.begin_program_intake_attempt(
			config.service_id(),
			request.contract_id,
			&request_digest,
		)? {
			ProgramIntakeAttemptClaim::Acquired | ProgramIntakeAttemptClaim::Prepared => {},
			ProgramIntakeAttemptClaim::Started => {
				eyre::bail!("program_intake_attempt_manual_recovery_required")
			},
			ProgramIntakeAttemptClaim::Completed => {
				eyre::bail!("program_intake_attempt_already_completed")
			},
		}
	}

	let tracker = LinearClient::new(config.tracker().resolve_api_key()?)?;

	if request.apply {
		program_intake::run_goal_intake(GoalIntakeRunRequest {
			state_store: &state_store,
			tracker: &tracker,
			config: &config,
			workflow: &workflow,
			contract_id: request.contract_id,
			team_issue_identifier: request.team_issue_identifier.map(str::to_owned),
			dry_run: true,
			apply: false,
		})?;

		state_store
			.mark_program_intake_attempt_started(config.service_id(), request.contract_id)?;
	}

	let report = program_intake::run_goal_intake(GoalIntakeRunRequest {
		state_store: &state_store,
		tracker: &tracker,
		config: &config,
		workflow: &workflow,
		contract_id: request.contract_id,
		team_issue_identifier: request.team_issue_identifier.map(str::to_owned),
		dry_run: request.dry_run,
		apply: request.apply,
	})?;

	if request.apply {
		state_store.complete_program_intake_attempt(config.service_id(), request.contract_id)?;
	}

	Ok(report)
}

fn goal_intake_request_digest(
	project_id: &str,
	contract_id: &str,
	config_path: &Path,
	workflow_path: &Path,
	team_issue_identifier: Option<&str>,
) -> Result<String> {
	let mut digest = Sha256::new();

	for value in [
		project_id.as_bytes().to_vec(),
		contract_id.as_bytes().to_vec(),
		fs::read(config_path)?,
		fs::read(workflow_path)?,
		team_issue_identifier.unwrap_or_default().as_bytes().to_vec(),
	] {
		digest.update((value.len() as u64).to_be_bytes());
		digest.update(value);
	}

	let encoded = digest.finalize().iter().map(|byte| format!("{byte:02x}")).collect::<String>();

	Ok(format!("sha256:{encoded}"))
}
