//! Explicit deferred-state projection for repository factory capabilities.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Current availability of the deferred managed-repository capability.
pub enum ManagedRepositoryReadiness {
	/// The capability has all required owners and can accept work.
	Ready,
	/// This product slice intentionally disables the capability.
	Disabled,
	/// A required owner is unavailable for the stated reason.
	Unavailable(ManagedRepositoryUnavailableReason),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Closed reasons that prevent managed-repository work from becoming ready.
pub enum ManagedRepositoryUnavailableReason {
	/// The product-state owner is unavailable.
	ProductStore,
	/// The effect executor is unavailable.
	Executor,
	/// The durable reconciliation owner is unavailable.
	Reconciliation,
	/// Restart recovery found unresolved prior work.
	RestartWorkResidual,
}

#[derive(Clone, Copy)]
pub(crate) enum ManagedRepositoryCapability {
	Disabled,
	Unavailable(ManagedRepositoryUnavailableReason),
}

impl ManagedRepositoryCapability {
	pub(crate) const fn unavailable(reason: ManagedRepositoryUnavailableReason) -> Self {
		Self::Unavailable(reason)
	}

	pub(crate) const fn readiness(&self) -> ManagedRepositoryReadiness {
		match self {
			Self::Disabled => ManagedRepositoryReadiness::Disabled,
			Self::Unavailable(reason) => ManagedRepositoryReadiness::Unavailable(*reason),
		}
	}
}
