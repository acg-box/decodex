use crate::orchestrator::harness_improvement::Serialize;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct HarnessImprovementCandidateSummary {
	pub(crate) kind: String,
	pub(crate) reason_code: String,
	pub(crate) target: String,
	pub(crate) source_event_count: usize,
	pub(crate) recommendation: String,
}
