//! `decodexd` lifecycle assembly and the same-UID V2.2 local connection owner.
//!
//! Account-process and routing composition remain crate-private. The ordinary Quick Task owner
//! composes them without exporting raw process, routing, or provider-dispatch facades.

mod account_api;
mod account_import;
#[expect(dead_code, reason = "dormant until a later explicit product authority enables routing")]
mod account_launch;
mod account_observation;
mod account_profile;
mod account_service;
mod application;
mod auth_projection;
mod bootstrap;
#[expect(dead_code, reason = "sealed until the accepted GitHub-effect composition owner")]
pub(crate) mod github_effects;
mod host_credentials;
#[path = "managed_repository_disabled.rs"] mod managed_repository_runtime;
mod process_platform;
mod process_supervisor;
mod provider_attempt_service;
mod quick_task;
mod routing_orchestration;
mod supervised_validation;
mod websocket;

pub use account_service::{
	AccountInspection, AccountLifecycleError, AccountSelectionFailure, AccountSelectionResult,
	AccountService, ChatgptTokenProjection, CredentialRefreshError, StartupAccountReconciliation,
};
pub use application::{Application, ApplicationEventPublication, ApplicationPublication};
pub use bootstrap::{LocalDatabaseError, ServiceBootstrap};
pub use decodex_core::DecodexRoot;
pub use decodex_protocol::ServerId;
pub use host_credentials::{
	CredentialSecretBundle, CredentialStoreError, HostCredentialStore, SqliteCredentialStore,
	StoredCredential,
};
pub use managed_repository_runtime::{
	ManagedRepositoryReadiness, ManagedRepositoryUnavailableReason,
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
pub use quick_task::QuickTaskReadiness;
pub use supervised_validation::{
	ProtectedWorktreeFingerprint, ProtectedWorktreeStateProbe, SupervisedValidationEvidence,
	ValidationAcceptance, ValidationCancellation, ValidationCommandAuthority, ValidationRejection,
	ValidationSupervisionError, ValidationTermination, supervise_validation,
};
pub use websocket::{
	ActorCommandDeadlineClass, BoundServer, OwnedTaskIdentity, OwnedTaskKind, ProtocolServer,
	ServerConfig, ServerError, SpawnId, TerminationPrimary, TerminationReceipt,
};

#[cfg(test)] use {tempfile as _, tokio_tungstenite as _};

/// The vNext service assembly selected by the `decodexd` composition root.
#[derive(Clone, Copy, Debug)]
pub struct ServiceComposition;
impl ServiceComposition {
	/// Initialize or upgrade the fixed bundled SQLite database.
	pub async fn initialize_local_database(root: DecodexRoot) -> Result<(), LocalDatabaseError> {
		bootstrap::initialize_local_database(root).await
	}

	/// Verify the fixed bundled SQLite database and migration ledger.
	pub async fn validate_local_database(root: DecodexRoot) -> Result<(), LocalDatabaseError> {
		bootstrap::validate_local_database(root).await
	}

	/// Acquire singleton authority, then bootstrap the platform-default typed root.
	pub async fn bootstrap_default() -> ServiceBootstrap {
		bootstrap::bootstrap_default().await
	}

	/// Acquire singleton authority, then bootstrap an explicit validated root.
	///
	/// The returned owner retains the one published listener and namespace lock.
	/// Dropping it without binding releases that capability after its services.
	pub async fn bootstrap(root: DecodexRoot) -> ServiceBootstrap {
		bootstrap::bootstrap(root).await
	}
}
