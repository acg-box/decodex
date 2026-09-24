//! `decodex serve` lifecycle assembly and the same-UID V2.17 local connection owner.
//!
//! Account-process and routing composition remain crate-private. The ordinary Conversation owner
//! composes them without exporting raw process, routing, or provider-dispatch facades.

mod account_api;
mod account_import;
#[expect(dead_code, reason = "dormant until a later explicit product authority enables routing")]
mod account_launch;
mod account_login;
mod account_observation;
mod account_profile;
mod account_service;
mod application;
mod auth_projection;
mod bootstrap;
mod chief;
mod chief_capabilities;
mod chief_detail;
mod chief_guardian;
mod chief_hooks;
mod chief_host;
mod chief_install;
mod chief_integrations;
mod chief_live_settings;
mod chief_model_settings;
mod chief_native_goal;
mod chief_permissions;
mod chief_plugins;
mod chief_resources;
mod chief_usage_estimate;
mod chief_voice;
mod conversation;
mod dictation;
mod domain_packs;
mod host_credentials;
mod mcp_login;
mod native_agents;
mod native_config_warning;
mod process_platform;
mod process_supervisor;
mod provider_attempt_service;
mod routing_orchestration;
mod shared_auth_coordinator;
mod supervised_validation;
mod websocket;

pub use account_service::{
	AccountInspection, AccountLifecycleError, AccountSelectionFailure, AccountSelectionResult,
	AccountService, ChatgptTokenProjection, CredentialRefreshError, StartupAccountReconciliation,
};
pub use application::{Application, ApplicationEventPublication, ApplicationPublication};
pub use bootstrap::{LocalDatabaseError, ServiceBootstrap};
pub use chief::{ChiefConfig, ChiefCoordinator, ChiefError};
pub use conversation::ConversationReadiness;
pub use decodex_core::DecodexRoot;
pub use decodex_protocol::ServerId;
pub use host_credentials::{
	CredentialSecretBundle, CredentialStoreError, HostCredentialStore, SqliteCredentialStore,
	StoredCredential,
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

/// The vNext service assembly selected by the `decodex serve` composition root.
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
