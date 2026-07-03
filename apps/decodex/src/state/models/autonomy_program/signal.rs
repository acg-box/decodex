use crate::autonomy_signal::{
	AutonomySignal, AutonomySignalConfidence, AutonomySignalEvidenceClass, AutonomySignalFreshness,
	AutonomySignalKind, AutonomySignalPrivacy,
};

/// SQLite-backed autonomy signal evidence retained by the local runtime.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AutonomySignalRecord {
	pub(in crate::state) project_id: String,
	pub(in crate::state) signal: AutonomySignal,
	pub(in crate::state) created_at: String,
	pub(in crate::state) created_at_unix: i64,
	pub(in crate::state) updated_at: String,
	pub(in crate::state) updated_at_unix: i64,
}
#[allow(dead_code)]
impl AutonomySignalRecord {
	pub(crate) fn project_id(&self) -> &str {
		&self.project_id
	}

	pub(crate) fn signal(&self) -> &AutonomySignal {
		&self.signal
	}

	pub(crate) fn signal_id(&self) -> &str {
		self.signal.id()
	}

	pub(crate) fn objective_id(&self) -> &str {
		self.signal.objective_id()
	}

	pub(crate) fn objective_version(&self) -> u64 {
		self.signal.objective_version()
	}

	pub(crate) fn kind(&self) -> AutonomySignalKind {
		self.signal.kind()
	}

	pub(crate) fn freshness(&self) -> AutonomySignalFreshness {
		self.signal.freshness()
	}

	pub(crate) fn evidence_class(&self) -> AutonomySignalEvidenceClass {
		self.signal.evidence_class()
	}

	pub(crate) fn confidence(&self) -> AutonomySignalConfidence {
		self.signal.confidence()
	}

	pub(crate) fn privacy(&self) -> AutonomySignalPrivacy {
		self.signal.privacy()
	}

	pub(crate) fn created_at(&self) -> &str {
		&self.created_at
	}

	pub(crate) fn created_at_unix(&self) -> i64 {
		self.created_at_unix
	}

	pub(crate) fn updated_at(&self) -> &str {
		&self.updated_at
	}

	pub(crate) fn updated_at_unix(&self) -> i64 {
		self.updated_at_unix
	}
}
