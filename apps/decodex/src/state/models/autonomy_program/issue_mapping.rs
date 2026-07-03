/// SQLite-backed normal Linear issue mapping for one internal program node.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ProgramIssueMappingRecord {
	pub(in crate::state) project_id: String,
	pub(in crate::state) program_id: String,
	pub(in crate::state) node_id: String,
	pub(in crate::state) issue_id: String,
	pub(in crate::state) issue_identifier: String,
	pub(in crate::state) issue_state: String,
	pub(in crate::state) queue_intent: String,
	pub(in crate::state) has_active_label: bool,
	pub(in crate::state) has_opt_out_label: bool,
	pub(in crate::state) has_needs_attention_label: bool,
	pub(in crate::state) has_generic_dispatch_briefing: bool,
	pub(in crate::state) created_at: String,
	pub(in crate::state) created_at_unix: i64,
	pub(in crate::state) updated_at: String,
	pub(in crate::state) updated_at_unix: i64,
}
#[allow(dead_code)]
impl ProgramIssueMappingRecord {
	pub(crate) fn project_id(&self) -> &str {
		&self.project_id
	}

	pub(crate) fn program_id(&self) -> &str {
		&self.program_id
	}

	pub(crate) fn node_id(&self) -> &str {
		&self.node_id
	}

	pub(crate) fn issue_id(&self) -> &str {
		&self.issue_id
	}

	pub(crate) fn issue_identifier(&self) -> &str {
		&self.issue_identifier
	}

	pub(crate) fn issue_state(&self) -> &str {
		&self.issue_state
	}

	pub(crate) fn queue_intent(&self) -> &str {
		&self.queue_intent
	}

	pub(crate) fn has_active_label(&self) -> bool {
		self.has_active_label
	}

	pub(crate) fn has_opt_out_label(&self) -> bool {
		self.has_opt_out_label
	}

	pub(crate) fn has_needs_attention_label(&self) -> bool {
		self.has_needs_attention_label
	}

	pub(crate) fn has_generic_dispatch_briefing(&self) -> bool {
		self.has_generic_dispatch_briefing
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
