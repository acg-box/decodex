//! `decodexd` lifecycle assembly and the same-UID V1 local connection owner.
//!
//! Default account-process composition remains crate-private and cannot be called by a product
//! root. The V22 manual runner requires an explicit non-production feature and binary:
//!
//! ```compile_fail
//! use decodex_runtime::ManualAccountLauncher;
//! ```
//!
//! Stateless execution coordination is not connected to a production root. It can produce only
//! an immutable V17 plan and prepared ProviderAttempt binding, an inert wait projection, a
//! no-route result, or a typed fail-closed result. No dispatch gate is exported or constructed.

#[expect(dead_code, reason = "dormant until a later explicit product authority enables routing")]
mod account_launch;
mod application;
mod bootstrap;
#[expect(dead_code, reason = "sealed until the accepted GitHub-effect composition owner")]
pub(crate) mod github_effects;
mod managed_repository_executor;
mod managed_repository_runtime;
mod managed_repository_saga;
mod process_platform;
mod process_supervisor;
mod provider_attempt_service;
mod routing_orchestration;
mod supervised_validation;
mod websocket;

pub use application::{Application, ApplicationPublication};
#[cfg(feature = "retained-title-experiment")]
pub use account_launch::retained_title_experiment::{
	ManualRetainedTitleExperimentError, ManualRetainedTitleExperimentReport,
	run_manual_retained_title_experiment,
};
pub use bootstrap::ServiceBootstrap;
pub use decodex_protocol::ServerId;
pub use managed_repository_runtime::ManagedRepositoryReadiness;
pub use managed_repository_saga::{
	ManagedRepositoryEffectPort, ManagedRepositoryEffectSaga, ManagedRepositoryRestartOutcome,
	ManagedRepositorySagaOutcome, RepositoryDispatchFailure, RepositoryDispatchObservation,
	RepositoryReadbackEvidence,
};
pub use process_supervisor::{
	ProcessGenerationControl, ProcessGenerationDiagnostic, ProcessGenerationExitWitnessKind,
	ProcessGenerationObservation, ProcessGenerationReadiness, ProcessGenerationReconciliation,
	ProcessGenerationTermination, ProcessSupervisorError,
};
pub use provider_attempt_service::{
	ProviderAttemptControl, ProviderAttemptDiagnostic, ProviderAttemptReadiness,
	ProviderAttemptReconciliation, ProviderAttemptServiceError, ProviderEvidenceLookupError,
	ProviderPositiveEvidenceSource,
};
pub use routing_orchestration::{
	ContinuationCoordinates, ExecutionCommand, ExecutionCoordinator, ExecutionFailure,
	ExecutionFailureKind, ExecutionOutcome, PersistedDecisionProvenance,
	PreparedAttemptHandoff, RoutingAuthorityRejection, WaitingReconciliationHandoff,
	WaitingUsageHandoff,
};
pub use supervised_validation::{
	ProtectedWorktreeFingerprint, ProtectedWorktreeStateProbe, SupervisedValidationEvidence,
	ValidationAcceptance, ValidationCancellation, ValidationCommandAuthority, ValidationRejection,
	ValidationSupervisionError, ValidationTermination, supervise_validation,
};
pub use websocket::{
	BoundServer, OwnedTaskIdentity, OwnedTaskKind, ProtocolServer, ServerConfig, ServerError,
	SpawnId, TerminationPrimary, TerminationReceipt,
};

#[cfg(test)] use {tempfile as _, tokio_tungstenite as _};

use decodex_core::DecodexRoot;

/// The vNext service assembly selected by the `decodexd` composition root.
#[derive(Clone, Copy, Debug)]
pub struct ServiceComposition;
impl ServiceComposition {
	/// Bootstrap the platform-default typed root and fail closed into doctor/status.
	pub async fn bootstrap_default() -> ServiceBootstrap {
		bootstrap::bootstrap_default().await
	}

	/// Bootstrap an explicit validated root. This is the deterministic test/embedding seam.
	pub async fn bootstrap(root: DecodexRoot) -> ServiceBootstrap {
		bootstrap::bootstrap(root).await
	}
}
