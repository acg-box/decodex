//! `decodexd` lifecycle assembly and the loopback V1 connection owner.

mod application;
mod websocket;

pub use application::{Application, ApplicationPublication, FoundationApplication};
pub use decodex_protocol::ServerId;
pub use websocket::{BoundServer, ProtocolServer, ServerConfig, ServerError};

#[cfg(test)] use tokio_tungstenite as _;

use decodex_codex::CodexAdapter;
use decodex_core::FoundationStatus;
use decodex_postgres::PostgresStore;
use decodex_protocol::{CURRENT_VERSION, ServiceAnnouncement};

/// The vNext service assembly selected by the `decodexd` composition root.
#[derive(Clone, Copy, Debug)]
pub struct ServiceComposition {
	store: PostgresStore,
	codex: CodexAdapter,
}
impl ServiceComposition {
	/// Select the accepted adapters without enabling either unavailable implementation.
	pub const fn foundation() -> Self {
		Self { store: PostgresStore::unavailable(), codex: CodexAdapter::unavailable() }
	}

	/// Validate and describe the assembled service.
	pub fn boot(self) -> ServiceAnnouncement {
		ServiceAnnouncement {
			version: CURRENT_VERSION,
			foundation: FoundationStatus::assemble(&self.store, &self.codex),
		}
	}

	/// Compose the sole V1 server root while later adapters remain unavailable.
	pub fn protocol_server(
		self,
		server_id: ServerId,
		config: ServerConfig,
	) -> ProtocolServer<FoundationApplication> {
		let _ = self.boot();

		ProtocolServer::new(server_id, FoundationApplication, config)
	}
}

#[cfg(test)]
mod tests {
	use crate::ServiceComposition;
	use decodex_core::Availability;
	use decodex_protocol::CURRENT_VERSION;

	#[test]
	fn service_boot_wires_v1_without_enabling_unimplemented_adapters() {
		let announcement = ServiceComposition::foundation().boot();

		assert_eq!(announcement.version, CURRENT_VERSION);
		assert_eq!(
			announcement.foundation.product_state(),
			Availability::Unavailable { reason: decodex_postgres::NOT_IMPLEMENTED }
		);
		assert_eq!(
			announcement.foundation.conversation_runtime(),
			Availability::Unavailable { reason: decodex_codex::NOT_IMPLEMENTED }
		);
		assert!(!announcement.foundation.is_operational());
	}
}
