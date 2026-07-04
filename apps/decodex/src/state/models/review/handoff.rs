#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ReviewHandoffMarker {
	pub(in crate::state) run_id: String,
	pub(in crate::state) attempt_number: i64,
	pub(in crate::state) branch_name: String,
	pub(in crate::state) pr_url: String,
	pub(in crate::state) target_base_ref_name: Option<String>,
	pub(in crate::state) pr_head_ref_name: String,
	pub(in crate::state) pr_head_oid: String,
}
impl ReviewHandoffMarker {
	pub(crate) fn new(
		run_id: impl Into<String>,
		attempt_number: i64,
		branch_name: impl Into<String>,
		pr_url: impl Into<String>,
		target_base_ref_name: impl Into<String>,
		pr_head_ref_name: impl Into<String>,
		pr_head_oid: impl Into<String>,
	) -> Self {
		Self {
			run_id: run_id.into(),
			attempt_number,
			branch_name: branch_name.into(),
			pr_url: pr_url.into(),
			target_base_ref_name: Some(target_base_ref_name.into()),
			pr_head_ref_name: pr_head_ref_name.into(),
			pr_head_oid: pr_head_oid.into(),
		}
	}

	pub(crate) fn branch_name(&self) -> &str {
		&self.branch_name
	}

	pub(crate) fn run_id(&self) -> &str {
		&self.run_id
	}

	pub(crate) fn attempt_number(&self) -> i64 {
		self.attempt_number
	}

	pub(crate) fn pr_url(&self) -> &str {
		&self.pr_url
	}

	pub(crate) fn target_base_ref_name(&self) -> Option<&str> {
		self.target_base_ref_name.as_deref()
	}

	pub(crate) fn pr_head_ref_name(&self) -> &str {
		&self.pr_head_ref_name
	}

	pub(crate) fn pr_head_oid(&self) -> &str {
		&self.pr_head_oid
	}
}
