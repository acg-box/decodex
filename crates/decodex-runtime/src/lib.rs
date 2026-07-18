//! `decodexd` lifecycle assembly and the loopback V1 connection owner.
//!
//! Account-process composition remains crate-private and cannot be called by a product root:
//!
//! ```compile_fail
//! use decodex_runtime::ManualAccountLauncher;
//! ```

#[expect(dead_code, reason = "dormant until a later explicit product authority enables routing")]
mod account_launch;
mod application;
mod bootstrap;
#[expect(dead_code, reason = "sealed until the accepted GitHub-effect composition owner")]
pub(crate) mod github_effects;
mod managed_repository_executor;
mod managed_repository_saga;
mod supervised_validation;
mod websocket;

pub use application::{Application, ApplicationPublication};
pub use bootstrap::ServiceBootstrap;
pub use decodex_protocol::ServerId;
pub use managed_repository_saga::{
	ManagedRepositoryEffectPort, ManagedRepositoryEffectSaga, ManagedRepositoryRestartOutcome,
	ManagedRepositorySagaOutcome, RepositoryDispatchFailure, RepositoryDispatchObservation,
	RepositoryReadbackEvidence,
};
pub use supervised_validation::{
	ProtectedWorktreeFingerprint, ProtectedWorktreeStateProbe, SupervisedValidationEvidence,
	ValidationAcceptance, ValidationCancellation, ValidationCommandAuthority, ValidationRejection,
	ValidationSupervisionError, ValidationTermination, supervise_validation,
};
pub use websocket::{BoundServer, ProtocolServer, ServerConfig, ServerError};

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
