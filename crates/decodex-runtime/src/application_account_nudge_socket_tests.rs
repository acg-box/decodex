//! Same-UID transport over the real command owner; provider observations remain fixture-owned.
use super::*;
use crate::{Application, ProtocolServer, ServerConfig};
use decodex_protocol::{
	AccountCommandResponse, AccountRecoveryNudgeResult, LocalTransportAuthority, RetainedSession,
	RetainedSessionConfig, ServerId, SessionCancellation, SessionDelivery,
};

// The fixture owns observation timing. Delegate real command/query and shutdown behavior,
// but do not launch background provider refresh against accounts without a provider adapter.
struct ObservedFixture(ServiceApplication);
impl Application for ObservedFixture {
	fn begin_shutdown(&self) {
		self.0.begin_shutdown();
	}

	async fn wait_for_shutdown(&self) {
		self.0.wait_for_shutdown().await;
	}

	async fn snapshot(&self) -> Vec<decodex_protocol::SnapshotItem> {
		self.0.snapshot().await
	}

	fn command_independent_snapshot(&self) -> Option<Vec<decodex_protocol::SnapshotItem>> {
		self.0.command_independent_snapshot()
	}

	async fn execute<'a>(
		&'a self,
		command: &'a CommandEnvelope,
	) -> Result<ApplicationPublication, CommandError> {
		self.0.execute(command).await
	}

	async fn query<'a>(
		&'a self,
		query: &'a decodex_protocol::QueryEnvelope,
	) -> decodex_protocol::QueryResultPayload {
		self.0.query(query).await
	}
}

pub(super) async fn qualify(
	app: ServiceApplication,
	root: &DecodexRoot,
	source: &AccountRecoveryResult,
	observations: &AccountObservationService,
	listener: &tokio::net::TcpListener,
) {
	observations
		.cache_recovery_fixture(
			AccountId::new(source.account_id.as_str()).expect("account"),
			source.account_revision.0 as i64,
			fixture_usage(),
			"workspace-fixture",
			"user-fixture",
		)
		.await;
	let authority = LocalTransportAuthority::new(
		root.paths(),
		decodex_core::LocalTrustPolicy::SameUid,
		Some(unsafe { libc::geteuid() }),
	)
	.expect("same-UID authority");
	let server_id = ServerId::new("20000000-0000-4000-8000-000000000001").expect("server identity");
	let config = RetainedSessionConfig::new(authority, server_id.clone());
	let mut server = ProtocolServer::new(server_id, ObservedFixture(app), ServerConfig::default())
		.bind(
			LocalTransportAuthority::new(
				root.paths(),
				decodex_core::LocalTrustPolicy::SameUid,
				Some(unsafe { libc::geteuid() }),
			)
			.expect("server authority"),
		)
		.await
		.expect("bind actual transport");
	let client = config.account_client();
	let mut peer = RetainedSession::connect(config, None, SessionCancellation::new())
		.await
		.expect("peer admission");
	let SessionDelivery::Snapshot { confirmation, .. } = peer.next().await.expect("peer snapshot")
	else {
		panic!("initial snapshot")
	};
	peer.confirm_applied(confirmation).expect("snapshot admission");
	let current = client
		.recovery(source.account_id.clone(), source.account_revision)
		.await
		.expect("recovery query");
	assert!(matches!(
		client
			.prepare_recovery(current.clone(), AccountRecoveryAction::NotifyOwner)
			.await
			.expect("prepare query"),
		decodex_protocol::AccountRecoveryPreparation::Ready {
			destination: decodex_protocol::AccountRecoveryDestination::RequestCredits,
			..
		}
	));
	let key =
		IdempotencyKey::new("native-notification-through-socket").expect("explicit operation");
	let (response, ()) = tokio::time::timeout(Duration::from_secs(30), async {
		tokio::join!(
			client.send_recovery_nudge(
				current.clone(),
				AccountRecoveryAction::NotifyOwner,
				key.clone()
			),
			crate::account_launch::serve_native_nudge_fixture(
				listener,
				"200 OK",
				decodex_codex::app_server_client::AccountNudgeCreditType::Credits
			)
		)
	})
	.await
	.expect("bounded admitted command");
	assert!(
		matches!(response.expect("verified command response"), AccountCommandResponse::Applied { result, .. }
		if matches!(&*result, ResultPayload::AccountRecoveryNudge { status:Status::Sent, operation_key, .. } if operation_key == &key))
	);
	let SessionDelivery::Event { event, confirmation } =
		tokio::time::timeout(Duration::from_secs(5), peer.next())
			.await
			.expect("peer event deadline")
			.expect("peer event")
	else {
		panic!("account notification publication")
	};
	assert!(
		matches!(event.payload, EventPayload::AccountRecoveryNudge { status:Status::Sent, operation_key, .. } if operation_key == key)
	);
	peer.confirm_applied(confirmation).expect("peer applied event");
	let outcome = client
		.recovery_nudge_status(
			source.account_id.clone(),
			AccountRecoveryAction::NotifyOwner,
			Some(key.clone()),
		)
		.await
		.expect("status readback");
	assert!(
		matches!(outcome, AccountRecoveryNudgeResult::Found(operation) if operation.outcome == Status::Sent && operation.operation_key == key)
	);
	assert!(
		matches!(client.send_recovery_nudge(current, AccountRecoveryAction::NotifyOwner, key).await.expect("same-key retry"),
		AccountCommandResponse::Applied { result, .. } if matches!(*result, ResultPayload::AccountRecoveryNudge {status:Status::Sent,..}))
	);
	assert!(
		tokio::time::timeout(Duration::from_millis(300), listener.accept()).await.is_err(),
		"wire replay must not start native work"
	);
	peer.close().await.expect("peer close");
	assert!(server.shutdown().await.expect("server shutdown").is_success());
}
