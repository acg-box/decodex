use std::{
	io::{self, IsTerminal as _, Write as _},
	path::PathBuf,
};

use clap::{Args, Subcommand};

use crate::{
	autonomy_runtime_policy,
	config::ServiceConfig,
	prelude::{Result, eyre},
	runtime,
	state::{AutonomyRuntimePolicyReceiptInput, StateStore},
};

#[derive(Debug, Args)]
pub(in crate::cli) struct ProjectCommand {
	#[command(subcommand)]
	pub(in crate::cli) command: ProjectSubcommand,
}
impl ProjectCommand {
	pub(in crate::cli) fn run(&self) -> Result<()> {
		let state_store = runtime::open_runtime_store()?;

		match &self.command {
			ProjectSubcommand::Add(args) => {
				let registration =
					runtime::register_project_config(&state_store, &args.config, true)?;

				if !registration.enabled() {
					state_store.set_project_enabled(registration.service_id(), true)?;
				}

				println!(
					"registered project {} at {}",
					registration.service_id(),
					registration.config_path().display()
				);
			},
			ProjectSubcommand::List => {
				let projects = state_store.list_projects()?;

				if projects.is_empty() {
					println!("No registered projects.");
				} else {
					for project in projects {
						let status = if project.enabled() { "enabled" } else { "disabled" };

						println!(
							"{}\t{}\t{}",
							project.service_id(),
							status,
							project.config_path().display()
						);
					}
				}
			},
			ProjectSubcommand::Enable(args) => {
				state_store.set_project_enabled(&args.service_id, true)?;

				println!("enabled project {}", args.service_id);
			},
			ProjectSubcommand::Disable(args) => {
				state_store.set_project_enabled(&args.service_id, false)?;

				println!("disabled project {}", args.service_id);
			},
			ProjectSubcommand::Remove(args) => {
				let removed = state_store.remove_project(&args.service_id)?;

				println!(
					"removed project {} at {}",
					removed.service_id(),
					removed.config_path().display()
				);
			},
			ProjectSubcommand::AcceptRuntimePolicy(args) => {
				accept_runtime_policy(&state_store, args)?;
			},
		}

		Ok(())
	}
}

#[derive(Debug, Args)]
pub(in crate::cli) struct ProjectAddCommand {
	/// Path to a Decodex project directory containing `project.toml` and `WORKFLOW.md`.
	#[arg(value_name = "PROJECT_DIR")]
	pub(in crate::cli) config: PathBuf,
}

#[derive(Debug, Args)]
pub(in crate::cli) struct ProjectToggleCommand {
	/// Project service id from the registered Decodex config.
	#[arg(value_name = "SERVICE_ID")]
	pub(in crate::cli) service_id: String,
}

#[derive(Debug, Args)]
pub(in crate::cli) struct ProjectAcceptRuntimePolicyCommand {
	/// Registered project service id whose configured runtime policy is being accepted.
	#[arg(value_name = "SERVICE_ID")]
	pub(in crate::cli) service_id: String,
	/// Public-safe non-goal retained in every policy-derived Decision Contract.
	#[arg(long = "public-non-goal", required = true)]
	pub(in crate::cli) public_non_goals: Vec<String>,
}

#[derive(Debug, Subcommand)]
pub(in crate::cli) enum ProjectSubcommand {
	/// Register or refresh one Decodex project config and enable it.
	Add(ProjectAddCommand),
	/// List registered local projects.
	List,
	/// Enable one registered project for `decodex serve`.
	Enable(ProjectToggleCommand),
	/// Disable one registered project for `decodex serve`.
	Disable(ProjectToggleCommand),
	/// Remove one registered project from the local registry.
	Remove(ProjectToggleCommand),
	/// Explicit interactive operator ceremony for one immutable runtime-policy candidate.
	AcceptRuntimePolicy(ProjectAcceptRuntimePolicyCommand),
}

fn accept_runtime_policy(
	state_store: &StateStore,
	args: &ProjectAcceptRuntimePolicyCommand,
) -> Result<()> {
	if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
		eyre::bail!("Runtime-policy acceptance requires an interactive operator terminal.");
	}

	let registration = state_store
		.list_projects()?
		.into_iter()
		.find(|project| project.service_id() == args.service_id)
		.ok_or_else(|| eyre::eyre!("Registered project `{}` was not found.", args.service_id))?;
	let config = ServiceConfig::from_path(registration.config_path())?;
	let principal = autonomy_runtime_policy::resolved_local_principal()?;
	let accepted_at = autonomy_runtime_policy::current_rfc3339()?;
	let candidate = autonomy_runtime_policy::registered_policy_candidate(
		&config,
		state_store,
		&args.service_id,
		&principal,
		&accepted_at,
		"decodex-operator-cli",
		args.public_non_goals.clone(),
	)?;
	let digest = autonomy_runtime_policy::runtime_policy_candidate_digest(&candidate)?;

	println!("Runtime policy: {}@{}", candidate.policy_id(), candidate.policy_version());
	println!("Objective: {}@{}", candidate.objective_id(), candidate.objective_version());
	println!("Principal: {}", candidate.accepted_by());
	println!("Candidate digest: {digest}");

	for non_goal in candidate.public_non_goals() {
		println!("Non-goal: {non_goal}");
	}

	print!("Type ACCEPT {digest} to issue a single-use 10-minute receipt: ");

	io::stdout().flush()?;

	let mut confirmation = String::new();

	io::stdin().read_line(&mut confirmation)?;

	if confirmation.trim() != format!("ACCEPT {digest}") {
		eyre::bail!("Runtime-policy acceptance confirmation did not match the candidate digest.");
	}

	let receipt_id = autonomy_runtime_policy::new_operator_receipt_id()?;

	state_store.issue_autonomy_runtime_policy_receipt(AutonomyRuntimePolicyReceiptInput {
		project_id: &args.service_id,
		receipt_id: &receipt_id,
		principal: &principal,
		candidate_digest: &digest,
		candidate: &candidate,
		created_at: &accepted_at,
		expires_at_unix: autonomy_runtime_policy::operator_receipt_expiry_unix(),
	})?;

	let accepted = state_store.accept_autonomy_runtime_policy_with_receipt(
		&args.service_id,
		&receipt_id,
		&principal,
	)?;

	println!(
		"accepted runtime policy {}@{} for objective {}@{}",
		accepted.policy_id(),
		accepted.policy_version(),
		accepted.objective_id(),
		accepted.objective_version()
	);

	Ok(())
}
