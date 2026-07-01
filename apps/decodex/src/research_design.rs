//! Decodex-native research/design runner and Decision Contract compiler.

pub(crate) mod compiler;
pub(crate) mod input;
pub(crate) mod lifecycle;
pub(crate) mod normalized;
pub(crate) mod reports;
pub(crate) mod requests;

pub(crate) use self::{
	compiler::dry_run_research_design_compile,
	input::{ResearchDesignOutcome, ResearchDesignRunInput},
	lifecycle::{persist_research_design_run, promote_research_design_contract},
	reports::{ResearchDesignPromotionReport, ResearchDesignRunReport},
	requests::{ResearchDesignCompileRequest, ResearchDesignPromoteRequest},
};

use std::{
	env,
	path::{Path, PathBuf},
};

use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
	config::ServiceConfig,
	loop_contract::{DecisionPromotion, DecisionPromotionActorKind},
	prelude::{Result, eyre},
	runtime,
	state::StateStore,
};
#[cfg(test)]
use self::{
	compiler::compile_research_design_run,
	input::{
		ResearchEvidenceInput, ResearchOptionInput, ResearchPrivateEvidenceRefInput,
		ResearchProposedIssueInput, ResearchProvenanceInput, ResearchPublicProjectionRefInput,
		ResearchSubworkInput,
	},
	lifecycle::ensure_contract_authorizes_execution,
};

/// Compile and persist a research/design result into the local runtime store.
pub(crate) fn run_compile(
	request: ResearchDesignCompileRequest<'_>,
) -> Result<ResearchDesignRunReport> {
	let state_store = runtime::open_runtime_store()?;
	let config_path = resolve_research_project_config_path(request.config_path, &state_store)?;
	let config = ServiceConfig::from_path(&config_path)?;

	runtime::register_project_config(&state_store, &config_path, true)?;

	lifecycle::persist_research_design_run(&state_store, config.service_id(), request.input)
}

/// Promote an already persisted contract into accepted execution authority.
pub(crate) fn run_promote(
	request: ResearchDesignPromoteRequest<'_>,
) -> Result<ResearchDesignPromotionReport> {
	let state_store = runtime::open_runtime_store()?;
	let config_path = resolve_research_project_config_path(request.config_path, &state_store)?;
	let config = ServiceConfig::from_path(&config_path)?;

	runtime::register_project_config(&state_store, &config_path, true)?;

	let accepted_at = match request.accepted_at {
		Some(accepted_at) => accepted_at.to_owned(),
		None => OffsetDateTime::now_utc().format(&Rfc3339)?,
	};
	let promotion = DecisionPromotion::new(
		request.accepted_by,
		DecisionPromotionActorKind::User,
		accepted_at,
		request.acceptance_source,
		request.promotion_reason,
	)?;
	let record = lifecycle::promote_research_design_contract(
		&state_store,
		config.service_id(),
		request.contract_id,
		promotion,
	)?;

	Ok(ResearchDesignPromotionReport {
		contract_id: record.contract_id().to_owned(),
		contract_status: record.status(),
		execution_authority_granted: true,
		ready_for_issue_shaping: record.contract().execution_readiness().ready_for_issue_shaping(),
	})
}

fn resolve_research_project_config_path(
	config_path: Option<&Path>,
	state_store: &StateStore,
) -> Result<PathBuf> {
	if let Some(config_path) = config_path {
		return ServiceConfig::resolve_project_config_path(config_path);
	}

	let cwd = env::current_dir()?;

	runtime::registered_config_path_for_cwd(state_store, &cwd)?.ok_or_else(|| {
		eyre::eyre!(
			"No Decodex project config found. Pass this command's --config <PROJECT_DIR> or register one with `decodex project add <PROJECT_DIR>`."
		)
	})
}

#[cfg(test)]
mod tests;
