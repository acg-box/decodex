use serde::Serialize;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DecodexRunContext {
	pub(crate) run_id: String,
	pub(crate) attempt_number: i64,
	pub(crate) issue_id: String,
	pub(crate) issue_identifier: String,
	pub(crate) branch: String,
	pub(crate) worktree_path: String,
	pub(crate) max_turns: u32,
	pub(crate) default_canonicalize_commands: Vec<String>,
	pub(crate) default_verify_commands: Vec<String>,
}
