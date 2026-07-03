use crate::autonomy_signal::{
	AutonomySignal,
	review::AutonomySignalReviewEvidence,
	types::{
		AutonomySignalConfidence, AutonomySignalEvidenceClass, AutonomySignalFreshness,
		AutonomySignalKind, AutonomySignalPrivacy, AutonomySignalSourceType,
	},
};

#[allow(dead_code)]
impl AutonomySignal {
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
}
