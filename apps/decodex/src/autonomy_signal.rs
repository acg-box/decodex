//! Versioned read-only autonomy signal evidence.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::prelude::{Result, eyre};

pub(crate) const AUTONOMY_SIGNAL_SCHEMA: &str = "decodex.autonomy_signal/1";

const AUTONOMY_SIGNAL_RECORD_VERSION: u16 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AutonomySignalKind {
	RuntimeHealth,
	ValidationRegression,
	ReviewFeedbackCluster,
	UserFeedbackCluster,
	SpecDrift,
	ProtocolDrift,
	MetricRegression,
	ExecutionFriction,
	DocsSkillDrift,
}
impl AutonomySignalKind {
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::RuntimeHealth => "runtime_health",
			Self::ValidationRegression => "validation_regression",
			Self::ReviewFeedbackCluster => "review_feedback_cluster",
			Self::UserFeedbackCluster => "user_feedback_cluster",
			Self::SpecDrift => "spec_drift",
			Self::ProtocolDrift => "protocol_drift",
			Self::MetricRegression => "metric_regression",
			Self::ExecutionFriction => "execution_friction",
			Self::DocsSkillDrift => "docs_skill_drift",
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AutonomySignalSourceType {
	User,
	Review,
	Ci,
	Telemetry,
	Runtime,
	Docs,
	Protocol,
	Agent,
	Tracker,
	Memory,
	Report,
}
impl AutonomySignalSourceType {
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::User => "user",
			Self::Review => "review",
			Self::Ci => "ci",
			Self::Telemetry => "telemetry",
			Self::Runtime => "runtime",
			Self::Docs => "docs",
			Self::Protocol => "protocol",
			Self::Agent => "agent",
			Self::Tracker => "tracker",
			Self::Memory => "memory",
			Self::Report => "report",
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AutonomySignalFreshness {
	Fresh,
	Stale,
	Unknown,
}
impl AutonomySignalFreshness {
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::Fresh => "fresh",
			Self::Stale => "stale",
			Self::Unknown => "unknown",
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AutonomySignalEvidenceClass {
	ExternalSource,
	RepoSource,
	LiveReadback,
	Inference,
	Gap,
}
impl AutonomySignalEvidenceClass {
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::ExternalSource => "external_source",
			Self::RepoSource => "repo_source",
			Self::LiveReadback => "live_readback",
			Self::Inference => "inference",
			Self::Gap => "gap",
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AutonomySignalConfidence {
	High,
	Medium,
	Low,
	Unknown,
}
impl AutonomySignalConfidence {
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::High => "high",
			Self::Medium => "medium",
			Self::Low => "low",
			Self::Unknown => "unknown",
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AutonomySignalPrivacy {
	Public,
	Team,
	LocalPrivate,
}
impl AutonomySignalPrivacy {
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::Public => "public",
			Self::Team => "team",
			Self::LocalPrivate => "local_private",
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AutonomySignalReviewRoute {
	pub(crate) route: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) finding_source: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) finding_index: Option<u64>,
	pub(crate) summary: String,
	#[serde(default)]
	pub(crate) evidence_refs: Vec<String>,
}
impl AutonomySignalReviewRoute {
	fn validate(&self) -> Result<()> {
		validate_required("autonomy signal review route.route", &self.route)?;
		validate_required("autonomy signal review route.summary", &self.summary)?;
		validate_optional_required(
			"autonomy signal review route.finding_source",
			self.finding_source.as_deref(),
		)?;

		validate_string_list("autonomy signal review route.evidence_refs", &self.evidence_refs)
	}
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AutonomySignalReviewEvidence {
	pub(crate) review_phase: String,
	pub(crate) review_status: String,
	pub(crate) head_sha: String,
	#[serde(default)]
	pub(crate) checkpoint_refs: Vec<String>,
	#[serde(default)]
	pub(crate) finding_routes: Vec<AutonomySignalReviewRoute>,
}
impl AutonomySignalReviewEvidence {
	fn validate(&self) -> Result<()> {
		validate_required("autonomy signal review evidence.review_phase", &self.review_phase)?;
		validate_required("autonomy signal review evidence.review_status", &self.review_status)?;
		validate_required("autonomy signal review evidence.head_sha", &self.head_sha)?;
		validate_nonempty_list(
			"autonomy signal review evidence.checkpoint_refs",
			&self.checkpoint_refs,
		)?;

		if self.finding_routes.is_empty() {
			eyre::bail!("Review-derived autonomy signals require finding_routes.");
		}

		for route in &self.finding_routes {
			route.validate()?;
		}

		Ok(())
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AutonomySignalInput {
	pub(crate) project_id: String,
	pub(crate) objective_id: String,
	pub(crate) objective_version: u64,
	pub(crate) source_type: AutonomySignalSourceType,
	pub(crate) source_refs: Vec<String>,
	pub(crate) primary_source_refs: Vec<String>,
	pub(crate) issue_id: Option<String>,
	pub(crate) run_id: Option<String>,
	pub(crate) attempt_id: Option<String>,
	pub(crate) head_sha: Option<String>,
	pub(crate) captured_at: String,
	pub(crate) freshness: AutonomySignalFreshness,
	pub(crate) summary: String,
	pub(crate) evidence: Vec<String>,
	pub(crate) evidence_class: AutonomySignalEvidenceClass,
	pub(crate) contradictions: Vec<String>,
	pub(crate) gaps: Vec<String>,
	pub(crate) confidence: AutonomySignalConfidence,
	pub(crate) privacy: AutonomySignalPrivacy,
	pub(crate) observed_counts: BTreeMap<String, u64>,
	pub(crate) review_evidence: Option<AutonomySignalReviewEvidence>,
	pub(crate) proposal_only: bool,
	pub(crate) created_at: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AutonomySignal {
	#[serde(default = "autonomy_signal_schema")]
	schema: String,
	#[serde(default = "autonomy_signal_record_version")]
	record_version: u16,
	id: String,
	fingerprint: String,
	project_id: String,
	objective_id: String,
	objective_version: u64,
	kind: AutonomySignalKind,
	source_type: AutonomySignalSourceType,
	source_refs: Vec<String>,
	#[serde(default)]
	primary_source_refs: Vec<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	issue_id: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	run_id: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	attempt_id: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	head_sha: Option<String>,
	captured_at: String,
	freshness: AutonomySignalFreshness,
	summary: String,
	evidence: Vec<String>,
	evidence_class: AutonomySignalEvidenceClass,
	#[serde(default)]
	contradictions: Vec<String>,
	#[serde(default)]
	gaps: Vec<String>,
	confidence: AutonomySignalConfidence,
	privacy: AutonomySignalPrivacy,
	#[serde(default)]
	observed_counts: BTreeMap<String, u64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	review_evidence: Option<AutonomySignalReviewEvidence>,
	proposal_only: bool,
	created_at: String,
}
#[allow(dead_code)]
impl AutonomySignal {
	pub(crate) fn runtime_health(input: AutonomySignalInput) -> Result<Self> {
		Self::from_input(AutonomySignalKind::RuntimeHealth, input)
	}

	pub(crate) fn validation_regression(input: AutonomySignalInput) -> Result<Self> {
		Self::from_input(AutonomySignalKind::ValidationRegression, input)
	}

	pub(crate) fn review_feedback_cluster(input: AutonomySignalInput) -> Result<Self> {
		Self::from_input(AutonomySignalKind::ReviewFeedbackCluster, input)
	}

	pub(crate) fn user_feedback_cluster(input: AutonomySignalInput) -> Result<Self> {
		Self::from_input(AutonomySignalKind::UserFeedbackCluster, input)
	}

	pub(crate) fn spec_drift(input: AutonomySignalInput) -> Result<Self> {
		Self::from_input(AutonomySignalKind::SpecDrift, input)
	}

	pub(crate) fn protocol_drift(input: AutonomySignalInput) -> Result<Self> {
		Self::from_input(AutonomySignalKind::ProtocolDrift, input)
	}

	#[allow(dead_code)]
	pub(crate) fn metric_regression(input: AutonomySignalInput) -> Result<Self> {
		Self::from_input(AutonomySignalKind::MetricRegression, input)
	}

	pub(crate) fn execution_friction(input: AutonomySignalInput) -> Result<Self> {
		Self::from_input(AutonomySignalKind::ExecutionFriction, input)
	}

	pub(crate) fn docs_skill_drift(input: AutonomySignalInput) -> Result<Self> {
		Self::from_input(AutonomySignalKind::DocsSkillDrift, input)
	}

	pub(crate) fn id(&self) -> &str {
		&self.id
	}

	pub(crate) fn fingerprint(&self) -> &str {
		&self.fingerprint
	}

	pub(crate) fn project_id(&self) -> &str {
		&self.project_id
	}

	pub(crate) fn objective_id(&self) -> &str {
		&self.objective_id
	}

	pub(crate) fn objective_version(&self) -> u64 {
		self.objective_version
	}

	pub(crate) fn kind(&self) -> AutonomySignalKind {
		self.kind
	}

	pub(crate) fn source_type(&self) -> AutonomySignalSourceType {
		self.source_type
	}

	pub(crate) fn freshness(&self) -> AutonomySignalFreshness {
		self.freshness
	}

	pub(crate) fn evidence_class(&self) -> AutonomySignalEvidenceClass {
		self.evidence_class
	}

	pub(crate) fn confidence(&self) -> AutonomySignalConfidence {
		self.confidence
	}

	pub(crate) fn privacy(&self) -> AutonomySignalPrivacy {
		self.privacy
	}

	pub(crate) fn summary(&self) -> &str {
		&self.summary
	}

	pub(crate) fn gaps(&self) -> &[String] {
		&self.gaps
	}

	pub(crate) fn contradictions(&self) -> &[String] {
		&self.contradictions
	}

	pub(crate) fn source_refs(&self) -> &[String] {
		&self.source_refs
	}

	pub(crate) fn primary_source_refs(&self) -> &[String] {
		&self.primary_source_refs
	}

	pub(crate) fn head_sha(&self) -> Option<&str> {
		self.head_sha.as_deref()
	}

	pub(crate) fn review_evidence(&self) -> Option<&AutonomySignalReviewEvidence> {
		self.review_evidence.as_ref()
	}

	pub(crate) fn validate(&self) -> Result<()> {
		validate_required("autonomy signal schema", &self.schema)?;
		validate_required("autonomy signal id", &self.id)?;
		validate_required("autonomy signal fingerprint", &self.fingerprint)?;
		validate_required("autonomy signal project_id", &self.project_id)?;
		validate_required("autonomy signal objective_id", &self.objective_id)?;
		validate_required("autonomy signal captured_at", &self.captured_at)?;
		validate_required("autonomy signal summary", &self.summary)?;
		validate_required("autonomy signal created_at", &self.created_at)?;
		validate_nonempty_list("autonomy signal source_refs", &self.source_refs)?;
		validate_nonempty_list("autonomy signal evidence", &self.evidence)?;
		validate_string_list("autonomy signal primary_source_refs", &self.primary_source_refs)?;
		validate_string_list("autonomy signal contradictions", &self.contradictions)?;
		validate_string_list("autonomy signal gaps", &self.gaps)?;
		validate_optional_required("autonomy signal issue_id", self.issue_id.as_deref())?;
		validate_optional_required("autonomy signal run_id", self.run_id.as_deref())?;
		validate_optional_required("autonomy signal attempt_id", self.attempt_id.as_deref())?;
		validate_optional_required("autonomy signal head_sha", self.head_sha.as_deref())?;

		if self.schema != AUTONOMY_SIGNAL_SCHEMA {
			eyre::bail!("Autonomy signal `{}` has unsupported schema `{}`.", self.id, self.schema);
		}
		if self.record_version != AUTONOMY_SIGNAL_RECORD_VERSION {
			eyre::bail!(
				"Autonomy signal `{}` has unsupported record_version `{}`.",
				self.id,
				self.record_version
			);
		}
		if self.objective_version == 0 {
			eyre::bail!(
				"Autonomy signal `{}` objective_version must be greater than zero.",
				self.id
			);
		}
		if !self.proposal_only {
			eyre::bail!("Autonomy signal `{}` must remain proposal-only evidence.", self.id);
		}

		self.validate_source_specific_rules()?;

		let expected = autonomy_signal_fingerprint(self)?;

		if expected != self.fingerprint {
			eyre::bail!(
				"Autonomy signal `{}` fingerprint mismatch: expected `{expected}`.",
				self.id
			);
		}

		let expected_id = autonomy_signal_id(&expected);

		if expected_id != self.id {
			eyre::bail!(
				"Autonomy signal id `{}` does not match fingerprint `{expected}`.",
				self.id
			);
		}

		Ok(())
	}

	fn from_input(kind: AutonomySignalKind, input: AutonomySignalInput) -> Result<Self> {
		let mut signal = Self {
			schema: autonomy_signal_schema(),
			record_version: autonomy_signal_record_version(),
			id: String::new(),
			fingerprint: String::new(),
			project_id: input.project_id,
			objective_id: input.objective_id,
			objective_version: input.objective_version,
			kind,
			source_type: input.source_type,
			source_refs: input.source_refs,
			primary_source_refs: input.primary_source_refs,
			issue_id: input.issue_id,
			run_id: input.run_id,
			attempt_id: input.attempt_id,
			head_sha: input.head_sha,
			captured_at: input.captured_at,
			freshness: input.freshness,
			summary: input.summary,
			evidence: input.evidence,
			evidence_class: input.evidence_class,
			contradictions: input.contradictions,
			gaps: input.gaps,
			confidence: input.confidence,
			privacy: input.privacy,
			observed_counts: input.observed_counts,
			review_evidence: input.review_evidence,
			proposal_only: input.proposal_only,
			created_at: input.created_at,
		};
		let fingerprint = autonomy_signal_fingerprint(&signal)?;

		signal.id = autonomy_signal_id(&fingerprint);
		signal.fingerprint = fingerprint;

		signal.validate()?;

		Ok(signal)
	}

	fn validate_source_specific_rules(&self) -> Result<()> {
		if matches!(
			self.source_type,
			AutonomySignalSourceType::Memory | AutonomySignalSourceType::Report
		) && self.primary_source_refs.is_empty()
		{
			eyre::bail!(
				"Memory/report autonomy signal `{}` requires primary_source_refs.",
				self.id
			);
		}
		if self.kind == AutonomySignalKind::ReviewFeedbackCluster
			|| self.source_type == AutonomySignalSourceType::Review
		{
			let Some(review_evidence) = &self.review_evidence else {
				eyre::bail!(
					"Review-derived autonomy signal `{}` requires review_evidence.",
					self.id
				);
			};

			review_evidence.validate()?;

			let Some(head_sha) = self.head_sha.as_deref() else {
				eyre::bail!("Review-derived autonomy signal `{}` requires head_sha.", self.id);
			};

			if review_evidence.head_sha != head_sha {
				eyre::bail!(
					"Review-derived autonomy signal `{}` head_sha must match review evidence head.",
					self.id
				);
			}
		}

		Ok(())
	}
}

fn autonomy_signal_schema() -> String {
	AUTONOMY_SIGNAL_SCHEMA.to_owned()
}

const fn autonomy_signal_record_version() -> u16 {
	AUTONOMY_SIGNAL_RECORD_VERSION
}

fn autonomy_signal_id(fingerprint: &str) -> String {
	format!("autonomy_signal:{fingerprint}")
}

fn autonomy_signal_fingerprint(signal: &AutonomySignal) -> Result<String> {
	let material = serde_json::json!({
		"schema": signal.schema,
		"record_version": signal.record_version,
		"project_id": signal.project_id,
		"objective_id": signal.objective_id,
		"objective_version": signal.objective_version,
		"kind": signal.kind.as_str(),
		"source_type": signal.source_type.as_str(),
		"source_refs": sorted_strings(&signal.source_refs),
		"primary_source_refs": sorted_strings(&signal.primary_source_refs),
		"issue_id": signal.issue_id,
		"run_id": signal.run_id,
		"attempt_id": signal.attempt_id,
		"head_sha": signal.head_sha,
		"freshness": signal.freshness.as_str(),
		"summary": signal.summary,
		"evidence": sorted_strings(&signal.evidence),
		"evidence_class": signal.evidence_class.as_str(),
		"contradictions": sorted_strings(&signal.contradictions),
		"gaps": sorted_strings(&signal.gaps),
		"confidence": signal.confidence.as_str(),
		"privacy": signal.privacy.as_str(),
		"review_evidence": canonical_review_evidence(signal.review_evidence.as_ref()),
		"proposal_only": signal.proposal_only,
	});
	let payload = serde_json::to_vec(&material)?;
	let digest = Sha256::digest(payload);
	let mut hash = String::with_capacity(64);

	for byte in digest {
		hash.push(char::from(b"0123456789abcdef"[(byte >> 4) as usize]));
		hash.push(char::from(b"0123456789abcdef"[(byte & 0x0f) as usize]));
	}

	Ok(hash)
}

fn canonical_review_evidence(evidence: Option<&AutonomySignalReviewEvidence>) -> serde_json::Value {
	let Some(evidence) = evidence else {
		return serde_json::Value::Null;
	};
	let mut finding_routes = evidence
		.finding_routes
		.iter()
		.map(|route| {
			serde_json::json!({
				"route": route.route,
				"finding_source": route.finding_source,
				"finding_index": route.finding_index,
				"summary": route.summary,
				"evidence_refs": sorted_strings(&route.evidence_refs),
			})
		})
		.collect::<Vec<_>>();

	finding_routes.sort_by_key(serde_json::Value::to_string);

	serde_json::json!({
		"review_phase": evidence.review_phase,
		"review_status": evidence.review_status,
		"head_sha": evidence.head_sha,
		"checkpoint_refs": sorted_strings(&evidence.checkpoint_refs),
		"finding_routes": finding_routes,
	})
}

fn sorted_strings(values: &[String]) -> Vec<&str> {
	let mut values = values.iter().map(String::as_str).collect::<Vec<_>>();

	values.sort_unstable();

	values
}

fn validate_required(name: &str, value: &str) -> Result<()> {
	if value.trim().is_empty() {
		eyre::bail!("{name} must not be empty.");
	}

	Ok(())
}

fn validate_optional_required(name: &str, value: Option<&str>) -> Result<()> {
	if let Some(value) = value {
		validate_required(name, value)?;
	}

	Ok(())
}

fn validate_string_list(name: &str, values: &[String]) -> Result<()> {
	for value in values {
		validate_required(name, value)?;
	}

	Ok(())
}

fn validate_nonempty_list(name: &str, values: &[String]) -> Result<()> {
	if values.is_empty() {
		eyre::bail!("{name} must not be empty.");
	}

	validate_string_list(name, values)
}

#[cfg(test)]
mod tests {
	use std::collections::BTreeMap;

	use crate::{
		autonomy_objective::{
			AutonomyObjectiveAcceptance, AutonomyObjectiveActorKind, AutonomyObjectiveContract,
		},
		autonomy_signal::{
			AutonomySignal, AutonomySignalConfidence, AutonomySignalEvidenceClass,
			AutonomySignalFreshness, AutonomySignalInput, AutonomySignalPrivacy,
			AutonomySignalReviewEvidence, AutonomySignalReviewRoute, AutonomySignalSourceType,
		},
		state::StateStore,
	};

	fn signal_input() -> AutonomySignalInput {
		AutonomySignalInput {
			project_id: String::from("decodex"),
			objective_id: String::from("quality-autonomy"),
			objective_version: 1,
			source_type: AutonomySignalSourceType::Runtime,
			source_refs: vec![String::from("status:XY-1085:runtime-health")],
			primary_source_refs: Vec::new(),
			issue_id: Some(String::from("XY-1085")),
			run_id: Some(String::from("xy-1085-attempt-1")),
			attempt_id: Some(String::from("1")),
			head_sha: Some(String::from("3273e45234aa3346e194a7a9e48cd1c58c3e408c")),
			captured_at: String::from("2026-06-22T00:00:00Z"),
			freshness: AutonomySignalFreshness::Fresh,
			summary: String::from("Runtime status readback remained internally consistent."),
			evidence: vec![String::from("status readback had no contradictory lane states")],
			evidence_class: AutonomySignalEvidenceClass::LiveReadback,
			contradictions: Vec::new(),
			gaps: vec![String::from("No external dashboard readback included.")],
			confidence: AutonomySignalConfidence::Medium,
			privacy: AutonomySignalPrivacy::LocalPrivate,
			observed_counts: BTreeMap::new(),
			review_evidence: None,
			proposal_only: true,
			created_at: String::from("2026-06-22T00:00:05Z"),
		}
	}

	fn review_evidence() -> AutonomySignalReviewEvidence {
		AutonomySignalReviewEvidence {
			review_phase: String::from("handoff"),
			review_status: String::from("findings"),
			head_sha: String::from("3273e45234aa3346e194a7a9e48cd1c58c3e408c"),
			checkpoint_refs: vec![String::from(
				"review_checkpoint:XY-1085:3273e45234aa3346e194a7a9e48cd1c58c3e408c",
			)],
			finding_routes: vec![AutonomySignalReviewRoute {
				route: String::from("follow_up"),
				finding_source: Some(String::from("accepted_findings")),
				finding_index: Some(0),
				summary: String::from("Follow-up evidence should inform future proposals."),
				evidence_refs: vec![String::from("finding_routes[0]")],
			}],
		}
	}

	fn objective_fixture(version: u64) -> AutonomyObjectiveContract {
		serde_json::from_value(serde_json::json!({
			"schema": "decodex.autonomy_objective/1",
			"record_version": 1,
			"project_id": "decodex",
			"id": "quality-autonomy",
			"version": version,
			"state": "draft",
			"summary": "Improve Decodex autonomy quality under explicit authority.",
			"goals": ["Reduce repeated validation and review churn."],
			"non_goals": ["Do not bypass Decision Contract authority."],
			"metrics": ["Validation retry count stays below objective tolerance."],
			"allowed_surfaces": ["apps/decodex/src", "docs/spec"],
			"allowed_signal_kinds": ["runtime_health", "review_feedback_cluster"],
			"validation_gates": ["cargo make check-docs"],
			"review_policy": "independent current-head review required",
			"memory_policy": "read-only source-linked memory only",
			"report_policy": "public-safe summaries only"
		}))
		.expect("objective fixture should parse")
	}

	fn accept_objective(store: &StateStore, version: u64) {
		store
			.upsert_autonomy_objective_draft("decodex", objective_fixture(version))
			.expect("draft objective should store");
		store
			.accept_autonomy_objective_version(
				"decodex",
				"quality-autonomy",
				version,
				AutonomyObjectiveAcceptance::new(
					"operator",
					AutonomyObjectiveActorKind::User,
					format!("2026-06-22T00:0{version}:00Z"),
					"conversation",
				)
				.expect("acceptance should validate"),
			)
			.expect("objective should accept");
	}

	#[test]
	fn autonomy_signal_fingerprint_ignores_timestamps_and_counts() {
		let signal =
			AutonomySignal::runtime_health(signal_input()).expect("runtime signal should validate");
		let mut input = signal_input();

		input.captured_at = String::from("2026-06-22T00:05:00Z");
		input.created_at = String::from("2026-06-22T00:05:05Z");

		input.observed_counts.insert(String::from("validation_retry_count"), 7);

		let changed = AutonomySignal::runtime_health(input)
			.expect("runtime signal with volatile fields should validate");

		assert_eq!(signal.fingerprint(), changed.fingerprint());
		assert_eq!(signal.id(), changed.id());
	}

	#[test]
	fn autonomy_signal_review_feedback_requires_finding_routes_and_current_head_evidence() {
		let mut input = signal_input();

		input.source_type = AutonomySignalSourceType::Review;

		assert!(AutonomySignal::review_feedback_cluster(input.clone()).is_err());

		input.review_evidence = Some(review_evidence());

		let signal = AutonomySignal::review_feedback_cluster(input)
			.expect("review signal should require normalized route evidence");

		assert_eq!(signal.review_evidence().expect("review evidence").finding_routes.len(), 1);
		assert_eq!(signal.head_sha(), Some("3273e45234aa3346e194a7a9e48cd1c58c3e408c"));
	}

	#[test]
	fn autonomy_signal_memory_and_report_sources_require_primary_refs_and_proposal_only() {
		for source_type in [AutonomySignalSourceType::Memory, AutonomySignalSourceType::Report] {
			let mut input = signal_input();

			input.source_type = source_type;
			input.source_refs = vec![String::from("memory:summary:older-context")];
			input.primary_source_refs = Vec::new();
			input.proposal_only = false;

			assert!(AutonomySignal::docs_skill_drift(input.clone()).is_err());

			input.primary_source_refs = vec![String::from("docs/spec/runtime.md")];
			input.proposal_only = true;

			AutonomySignal::docs_skill_drift(input)
				.expect("memory/report signals with primary refs remain proposal-only");
		}
	}

	#[test]
	fn autonomy_signal_store_round_trips_exact_objective_version() {
		let store = StateStore::open_in_memory().expect("store should open");

		accept_objective(&store, 1);

		let signal_v1 =
			AutonomySignal::runtime_health(signal_input()).expect("runtime signal should validate");
		let stored_v1 = store
			.record_autonomy_signal("decodex", signal_v1.clone())
			.expect("signal should store");

		assert_eq!(stored_v1.signal().objective_version(), 1);
		assert_eq!(stored_v1.signal().freshness(), AutonomySignalFreshness::Fresh);
		assert_eq!(stored_v1.signal().gaps(), ["No external dashboard readback included."]);
		assert_eq!(stored_v1.signal().privacy(), AutonomySignalPrivacy::LocalPrivate);

		accept_objective(&store, 2);

		let mut input_v2 = signal_input();

		input_v2.objective_version = 2;
		input_v2.source_refs = vec![String::from("status:XY-1085:runtime-health:v2")];

		let signal_v2 =
			AutonomySignal::runtime_health(input_v2).expect("runtime signal should validate");

		store.record_autonomy_signal("decodex", signal_v2).expect("v2 signal should store");

		let v1_signals = store
			.list_autonomy_signals_for_objective("decodex", "quality-autonomy", 1)
			.expect("v1 signals should list");
		let v2_signals = store
			.list_autonomy_signals_for_objective("decodex", "quality-autonomy", 2)
			.expect("v2 signals should list");

		assert_eq!(v1_signals.len(), 1);
		assert_eq!(v1_signals[0].signal().id(), signal_v1.id());
		assert_eq!(v2_signals.len(), 1);
		assert_ne!(v1_signals[0].signal().id(), v2_signals[0].signal().id());
	}

	#[test]
	fn autonomy_signal_persistent_store_round_trips_signal_payload() {
		let tempdir = tempfile::tempdir().expect("tempdir should create");
		let db_path = tempdir.path().join("runtime.sqlite3");
		let signal = {
			let store = StateStore::open(&db_path).expect("store should open");

			accept_objective(&store, 1);

			let signal = AutonomySignal::runtime_health(signal_input())
				.expect("runtime signal should validate");

			store.record_autonomy_signal("decodex", signal.clone()).expect("signal should store");

			signal
		};
		let reopened = StateStore::open(&db_path).expect("store should reopen");
		let stored = reopened
			.autonomy_signal("decodex", signal.id())
			.expect("signal read should succeed")
			.expect("signal should exist");

		assert_eq!(stored.signal(), &signal);
		assert_eq!(stored.signal().source_refs(), ["status:XY-1085:runtime-health"]);
		assert!(stored.signal().primary_source_refs().is_empty());
	}

	#[test]
	fn autonomy_signal_status_readback_exposes_recent_signal_metadata() {
		let store = StateStore::open_in_memory().expect("store should open");

		accept_objective(&store, 1);

		store
			.record_autonomy_signal(
				"decodex",
				AutonomySignal::runtime_health(signal_input())
					.expect("runtime signal should validate"),
			)
			.expect("signal should store");

		let snapshot =
			store.project_loop_evidence_snapshot("decodex").expect("loop evidence should load");
		let recent = snapshot.recent_autonomy_signals(1);
		let signal = recent[0].signal();

		assert_eq!(signal.objective_id(), "quality-autonomy");
		assert_eq!(signal.objective_version(), 1);
		assert_eq!(signal.freshness(), AutonomySignalFreshness::Fresh);
		assert_eq!(signal.confidence(), AutonomySignalConfidence::Medium);
		assert_eq!(signal.privacy(), AutonomySignalPrivacy::LocalPrivate);
		assert_eq!(signal.gaps(), ["No external dashboard readback included."]);
	}
}
