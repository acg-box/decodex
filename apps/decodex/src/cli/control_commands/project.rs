use std::path::PathBuf;

use clap::{Args, Subcommand};

use crate::{prelude::Result, runtime};

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
}
