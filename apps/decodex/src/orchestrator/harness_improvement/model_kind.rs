use crate::orchestrator::harness_improvement::model::HarnessOutcomeSignals;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HarnessOutcomeKind {
	ReviewHandoff,
	ReviewRepair,
	Closeout,
	RetryableFailure,
	TerminalFailure,
	ManualAttention,
}
impl HarnessOutcomeKind {
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::ReviewHandoff => "review_handoff",
			Self::ReviewRepair => "review_repair",
			Self::Closeout => "closeout",
			Self::RetryableFailure => "retryable_failure",
			Self::TerminalFailure => "terminal_failure",
			Self::ManualAttention => "manual_attention",
		}
	}

	pub(super) fn validation_result(
		self,
		explicit: Option<&str>,
		signals: &HarnessOutcomeSignals,
	) -> String {
		if let Some(result) = explicit {
			return result.to_owned();
		}

		if signals.validation_failure_count > 0 {
			return String::from("failed");
		}
		if matches!(self, Self::ReviewHandoff | Self::ReviewRepair | Self::Closeout) {
			return String::from("passed");
		}

		String::from("not_recorded")
	}
}

pub(crate) struct HarnessOutcomeRecordInput<'a> {
	pub(crate) project_id: &'a str,
	pub(crate) issue_id: &'a str,
	pub(crate) issue_identifier: &'a str,
	pub(crate) run_id: &'a str,
	pub(crate) attempt_number: i64,
	pub(crate) outcome: HarnessOutcomeKind,
	pub(crate) error_class: Option<&'a str>,
	pub(crate) validation_result: Option<&'a str>,
	pub(crate) pr_url: Option<&'a str>,
}
