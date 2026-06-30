//! Decodex-native research/design runner and Decision Contract compiler.

use std::{
	env,
	path::{Path, PathBuf},
};

use serde::Serialize;
use serde_json::{self, Value};
use sha2::{Digest as _, Sha256};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
	config::ServiceConfig,
	loop_contract::{
		DecisionContract, DecisionContractStatus, DecisionPromotion, DecisionPromotionActorKind,
		DecisionProposedIssue,
	},
	prelude::{Result, eyre},
	runtime,
	state::{DecisionContractRecord, StateStore},
};

mod input;

pub(crate) use input::{ResearchDesignOutcome, ResearchDesignRunInput};
use input::{
	ResearchEvidenceInput, ResearchOptionInput, ResearchPrivateEvidenceRefInput,
	ResearchProposedIssueInput, ResearchProvenanceInput, ResearchPublicProjectionRefInput,
	ResearchSubworkInput,
};

/// CLI/runtime request for compiling and persisting one research/design contract.
pub(crate) struct ResearchDesignCompileRequest<'a> {
	pub(crate) config_path: Option<&'a Path>,
	pub(crate) input: ResearchDesignRunInput,
}

/// CLI/runtime request for promoting an already persisted research/design contract.
pub(crate) struct ResearchDesignPromoteRequest<'a> {
	pub(crate) config_path: Option<&'a Path>,
	pub(crate) contract_id: &'a str,
	pub(crate) accepted_by: &'a str,
	pub(crate) accepted_at: Option<&'a str>,
	pub(crate) acceptance_source: &'a str,
	pub(crate) promotion_reason: Option<String>,
}

/// Compiler report for one persisted research/design run.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct ResearchDesignRunReport {
	pub(crate) outcome: ResearchDesignOutcome,
	pub(crate) contract_id: String,
	pub(crate) contract_status: DecisionContractStatus,
	pub(crate) source_issue_id: Option<String>,
	pub(crate) ready_for_issue_shaping: bool,
	pub(crate) issue_generation_ready_after_promotion: bool,
	pub(crate) execution_authority_granted: bool,
	pub(crate) feedback: String,
	pub(crate) missing_decisions: Vec<String>,
	pub(crate) blockers: Vec<String>,
	pub(crate) proposed_issues: Vec<DecisionProposedIssue>,
	pub(crate) promotion_targets: Vec<String>,
	pub(crate) conflict_domains: Vec<String>,
	pub(crate) private_evidence_ref_count: usize,
	pub(crate) public_projection_ref_count: usize,
}
impl ResearchDesignRunReport {
	fn from_compilation(
		input: &NormalizedResearchDesignInput,
		contract: &DecisionContract,
	) -> Self {
		Self {
			outcome: input.outcome,
			contract_id: contract.contract_id().to_owned(),
			contract_status: contract.status(),
			source_issue_id: input.source_issue_identifier.clone(),
			ready_for_issue_shaping: contract.execution_readiness().ready_for_issue_shaping(),
			issue_generation_ready_after_promotion: input.ready_for_issue_shaping(),
			execution_authority_granted: false,
			feedback: default_feedback(input.outcome).to_owned(),
			missing_decisions: input.missing_decisions(),
			blockers: input.blockers.clone(),
			proposed_issues: contract.execution_readiness().proposed_issues().to_vec(),
			promotion_targets: contract.execution_readiness().promotion_targets().to_vec(),
			conflict_domains: contract.execution_readiness().conflict_domains().to_vec(),
			private_evidence_ref_count: input.private_evidence_refs.len(),
			public_projection_ref_count: input.public_projection_refs.len(),
		}
	}

	fn with_record(mut self, record: &DecisionContractRecord) -> Self {
		self.contract_id = record.contract_id().to_owned();
		self.contract_status = record.status();
		self.ready_for_issue_shaping =
			record.contract().execution_readiness().ready_for_issue_shaping();

		self
	}
}

/// Promotion report for an accepted research/design contract.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct ResearchDesignPromotionReport {
	pub(crate) contract_id: String,
	pub(crate) contract_status: DecisionContractStatus,
	pub(crate) execution_authority_granted: bool,
	pub(crate) ready_for_issue_shaping: bool,
}

struct ResearchDesignCompilation {
	contract: DecisionContract,
	report: ResearchDesignRunReport,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct NormalizedResearchDesignInput {
	contract_id: String,
	intent: String,
	source_issue_identifier: Option<String>,
	outcome: ResearchDesignOutcome,
	provenance: Vec<ResearchProvenanceInput>,
	evidence: Vec<ResearchEvidenceInput>,
	options: Vec<ResearchOptionInput>,
	ai_subwork: Vec<ResearchSubworkInput>,
	objectives: Vec<String>,
	non_goals: Vec<String>,
	constraints: Vec<String>,
	assumptions: Vec<String>,
	objections: Vec<String>,
	unresolved_decisions: Vec<String>,
	evidence_gaps: Vec<String>,
	blockers: Vec<String>,
	stop_conditions: Vec<String>,
	readiness_summary: String,
	validation_expectations: Vec<String>,
	risk_notes: Vec<String>,
	proposed_issues: Vec<ResearchProposedIssueInput>,
	promotion_targets: Vec<String>,
	conflict_domains: Vec<String>,
	private_evidence_refs: Vec<ResearchPrivateEvidenceRefInput>,
	public_projection_refs: Vec<ResearchPublicProjectionRefInput>,
	public_summary: Option<String>,
}
impl NormalizedResearchDesignInput {
	fn new(input: ResearchDesignRunInput) -> Result<Self> {
		let contract_id = match input.contract_id.clone() {
			Some(contract_id) => normalize_required_text("contract_id", contract_id)?,
			None => generated_contract_id(&input)?,
		};

		Ok(Self {
			contract_id,
			intent: normalize_required_text("intent", input.intent)?,
			source_issue_identifier: normalize_optional_text(
				"source_issue_identifier",
				input.source_issue_identifier,
			)?,
			outcome: input.outcome,
			provenance: input
				.provenance
				.into_iter()
				.map(ResearchProvenanceInput::normalized)
				.collect::<Result<Vec<_>>>()?,
			evidence: input
				.evidence
				.into_iter()
				.map(ResearchEvidenceInput::normalized)
				.collect::<Result<Vec<_>>>()?,
			options: input
				.options
				.into_iter()
				.map(ResearchOptionInput::normalized)
				.collect::<Result<Vec<_>>>()?,
			ai_subwork: input
				.ai_subwork
				.into_iter()
				.map(ResearchSubworkInput::normalized)
				.collect::<Result<Vec<_>>>()?,
			objectives: normalize_text_list("objectives", input.objectives)?,
			non_goals: normalize_text_list("non_goals", input.non_goals)?,
			constraints: normalize_text_list("constraints", input.constraints)?,
			assumptions: normalize_text_list("assumptions", input.assumptions)?,
			objections: normalize_text_list("objections", input.objections)?,
			unresolved_decisions: normalize_text_list(
				"unresolved_decisions",
				input.unresolved_decisions,
			)?,
			evidence_gaps: normalize_text_list("evidence_gaps", input.evidence_gaps)?,
			blockers: normalize_text_list("blockers", input.blockers)?,
			stop_conditions: normalize_text_list("stop_conditions", input.stop_conditions)?,
			readiness_summary: normalize_optional_text(
				"readiness_summary",
				input.readiness_summary,
			)?
			.unwrap_or_else(|| default_feedback(input.outcome).to_owned()),
			validation_expectations: normalize_text_list(
				"validation_expectations",
				input.validation_expectations,
			)?,
			risk_notes: normalize_text_list("risk_notes", input.risk_notes)?,
			proposed_issues: input
				.proposed_issues
				.into_iter()
				.map(ResearchProposedIssueInput::normalized)
				.collect::<Result<Vec<_>>>()?,
			promotion_targets: normalize_text_list("promotion_targets", input.promotion_targets)?,
			conflict_domains: normalize_text_list("conflict_domains", input.conflict_domains)?,
			private_evidence_refs: input
				.private_evidence_refs
				.into_iter()
				.map(ResearchPrivateEvidenceRefInput::normalized)
				.collect::<Result<Vec<_>>>()?,
			public_projection_refs: input
				.public_projection_refs
				.into_iter()
				.map(ResearchPublicProjectionRefInput::normalized)
				.collect::<Result<Vec<_>>>()?,
			public_summary: normalize_optional_text("public_summary", input.public_summary)?,
		})
	}

	fn validate_outcome(&self) -> Result<()> {
		match self.outcome {
			ResearchDesignOutcome::DecisionReady => self.validate_decision_ready(),
			ResearchDesignOutcome::NotDecisionReady => Ok(()),
			ResearchDesignOutcome::Blocked => self.validate_blocked(),
			ResearchDesignOutcome::NeedsHumanDecision => self.validate_needs_human_decision(),
		}
	}

	fn validate_decision_ready(&self) -> Result<()> {
		if self.objectives.is_empty() {
			eyre::bail!("decision-ready research requires at least one accepted objective.");
		}
		if self.evidence.is_empty() {
			eyre::bail!("decision-ready research requires at least one evidence claim.");
		}
		if self.evidence.iter().any(|evidence| evidence.kind == "unspecified") {
			eyre::bail!("decision-ready research requires an evidence kind for each claim.");
		}
		if self.options.is_empty() {
			eyre::bail!("decision-ready research requires at least one option comparison.");
		}
		if self.objections.is_empty() {
			eyre::bail!(
				"decision-ready research requires at least one recorded challenge objection or objection note."
			);
		}
		if self.validation_expectations.is_empty() {
			eyre::bail!("decision-ready research requires validation expectations.");
		}
		if self.proposed_issues.is_empty() {
			eyre::bail!(
				"decision-ready research requires at least one structured proposed issue for downstream shaping."
			);
		}
		if self.promotion_targets.is_empty() {
			eyre::bail!("decision-ready research requires at least one promotion target.");
		}
		if !self.unresolved_decisions.is_empty() || !self.evidence_gaps.is_empty() {
			eyre::bail!(
				"decision-ready research cannot carry unresolved decisions or evidence gaps."
			);
		}
		if !self.blockers.is_empty() {
			eyre::bail!("decision-ready research cannot carry unresolved blockers.");
		}

		Ok(())
	}

	fn validate_blocked(&self) -> Result<()> {
		if self.blockers.is_empty() {
			eyre::bail!("blocked research requires at least one blocker.");
		}

		Ok(())
	}

	fn validate_needs_human_decision(&self) -> Result<()> {
		if self.unresolved_decisions.is_empty() {
			eyre::bail!("needs-human-decision research requires an unresolved decision.");
		}

		Ok(())
	}

	fn ready_for_issue_shaping(&self) -> bool {
		self.outcome == ResearchDesignOutcome::DecisionReady
	}

	fn missing_decisions(&self) -> Vec<String> {
		let mut missing = Vec::new();

		missing.extend(self.unresolved_decisions.clone());
		missing.extend(self.evidence_gaps.iter().map(|gap| format!("Evidence gap: {gap}")));

		if self.outcome == ResearchDesignOutcome::NotDecisionReady && missing.is_empty() {
			missing.push(String::from(
				"Research is not decision-ready; gather more evidence or narrow the decision.",
			));
		}

		missing
	}
}

/// Compile and persist a research/design result into the local runtime store.
pub(crate) fn run_compile(
	request: ResearchDesignCompileRequest<'_>,
) -> Result<ResearchDesignRunReport> {
	let state_store = runtime::open_runtime_store()?;
	let config_path = resolve_research_project_config_path(request.config_path, &state_store)?;
	let config = ServiceConfig::from_path(&config_path)?;

	runtime::register_project_config(&state_store, &config_path, true)?;

	persist_research_design_run(&state_store, config.service_id(), request.input)
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
	let record = promote_research_design_contract(
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

pub(crate) fn dry_run_research_design_compile(
	input: ResearchDesignRunInput,
	project_id: &str,
) -> Result<ResearchDesignRunReport> {
	Ok(compile_research_design_run(input, project_id)?.report)
}

pub(crate) fn persist_research_design_run(
	store: &StateStore,
	project_id: &str,
	input: ResearchDesignRunInput,
) -> Result<ResearchDesignRunReport> {
	let source_issue_id = input.source_issue_identifier().map(str::to_owned);
	let compilation = compile_research_design_run(input, project_id)?;
	let record = store.upsert_decision_contract(
		project_id,
		source_issue_id.as_deref(),
		compilation.contract,
	)?;

	Ok(ResearchDesignRunReport { source_issue_id, ..compilation.report.with_record(&record) })
}

pub(crate) fn promote_research_design_contract(
	store: &StateStore,
	project_id: &str,
	contract_id: &str,
	promotion: DecisionPromotion,
) -> Result<DecisionContractRecord> {
	let record = store.promote_decision_contract(project_id, contract_id, promotion)?;

	ensure_contract_authorizes_execution(&record)?;

	Ok(record)
}

#[allow(dead_code)]
fn ensure_contract_authorizes_execution(record: &DecisionContractRecord) -> Result<()> {
	if record.status() != DecisionContractStatus::AcceptedPromoted {
		eyre::bail!(
			"Research/design contract `{}` is not accepted; refusing to create execution work from unaccepted research.",
			record.contract_id()
		);
	}
	if !record.contract().execution_readiness().ready_for_issue_shaping() {
		eyre::bail!(
			"Accepted research/design contract `{}` is not ready for issue shaping.",
			record.contract_id()
		);
	}
	if !record.contract().execution_readiness().missing_decisions().is_empty() {
		eyre::bail!(
			"Accepted research/design contract `{}` still has unresolved decisions.",
			record.contract_id()
		);
	}
	if record.contract().execution_readiness().proposed_issues().is_empty() {
		eyre::bail!(
			"Accepted research/design contract `{}` has no structured proposed issues.",
			record.contract_id()
		);
	}

	Ok(())
}

fn compile_research_design_run(
	input: ResearchDesignRunInput,
	project_id: &str,
) -> Result<ResearchDesignCompilation> {
	let normalized = NormalizedResearchDesignInput::new(input)?;

	normalized.validate_outcome()?;

	let contract = build_decision_contract(&normalized, project_id)?;
	let report = ResearchDesignRunReport::from_compilation(&normalized, &contract);

	Ok(ResearchDesignCompilation { contract, report })
}

fn build_decision_contract(
	input: &NormalizedResearchDesignInput,
	project_id: &str,
) -> Result<DecisionContract> {
	let payload = serde_json::json!({
		"schema": crate::loop_contract::DECISION_CONTRACT_SCHEMA,
		"record_version": crate::loop_contract::DECISION_CONTRACT_RECORD_VERSION,
		"contract_id": input.contract_id,
		"status": input.outcome.contract_status(),
		"source_intent": {
			"summary": input.intent,
			"user_utterance": input.intent,
			"source_issue_identifier": input.source_issue_identifier,
		},
		"research_provenance": research_provenance_json(input),
		"research_evidence": research_evidence_json(input),
		"research_options": research_options_json(input),
		"accepted_authority": {
			"accepted_objectives": input.objectives,
			"non_goals": input.non_goals,
			"constraints": input.constraints,
			"assumptions": input.assumptions,
			"objections": input.objections,
			"stop_conditions": stop_conditions(input),
		},
		"execution_readiness": {
			"summary": input.readiness_summary,
			"ready_for_issue_shaping": input.ready_for_issue_shaping(),
			"missing_decisions": input.missing_decisions(),
			"validation_expectations": input.validation_expectations,
			"risk_notes": risk_notes(input),
			"proposed_issues": input.proposed_issues,
			"promotion_targets": input.promotion_targets,
			"conflict_domains": input.conflict_domains,
		},
		"links": {
			"generated_issue_ids": [],
			"generated_issue_identifiers": [],
			"execution_program_node_ids": [],
		},
		"evidence_boundary": {
			"private_evidence_refs": private_evidence_refs_json(input, project_id),
			"public_projection_refs": public_projection_refs_json(input),
			"public_summary": input.public_summary,
		},
	});
	let contract = serde_json::from_value::<DecisionContract>(payload)?;

	contract.validate()?;

	Ok(contract)
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

fn research_provenance_json(input: &NormalizedResearchDesignInput) -> Vec<Value> {
	let mut provenance = input
		.provenance
		.iter()
		.map(|item| {
			serde_json::json!({
				"kind": item.kind,
				"reference": item.reference,
				"summary": item.summary,
			})
		})
		.collect::<Vec<_>>();

	for subwork in &input.ai_subwork {
		provenance.push(serde_json::json!({
			"kind": format!("ai_subwork_{}", subwork.worker_kind),
			"reference": subwork.objective,
			"summary": subwork.summary(),
		}));
	}

	provenance
}

fn research_evidence_json(input: &NormalizedResearchDesignInput) -> Vec<Value> {
	input
		.evidence
		.iter()
		.map(|item| {
			serde_json::json!({
				"kind": item.kind,
				"claim": item.claim,
				"support": item.support,
				"source_ref": item.source_ref,
			})
		})
		.collect()
}

fn research_options_json(input: &NormalizedResearchDesignInput) -> Vec<Value> {
	input
		.options
		.iter()
		.map(|item| {
			serde_json::json!({
				"option": item.option,
				"tradeoffs": item.tradeoffs,
				"decision": item.decision,
				"rejected_reason": item.rejected_reason,
			})
		})
		.collect()
}

fn private_evidence_refs_json(
	input: &NormalizedResearchDesignInput,
	project_id: &str,
) -> Vec<Value> {
	input
		.private_evidence_refs
		.iter()
		.map(|item| {
			serde_json::json!({
				"project_id": item.project_id.as_deref().unwrap_or(project_id),
				"issue_id": item.issue_id,
				"run_id": item.run_id,
				"attempt_number": item.attempt_number,
				"record_id": item.record_id,
				"event_type": item.event_type,
			})
		})
		.collect()
}

fn public_projection_refs_json(input: &NormalizedResearchDesignInput) -> Vec<Value> {
	input
		.public_projection_refs
		.iter()
		.map(|item| {
			serde_json::json!({
				"surface": item.surface,
				"reference": item.reference,
				"summary": item.summary,
			})
		})
		.collect()
}

fn stop_conditions(input: &NormalizedResearchDesignInput) -> Vec<String> {
	let mut stop_conditions = input.stop_conditions.clone();

	for blocker in &input.blockers {
		stop_conditions.push(format!("Stop research promotion until blocker resolves: {blocker}"));
	}

	stop_conditions
}

fn risk_notes(input: &NormalizedResearchDesignInput) -> Vec<String> {
	let mut risk_notes = input.risk_notes.clone();

	if input.outcome == ResearchDesignOutcome::NotDecisionReady {
		risk_notes.push(String::from(
			"Research is not decision-ready and must not become implementation work.",
		));
	}

	for blocker in &input.blockers {
		risk_notes.push(format!("Research/design blocker: {blocker}"));
	}

	risk_notes
}

fn default_feedback(outcome: ResearchDesignOutcome) -> &'static str {
	match outcome {
		ResearchDesignOutcome::DecisionReady =>
			"Decision-ready research/design output is stored as a latent contract until promotion.",
		ResearchDesignOutcome::NotDecisionReady =>
			"Research/design output is not decision-ready and must not become implementation work.",
		ResearchDesignOutcome::Blocked =>
			"Research/design output is blocked; resolve blockers before promotion.",
		ResearchDesignOutcome::NeedsHumanDecision =>
			"Research/design output needs an explicit human decision before execution authority exists.",
	}
}

fn generated_contract_id(input: &ResearchDesignRunInput) -> Result<String> {
	let slug = intent_slug(&input.intent);
	let encoded = serde_json::to_vec(input)?;
	let digest = Sha256::digest(&encoded);
	let mut hash = String::with_capacity(12);

	for byte in digest.iter().take(6) {
		hash.push(char::from(b"0123456789abcdef"[(byte >> 4) as usize]));
		hash.push(char::from(b"0123456789abcdef"[(byte & 0x0f) as usize]));
	}

	Ok(format!("research-design-{slug}-{hash}"))
}

fn intent_slug(intent: &str) -> String {
	let mut slug = String::new();
	let mut previous_dash = false;

	for character in intent.chars() {
		if character.is_ascii_alphanumeric() {
			slug.push(character.to_ascii_lowercase());

			previous_dash = false;
		} else if !previous_dash && !slug.is_empty() {
			slug.push('-');

			previous_dash = true;
		}
		if slug.len() >= 40 {
			break;
		}
	}

	while slug.ends_with('-') {
		slug.pop();
	}

	if slug.is_empty() { String::from("research") } else { slug }
}

fn normalize_required_text(name: &str, value: impl Into<String>) -> Result<String> {
	let value = value.into();
	let value = value.trim();

	if value.is_empty() {
		eyre::bail!("{name} must not be empty.");
	}

	Ok(value.to_owned())
}

fn normalize_optional_text(name: &str, value: Option<String>) -> Result<Option<String>> {
	value.map(|value| normalize_required_text(name, value)).transpose()
}

fn normalize_text_list(name: &str, values: Vec<String>) -> Result<Vec<String>> {
	values.into_iter().map(|value| normalize_required_text(name, value)).collect()
}

#[cfg(test)] mod tests;
