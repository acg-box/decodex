//! `decodexd` lifecycle assembly and the same-UID V2.0 local connection owner.
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
#[cfg(target_os = "macos")] mod daemon_wrapper;
#[expect(dead_code, reason = "sealed until the accepted GitHub-effect composition owner")]
pub(crate) mod github_effects;
mod host_credentials;
mod local_account_authority;
mod managed_repository_executor;
mod managed_repository_runtime;
mod managed_repository_saga;
mod process_platform;
mod process_supervisor;
mod provider_attempt_service;
mod quick_task;
#[expect(dead_code, reason = "sealed until a separate live-routing authority enables dispatch")]
mod routing_orchestration;
mod supervised_validation;
mod websocket;
mod work_item_board;

pub use account_service::{
	AccountInspection, AccountLifecycleError, AccountSelectionFailure, AccountSelectionResult,
	AccountService, ChatgptTokenProjection, CredentialRefreshError, StartupAccountReconciliation,
};
pub use application::{Application, ApplicationEventPublication, ApplicationPublication};
pub use bootstrap::{
	CurrentAuthorityValidationError, LatestSchemaBootstrapError, ServiceBootstrap,
};
pub use decodex_core::DecodexRoot;
pub use decodex_protocol::ServerId;
#[cfg(target_os = "macos")] pub use host_credentials::MacosKeychainCredentialStore;
pub use host_credentials::{
	CredentialSecretBundle, CredentialStoreError, HostCredentialStore, StoredCredential,
};
pub use local_account_authority::LocalAccountAuthorityRestoreReport;
pub use managed_repository_runtime::{
	ManagedRepositoryReadiness, ManagedRepositoryUnavailableReason,
};
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
	/// Restore the complete local account authority while retaining the stopped-daemon boundary.
	#[doc(hidden)]
	pub async fn restore_local_account_authority<R: std::io::Read>(
		root: DecodexRoot,
		schema_owner_user: String,
		schema_owner_credential_env_var: Option<String>,
		input: R,
	) -> LocalAccountAuthorityRestoreReport {
		local_account_authority::restore_local_account_authority(
			root,
			schema_owner_user,
			schema_owner_credential_env_var,
			input,
		)
		.await
	}

	/// Create the latest schema once on an empty target under explicit operator authority.
	pub async fn bootstrap_latest_schema(
		root: DecodexRoot,
		schema_owner_user: String,
		schema_owner_credential_env_var: Option<String>,
	) -> Result<(), LatestSchemaBootstrapError> {
		bootstrap::bootstrap_latest_schema(root, schema_owner_user, schema_owner_credential_env_var)
			.await
	}

	/// Verify the exact latest catalog and configured authority with the runtime identity only.
	pub async fn validate_current_authority(
		root: DecodexRoot,
	) -> Result<(), CurrentAuthorityValidationError> {
		bootstrap::validate_current_authority(root).await
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
