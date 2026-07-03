pub(crate) struct RetryComment<'a> {
	pub(crate) run_id: &'a str,
	pub(crate) attempt_number: i64,
	pub(crate) retry_budget_attempt_number: i64,
	pub(crate) max_attempts: i64,
	pub(crate) worktree_path: String,
	pub(crate) branch_name: &'a str,
	pub(crate) error_class: &'a str,
	pub(crate) next_action: &'a str,
}
