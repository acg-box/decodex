use sha2::{Digest as _, Sha256};

use crate::prelude::{Result, eyre};
use crate::research_design::{
	ResearchDesignOutcome, ResearchDesignRunInput,
	input::{
		ResearchEvidenceInput, ResearchOptionInput, ResearchPrivateEvidenceRefInput,
		ResearchProposedIssueInput, ResearchProvenanceInput, ResearchPublicProjectionRefInput,
		ResearchSubworkInput,
	},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct NormalizedResearchDesignInput {
	pub(super) contract_id: String,
	pub(super) intent: String,
	pub(super) source_issue_identifier: Option<String>,
	pub(super) outcome: ResearchDesignOutcome,
	pub(super) provenance: Vec<ResearchProvenanceInput>,
	pub(super) evidence: Vec<ResearchEvidenceInput>,
	pub(super) options: Vec<ResearchOptionInput>,
	pub(super) ai_subwork: Vec<ResearchSubworkInput>,
	pub(super) objectives: Vec<String>,
	pub(super) non_goals: Vec<String>,
	pub(super) constraints: Vec<String>,
	pub(super) assumptions: Vec<String>,
	pub(super) objections: Vec<String>,
	pub(super) unresolved_decisions: Vec<String>,
	pub(super) evidence_gaps: Vec<String>,
	pub(super) blockers: Vec<String>,
	pub(super) stop_conditions: Vec<String>,
	pub(super) readiness_summary: String,
	pub(super) validation_expectations: Vec<String>,
	pub(super) risk_notes: Vec<String>,
	pub(super) proposed_issues: Vec<ResearchProposedIssueInput>,
	pub(super) promotion_targets: Vec<String>,
	pub(super) conflict_domains: Vec<String>,
	pub(super) private_evidence_refs: Vec<ResearchPrivateEvidenceRefInput>,
	pub(super) public_projection_refs: Vec<ResearchPublicProjectionRefInput>,
	pub(super) public_summary: Option<String>,
}
impl NormalizedResearchDesignInput {
	pub(super) fn new(input: ResearchDesignRunInput) -> Result<Self> {
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

	pub(super) fn validate_outcome(&self) -> Result<()> {
		match self.outcome {
			ResearchDesignOutcome::DecisionReady => self.validate_decision_ready(),
			ResearchDesignOutcome::NotDecisionReady => Ok(()),
			ResearchDesignOutcome::Blocked => self.validate_blocked(),
			ResearchDesignOutcome::NeedsHumanDecision => self.validate_needs_human_decision(),
		}
	}

	pub(super) fn ready_for_issue_shaping(&self) -> bool {
		self.outcome == ResearchDesignOutcome::DecisionReady
	}

	pub(super) fn missing_decisions(&self) -> Vec<String> {
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
}

pub(super) fn default_feedback(outcome: ResearchDesignOutcome) -> &'static str {
	match outcome {
		ResearchDesignOutcome::DecisionReady => {
			"Decision-ready research/design output is stored as a latent contract until promotion."
		},
		ResearchDesignOutcome::NotDecisionReady => {
			"Research/design output is not decision-ready and must not become implementation work."
		},
		ResearchDesignOutcome::Blocked => {
			"Research/design output is blocked; resolve blockers before promotion."
		},
		ResearchDesignOutcome::NeedsHumanDecision => {
			"Research/design output needs an explicit human decision before execution authority exists."
		},
	}
}

pub(super) fn normalize_required_text(name: &str, value: impl Into<String>) -> Result<String> {
	let value = value.into();
	let value = value.trim();

	if value.is_empty() {
		eyre::bail!("{name} must not be empty.");
	}

	Ok(value.to_owned())
}

pub(super) fn normalize_optional_text(name: &str, value: Option<String>) -> Result<Option<String>> {
	value.map(|value| normalize_required_text(name, value)).transpose()
}

pub(super) fn normalize_text_list(name: &str, values: Vec<String>) -> Result<Vec<String>> {
	values.into_iter().map(|value| normalize_required_text(name, value)).collect()
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
