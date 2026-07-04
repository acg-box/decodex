pub(in crate::cli::git_hook_commands) const ZERO_OID: &str =
	"0000000000000000000000000000000000000000";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::cli::git_hook_commands) struct PrePushUpdate {
	pub(in crate::cli::git_hook_commands) local_ref: String,
	pub(in crate::cli::git_hook_commands) local_oid: String,
	pub(in crate::cli::git_hook_commands) remote_ref: String,
	pub(in crate::cli::git_hook_commands) remote_oid: String,
}
impl PrePushUpdate {
	pub(in crate::cli::git_hook_commands) fn new(
		local_ref: String,
		local_oid: String,
		remote_ref: String,
		remote_oid: String,
	) -> Self {
		Self { local_ref, local_oid, remote_ref, remote_oid }
	}
}
