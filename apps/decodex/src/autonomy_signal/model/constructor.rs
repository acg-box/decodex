use crate::{
	autonomy_signal::{
		AutonomySignal, AutonomySignalInput,
		fingerprint::{self},
		model,
		types::AutonomySignalKind,
	},
	prelude::Result,
};

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

	fn from_input(kind: AutonomySignalKind, input: AutonomySignalInput) -> Result<Self> {
		let mut signal = Self {
			schema: model::autonomy_signal_schema(),
			record_version: model::autonomy_signal_record_version(),
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
		let fingerprint = fingerprint::autonomy_signal_fingerprint(&signal)?;

		signal.id = fingerprint::autonomy_signal_id(&fingerprint);
		signal.fingerprint = fingerprint;

		signal.validate()?;

		Ok(signal)
	}
}
