/// A resolved repo gate ready to execute.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResolvedRepoGate<'a> {
	pub(in crate::workflow) profile_name: Option<&'a str>,
	pub(in crate::workflow) canonicalize_commands: &'a [String],
	pub(in crate::workflow) verify_commands: &'a [String],
}
impl<'a> ResolvedRepoGate<'a> {
	/// Optional selected profile name; `None` means the default full gate.
	pub fn profile_name(&self) -> Option<&'a str> {
		self.profile_name
	}

	/// Canonicalize commands selected for this gate run.
	pub fn canonicalize_commands(&self) -> &'a [String] {
		self.canonicalize_commands
	}

	/// Verification commands selected for this gate run.
	pub fn verify_commands(&self) -> &'a [String] {
		self.verify_commands
	}
}
