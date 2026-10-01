//! Same-UID transport over the real command owner; provider observations remain fixture-owned.

use tokio::{net::TcpListener, time};

use crate::{
	Application, ProtocolServer, ServerConfig, account_launch,
	application::account_nudge::{
		native_tests,
		native_tests::{
			AccountId, AccountObservationService, AccountRecoveryAction,
			AccountRecoveryNudgeStatus, AccountRecoveryResult, ApplicationPublication,
			CommandEnvelope, CommandError, DecodexRoot, Duration, EventPayload, IdempotencyKey,
			ResultPayload, ServiceApplication,
		},
	},
};
use decodex_core::LocalTrustPolicy;
use decodex_protocol::{
	AccountCommandResponse, AccountRecoveryNudgeResult, AccountRecoveryPreparation,
	LocalTransportAuthority, QueryEnvelope, QueryResultPayload, RetainedSession,
	RetainedSessionConfig, ServerId, SessionCancellation, SessionDelivery, SnapshotItem,
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

	async fn snapshot(&self) -> Vec<SnapshotItem> {
		self.0.snapshot().await
	}

	fn command_independent_snapshot(&self) -> Option<Vec<SnapshotItem>> {
		self.0.command_independent_snapshot()
	}

	async fn execute<'a>(
		&'a self,
		command: &'a CommandEnvelope,
	) -> Result<ApplicationPublication, CommandError> {
		self.0.execute(command).await
	}

	async fn query<'a>(&'a self, query: &'a QueryEnvelope) -> QueryResultPayload {
		self.0.query(query).await
	}
}

pub(super) async fn qualify(
	app: ServiceApplication,
	root: &DecodexRoot,
	source: &AccountRecoveryResult,
	observations: &AccountObservationService,
	listener: &TcpListener,
) {
	observations
		.cache_recovery_fixture(
			AccountId::new(source.account_id.as_str()).expect("account"),
			source.account_revision.0 as i64,
			native_tests::fixture_usage(),
			"workspace-fixture",
			"user-fixture",
		)
		.await;

	let authority = LocalTransportAuthority::new(
		root.paths(),
		LocalTrustPolicy::SameUid,
		Some(unsafe { libc::geteuid() }),
	)
	.expect("same-UID authority");
	let server_id = ServerId::new("20000000-0000-4000-8000-000000000001").expect("server identity");
	let config = RetainedSessionConfig::new(authority, server_id.clone());
	let mut server = ProtocolServer::new(server_id, ObservedFixture(app), ServerConfig::default())
		.bind(
			LocalTransportAuthority::new(
				root.paths(),
				LocalTrustPolicy::SameUid,
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
		AccountRecoveryPreparation::Ready {
			destination: decodex_protocol::AccountRecoveryDestination::RequestCredits,
			..
		}
	));

	let key =
		IdempotencyKey::new("native-notification-through-socket").expect("explicit operation");
	let (response, ()) = time::timeout(Duration::from_secs(30), async {
		tokio::join!(
			client.send_recovery_nudge(
				current.clone(),
				AccountRecoveryAction::NotifyOwner,
				key.clone()
			),
			account_launch::serve_native_nudge_fixture(
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
		if matches!(&*result, ResultPayload::AccountRecoveryNudge { status:AccountRecoveryNudgeStatus::Sent, operation_key, .. } if operation_key == &key))
	);

	let SessionDelivery::Event { event, confirmation } =
		time::timeout(Duration::from_secs(5), peer.next())
			.await
			.expect("peer event deadline")
			.expect("peer event")
	else {
		panic!("account notification publication")
	};

	assert!(
		matches!(event.payload, EventPayload::AccountRecoveryNudge { status:AccountRecoveryNudgeStatus::Sent, operation_key, .. } if operation_key == key)
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
		matches!(outcome, AccountRecoveryNudgeResult::Found(operation) if operation.outcome == AccountRecoveryNudgeStatus::Sent && operation.operation_key == key)
	);
	assert!(
		matches!(client.send_recovery_nudge(current, AccountRecoveryAction::NotifyOwner, key).await.expect("same-key retry"),
		AccountCommandResponse::Applied { result, .. } if matches!(*result, ResultPayload::AccountRecoveryNudge {status:AccountRecoveryNudgeStatus::Sent,..}))
	);
	assert!(
		time::timeout(Duration::from_millis(300), listener.accept()).await.is_err(),
		"wire replay must not start native work"
	);

	peer.close().await.expect("peer close");

	assert!(server.shutdown().await.expect("server shutdown").is_success());
}
