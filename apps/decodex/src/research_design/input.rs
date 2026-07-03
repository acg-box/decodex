use serde::{Deserialize, Serialize};

use crate::{
	loop_contract::DecisionContractStatus,
	prelude::{Result, eyre},
	research_design::normalized,
};

/// Research/design outcome before any execution authority exists.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ResearchDesignOutcome {
	DecisionReady,
	NotDecisionReady,
	Blocked,
	NeedsHumanDecision,
}
impl ResearchDesignOutcome {
	pub(super) fn contract_status(self) -> DecisionContractStatus {
		match self {
			Self::DecisionReady | Self::NotDecisionReady | Self::Blocked =>
				DecisionContractStatus::DraftLatent,
			Self::NeedsHumanDecision => DecisionContractStatus::NeedsHumanDecision,
		}
	}
}

/// Structured bounded research/design input compiled into a latent Decision Contract.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ResearchDesignRunInput {
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) contract_id: Option<String>,
	pub(super) intent: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) source_issue_identifier: Option<String>,
	pub(super) outcome: ResearchDesignOutcome,
	#[serde(default)]
	pub(super) provenance: Vec<ResearchProvenanceInput>,
	#[serde(default)]
	pub(super) evidence: Vec<ResearchEvidenceInput>,
	#[serde(default)]
	pub(super) options: Vec<ResearchOptionInput>,
	#[serde(default)]
	pub(super) ai_subwork: Vec<ResearchSubworkInput>,
	#[serde(default)]
	pub(super) objectives: Vec<String>,
	#[serde(default)]
	pub(super) non_goals: Vec<String>,
	#[serde(default)]
	pub(super) constraints: Vec<String>,
	#[serde(default)]
	pub(super) assumptions: Vec<String>,
	#[serde(default)]
	pub(super) objections: Vec<String>,
	#[serde(default)]
	pub(super) unresolved_decisions: Vec<String>,
	#[serde(default)]
	pub(super) evidence_gaps: Vec<String>,
	#[serde(default)]
	pub(super) blockers: Vec<String>,
	#[serde(default)]
	pub(super) stop_conditions: Vec<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) readiness_summary: Option<String>,
	#[serde(default)]
	pub(super) validation_expectations: Vec<String>,
	#[serde(default)]
	pub(super) risk_notes: Vec<String>,
	#[serde(default)]
	pub(super) proposed_issues: Vec<ResearchProposedIssueInput>,
	#[serde(default)]
	pub(super) promotion_targets: Vec<String>,
	#[serde(default)]
	pub(super) conflict_domains: Vec<String>,
	#[serde(default)]
	pub(super) private_evidence_refs: Vec<ResearchPrivateEvidenceRefInput>,
	#[serde(default)]
	pub(super) public_projection_refs: Vec<ResearchPublicProjectionRefInput>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) public_summary: Option<String>,
}
impl ResearchDesignRunInput {
	pub(crate) fn from_intent(
		intent: impl Into<String>,
		source_issue_identifier: Option<String>,
		outcome: ResearchDesignOutcome,
	) -> Self {
		Self {
			contract_id: None,
			intent: intent.into(),
			source_issue_identifier,
			outcome,
			provenance: Vec::new(),
			evidence: Vec::new(),
			options: Vec::new(),
			ai_subwork: Vec::new(),
			objectives: Vec::new(),
			non_goals: Vec::new(),
			constraints: Vec::new(),
			assumptions: Vec::new(),
			objections: Vec::new(),
			unresolved_decisions: Vec::new(),
			evidence_gaps: Vec::new(),
			blockers: Vec::new(),
			stop_conditions: Vec::new(),
			readiness_summary: None,
			validation_expectations: Vec::new(),
			risk_notes: Vec::new(),
			proposed_issues: Vec::new(),
			promotion_targets: Vec::new(),
			conflict_domains: Vec::new(),
			private_evidence_refs: Vec::new(),
			public_projection_refs: Vec::new(),
			public_summary: None,
		}
	}

	pub(crate) fn source_issue_identifier(&self) -> Option<&str> {
		self.source_issue_identifier.as_deref()
	}
}

/// Research source that contributed to a compiler run.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ResearchProvenanceInput {
	pub(super) kind: String,
	pub(super) reference: String,
	pub(super) summary: String,
}
impl ResearchProvenanceInput {
	pub(super) fn normalized(self) -> Result<Self> {
		Ok(Self {
			kind: normalized::normalize_required_text("provenance.kind", self.kind)?,
			reference: normalized::normalize_required_text("provenance.reference", self.reference)?,
			summary: normalized::normalize_required_text("provenance.summary", self.summary)?,
		})
	}
}

/// Evidence claim retained as research context, not execution authority.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ResearchEvidenceInput {
	#[serde(default = "default_evidence_kind")]
	pub(super) kind: String,
	pub(super) claim: String,
	pub(super) support: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) source_ref: Option<String>,
}
impl ResearchEvidenceInput {
	pub(super) fn normalized(self) -> Result<Self> {
		Ok(Self {
			kind: normalized::normalize_required_text("evidence.kind", self.kind)?,
			claim: normalized::normalize_required_text("evidence.claim", self.claim)?,
			support: normalized::normalize_required_text("evidence.support", self.support)?,
			source_ref: normalized::normalize_optional_text(
				"evidence.source_ref",
				self.source_ref,
			)?,
		})
	}
}

/// Candidate option considered during bounded research/design.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ResearchOptionInput {
	pub(super) option: String,
	#[serde(default)]
	pub(super) tradeoffs: Vec<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) decision: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) rejected_reason: Option<String>,
}
impl ResearchOptionInput {
	pub(super) fn normalized(self) -> Result<Self> {
		Ok(Self {
			option: normalized::normalize_required_text("options.option", self.option)?,
			tradeoffs: normalized::normalize_text_list("options.tradeoffs", self.tradeoffs)?,
			decision: normalized::normalize_optional_text("options.decision", self.decision)?,
			rejected_reason: normalized::normalize_optional_text(
				"options.rejected_reason",
				self.rejected_reason,
			)?,
		})
	}
}

/// AI-owned subwork folded back into the main coherent contract.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ResearchSubworkInput {
	pub(super) worker_kind: String,
	pub(super) objective: String,
	pub(super) outcome: String,
	#[serde(default)]
	pub(super) evidence_refs: Vec<String>,
}
impl ResearchSubworkInput {
	pub(super) fn normalized(self) -> Result<Self> {
		Ok(Self {
			worker_kind: normalized::normalize_required_text(
				"ai_subwork.worker_kind",
				self.worker_kind,
			)?,
			objective: normalized::normalize_required_text("ai_subwork.objective", self.objective)?,
			outcome: normalized::normalize_required_text("ai_subwork.outcome", self.outcome)?,
			evidence_refs: normalized::normalize_text_list(
				"ai_subwork.evidence_refs",
				self.evidence_refs,
			)?,
		})
	}

	pub(super) fn summary(&self) -> String {
		if self.evidence_refs.is_empty() {
			self.outcome.clone()
		} else {
			format!("{} Evidence refs: {}.", self.outcome, self.evidence_refs.join(", "))
		}
	}
}

/// Structured issue-shaping input emitted into Decision Contract readiness.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ResearchProposedIssueInput {
	pub(super) key: String,
	pub(super) title: String,
	pub(super) objective: String,
	pub(super) stage: String,
	pub(super) dependencies: Vec<String>,
	pub(super) conflict_domains: Vec<String>,
	pub(super) acceptance: Vec<String>,
	pub(super) validation: Vec<String>,
	pub(super) risk: Vec<String>,
	pub(super) queue_intent: String,
}
impl ResearchProposedIssueInput {
	pub(super) fn normalized(self) -> Result<Self> {
		let issue = Self {
			key: normalized::normalize_required_text("proposed_issues.key", self.key)?,
			title: normalized::normalize_required_text("proposed_issues.title", self.title)?,
			objective: normalized::normalize_required_text(
				"proposed_issues.objective",
				self.objective,
			)?,
			stage: normalized::normalize_required_text("proposed_issues.stage", self.stage)?,
			dependencies: normalized::normalize_text_list(
				"proposed_issues.dependencies",
				self.dependencies,
			)?,
			conflict_domains: normalized::normalize_text_list(
				"proposed_issues.conflict_domains",
				self.conflict_domains,
			)?,
			acceptance: normalized::normalize_text_list(
				"proposed_issues.acceptance",
				self.acceptance,
			)?,
			validation: normalized::normalize_text_list(
				"proposed_issues.validation",
				self.validation,
			)?,
			risk: normalized::normalize_text_list("proposed_issues.risk", self.risk)?,
			queue_intent: normalized::normalize_required_text(
				"proposed_issues.queue_intent",
				self.queue_intent,
			)?,
		};

		if issue.acceptance.is_empty() {
			eyre::bail!("proposed_issues.acceptance must include at least one item.");
		}
		if issue.validation.is_empty() {
			eyre::bail!("proposed_issues.validation must include at least one item.");
		}

		Ok(issue)
	}
}

/// Runtime-private evidence pointer retained inside the Decision Contract boundary.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ResearchPrivateEvidenceRefInput {
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) project_id: Option<String>,
	pub(super) issue_id: String,
	pub(super) run_id: String,
	pub(super) attempt_number: i64,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) record_id: Option<i64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) event_type: Option<String>,
}
impl ResearchPrivateEvidenceRefInput {
	pub(super) fn normalized(self) -> Result<Self> {
		Ok(Self {
			project_id: normalized::normalize_optional_text(
				"private_evidence_refs.project_id",
				self.project_id,
			)?,
			issue_id: normalized::normalize_required_text(
				"private_evidence_refs.issue_id",
				self.issue_id,
			)?,
			run_id: normalized::normalize_required_text(
				"private_evidence_refs.run_id",
				self.run_id,
			)?,
			attempt_number: self.attempt_number,
			record_id: self.record_id,
			event_type: normalized::normalize_optional_text(
				"private_evidence_refs.event_type",
				self.event_type,
			)?,
		})
	}
}

/// Sparse public projection pointer, such as an issue or summary record.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ResearchPublicProjectionRefInput {
	pub(super) surface: String,
	pub(super) reference: String,
	pub(super) summary: String,
}
impl ResearchPublicProjectionRefInput {
	pub(super) fn normalized(self) -> Result<Self> {
		Ok(Self {
			surface: normalized::normalize_required_text(
				"public_projection_refs.surface",
				self.surface,
			)?,
			reference: normalized::normalize_required_text(
				"public_projection_refs.reference",
				self.reference,
			)?,
			summary: normalized::normalize_required_text(
				"public_projection_refs.summary",
				self.summary,
			)?,
		})
	}
}

fn default_evidence_kind() -> String {
	String::from("unspecified")
}
