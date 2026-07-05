#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::recovery) enum RecoveryRuntimeMutationPolicy {
	AllowRuntimeWrites,
	ReadOnly,
}
impl RecoveryRuntimeMutationPolicy {
	pub(in crate::recovery) const fn allows_runtime_writes(self) -> bool {
		matches!(self, Self::AllowRuntimeWrites)
	}
}
