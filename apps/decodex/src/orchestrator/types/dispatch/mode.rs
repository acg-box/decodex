#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum IssueDispatchMode {
	Normal,
	Program,
	Retry,
	ReviewRepair,
	Closeout,
}
impl IssueDispatchMode {
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::Normal => "normal",
			Self::Program => "program",
			Self::Retry => "retry",
			Self::ReviewRepair => "review_repair",
			Self::Closeout => "closeout",
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RetryKind {
	Continuation,
	Failure,
}
impl RetryKind {
	pub(crate) const fn as_str(self) -> &'static str {
		match self {
			Self::Continuation => "continuation",
			Self::Failure => "failure",
		}
	}
}
