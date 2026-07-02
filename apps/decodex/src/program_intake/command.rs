use crate::{
	config::ServiceConfig,
	prelude::{Result, eyre},
	program_intake::{
		self, GoalIntakeCommandRequest, GoalIntakeReport, GoalIntakeRunRequest,
		IssueBatchIntakeCommandRequest, IssueBatchIntakeReport,
	},
	runtime,
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

	program_intake::register_intake_project_config_for_persist(
		&state_store,
		&config_path,
		request.apply,
	)?;

	let tracker = LinearClient::new(config.tracker().resolve_api_key()?)?;

	program_intake::run_goal_intake(GoalIntakeRunRequest {
		state_store: &state_store,
		tracker: &tracker,
		config: &config,
		workflow: &workflow,
		contract_id: request.contract_id,
		team_issue_identifier: request.team_issue_identifier.map(str::to_owned),
		dry_run: request.dry_run,
		apply: request.apply,
	})
}
