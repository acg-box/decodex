#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct IssueFacts {
	pub(crate) has_active_label: bool,
	pub(crate) has_opt_out_label: bool,
	pub(crate) has_needs_attention_label: bool,
	pub(crate) has_generic_dispatch_briefing: bool,
	pub(crate) has_open_blockers: bool,
}
