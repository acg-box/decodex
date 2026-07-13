//! Application-service seam used by the transport without exposing infrastructure.

use std::future::{self, Future};

use decodex_protocol::{
	Channel, CommandEnvelope, CommandError, EntityId, EntityRevision, EventPayload, ResultPayload,
	SnapshotItem, WireText,
};

/// The only mutation/observation seam reachable from the WebSocket server.
///
/// PostgreSQL-backed services can implement this async owner in XY-1267 without moving
/// command execution into the transport.
pub trait Application: Send + Sync + 'static {
	/// Return a bounded small-state snapshot. Artifact bytes are not representable.
	fn snapshot(&self) -> impl Future<Output = Vec<SnapshotItem>> + Send;

	/// Execute one typed command under the application's revision policy.
	fn execute<'a>(
		&'a self,
		command: &'a CommandEnvelope,
	) -> impl Future<Output = Result<ApplicationPublication, CommandError>> + Send + 'a;
}

/// A successful application execution ready for result and event publication.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplicationPublication {
	/// Logical channel for the resulting event.
	pub channel: Channel,
	/// Stable identity of the changed entity.
	pub entity_id: EntityId,
	/// Entity revision after execution.
	pub entity_revision: EntityRevision,
	/// Typed success result returned to the caller.
	pub result: ResultPayload,
	/// Typed event published to connected sessions.
	pub event: EventPayload,
}

/// Honest application boundary while the PostgreSQL slice is unavailable.
#[derive(Clone, Copy, Debug, Default)]
pub struct FoundationApplication;
impl Application for FoundationApplication {
	fn snapshot(&self) -> impl Future<Output = Vec<SnapshotItem>> + Send {
		future::ready(vec![SnapshotItem::SystemState {
			entity_id: EntityId::new("decodexd").expect("foundation entity ID is bounded"),
			revision: EntityRevision(0),
			status: WireText::new("product state unavailable until XY-1267")
				.expect("foundation status is bounded"),
		}])
	}

	fn execute<'a>(
		&'a self,
		_command: &'a CommandEnvelope,
	) -> impl Future<Output = Result<ApplicationPublication, CommandError>> + Send + 'a {
		future::ready(Err(CommandError::ApplicationUnavailable {
			message: WireText::new("product state unavailable until XY-1267")
				.expect("foundation message is bounded"),
		}))
	}
}
