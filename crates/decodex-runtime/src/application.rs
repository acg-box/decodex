//! Application-service seam used by the transport without exposing infrastructure.

use std::future::{self, Future};

use decodex_codex::CodexAdapter;
use decodex_core::{Availability, ProductState};
use decodex_postgres::{BootstrapFailure, PostgresStore};
use decodex_protocol::{
	Channel, CommandEnvelope, CommandError, CommandPayload, DoctorCheck, DoctorComponent,
	DoctorIssue, DoctorReport, DoctorStatus, EntityId, EntityRevision, EventPayload, QueryEnvelope,
	QueryPayload, QueryResultPayload, ResultPayload, SnapshotItem, WireText,
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

	/// Execute one fresh observation without mutation receipts or replay semantics.
	fn query<'a>(
		&'a self,
		query: &'a QueryEnvelope,
	) -> impl Future<Output = QueryResultPayload> + Send + 'a;
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

#[derive(Clone)]
pub(crate) enum ProductStore {
	Available(PostgresStore),
	Unavailable { reason: &'static str },
}
impl ProductStore {
	async fn database_status(&self, unavailable: DoctorStatus) -> DoctorStatus {
		let Self::Available(store) = self else {
			return unavailable;
		};

		match store.revalidate().await {
			Ok(()) => DoctorStatus::Ready,
			Err(error) => DoctorStatus::Unavailable(match error.bootstrap_failure() {
				BootstrapFailure::Authentication => DoctorIssue::Authentication,
				BootstrapFailure::Unreachable => DoctorIssue::DatabaseUnreachable,
				BootstrapFailure::Incompatible => DoctorIssue::DatabaseIncompatible,
				BootstrapFailure::UnsafeAuthority => DoctorIssue::UnsafeDatabaseAuthority,
				BootstrapFailure::UnsafeHostPath => DoctorIssue::UnsafeHostPath,
			}),
		}
	}
}
impl ProductState for ProductStore {
	fn availability(&self) -> Availability {
		match self {
			Self::Available(store) => store.availability(),
			Self::Unavailable { reason } => Availability::Unavailable { reason },
		}
	}
}

/// Runtime-owned application service retaining the selected adapter and doctor report.
#[derive(Clone)]
pub(crate) struct ServiceApplication {
	store: ProductStore,
	_codex: CodexAdapter,
	doctor: DoctorReport,
}
impl ServiceApplication {
	pub(crate) const fn new(
		store: ProductStore,
		codex: CodexAdapter,
		doctor: DoctorReport,
	) -> Self {
		Self { store, _codex: codex, doctor }
	}

	async fn refreshed_doctor(&self) -> DoctorReport {
		let previous_database = self
			.doctor
			.check(DoctorComponent::Database)
			.expect("the closed doctor report includes PostgreSQL")
			.status;
		let database = self.store.database_status(previous_database).await;
		let checks = self
			.doctor
			.checks()
			.iter()
			.map(|check| {
				if check.component == DoctorComponent::Database {
					DoctorCheck::new(DoctorComponent::Database, database)
				} else {
					*check
				}
			})
			.collect();

		DoctorReport::new(self.doctor.server_id().clone(), self.doctor.version(), checks)
			.expect("refresh preserves the bounded closed doctor shape")
	}
}
impl Application for ServiceApplication {
	fn snapshot(&self) -> impl Future<Output = Vec<SnapshotItem>> + Send {
		future::ready(vec![SnapshotItem::SystemState {
			entity_id: EntityId::new("decodexd").expect("service entity ID is bounded"),
			revision: EntityRevision(0),
			status: WireText::new("typed doctor/status is available through the daemon protocol")
				.expect("service status is bounded"),
		}])
	}

	async fn execute<'a>(
		&'a self,
		command: &'a CommandEnvelope,
	) -> Result<ApplicationPublication, CommandError> {
		match command.payload {
			CommandPayload::RefreshSystemObservation { .. } =>
				Err(CommandError::ApplicationUnavailable {
					message: WireText::new(
						"foundation refresh is superseded by typed doctor/status",
					)
					.expect("service message is bounded"),
				}),
		}
	}

	async fn query<'a>(&'a self, query: &'a QueryEnvelope) -> QueryResultPayload {
		match query.payload {
			QueryPayload::GetDoctorStatus =>
				QueryResultPayload::DoctorStatus(self.refreshed_doctor().await),
		}
	}
}
