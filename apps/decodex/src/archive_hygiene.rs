//! Linear archive hygiene planning and execution.

mod config;
mod plan;
mod render;

use std::path::Path;

use self::plan::ArchivePlan;
use crate::{
	config::ServiceConfig,
	prelude::{Result, eyre},
	runtime,
	tracker::linear::LinearClient,
	workflow::WorkflowDocument,
};

pub(crate) struct ArchiveHygieneRequest {
	pub(crate) repo_labels: Vec<String>,
	pub(crate) older_than_days: u32,
	pub(crate) execute: bool,
}

pub(crate) fn run(config_path: Option<&Path>, request: &ArchiveHygieneRequest) -> Result<()> {
	let state_store = runtime::open_runtime_store()?;
	let Some(config_path) = self::config::resolve_config_path(config_path, &state_store)? else {
		eyre::bail!(
			"No Decodex project config found. Pass this command's --config <PROJECT_DIR> or register one with `decodex project add <PROJECT_DIR>`."
		);
	};
	let config = ServiceConfig::from_path(&config_path)?;
	let workflow = WorkflowDocument::from_path(config.workflow_path())?;
	let tracker = LinearClient::new(config.tracker().resolve_api_key()?)?;
	let repo_labels = self::config::normalize_repo_labels(&request.repo_labels)?;
	let updated_before = self::config::updated_before_timestamp(request.older_than_days)?;
	let plan = self::plan::build_archive_plan(
		&tracker,
		&config,
		&workflow,
		&repo_labels,
		&updated_before,
	)?;

	self::render::print_archive_plan(&plan, &repo_labels, &updated_before, request.execute);

	if request.execute {
		for candidate in &plan.candidates {
			tracker.archive_issue(&candidate.id)?;
		}

		println!("Archived {} Linear issue(s).", plan.candidates.len());
	}

	Ok(())
}

#[cfg(test)]
mod tests;
