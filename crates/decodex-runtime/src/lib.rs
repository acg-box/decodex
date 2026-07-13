//! `decodexd` lifecycle assembly.
//!
//! This owner wires the accepted adapters and validates the service boundary. It does
//! not open sockets, connect to PostgreSQL, or spawn Codex in XY-1265.

use decodex_codex::CodexAdapter;
use decodex_core::FoundationStatus;
use decodex_postgres::PostgresStore;
use decodex_protocol::{ProtocolVersion, ServiceAnnouncement};

/// The vNext service assembly selected by the `decodexd` composition root.
#[derive(Clone, Copy, Debug)]
pub struct ServiceComposition {
	store: PostgresStore,
	codex: CodexAdapter,
}
impl ServiceComposition {
	/// Select the accepted vNext adapters without selecting a transport or endpoint.
	pub const fn foundation() -> Self {
		Self { store: PostgresStore::unavailable(), codex: CodexAdapter::unavailable() }
	}

	/// Validate and describe the assembled service without enabling later slices.
	pub fn boot(self) -> ServiceAnnouncement {
		ServiceAnnouncement {
			version: ProtocolVersion::V1,
			foundation: FoundationStatus::assemble(&self.store, &self.codex),
		}
	}
}

#[cfg(test)]
mod tests {
	use crate::ServiceComposition;
	use decodex_core::Availability;
	use decodex_protocol::ProtocolVersion;

	#[test]
	fn service_boot_wires_v1_without_enabling_unimplemented_adapters() {
		let announcement = ServiceComposition::foundation().boot();

		assert_eq!(announcement.version, ProtocolVersion::V1);
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
