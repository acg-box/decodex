use super::{
	Arc, DashboardEventHub, JoinHandle, Mutex, OffsetDateTime, OperatorStatusSnapshot, Sender,
	SocketAddr, StateStore, TcpListener, eyre, json, mpsc, operator_snapshot_json_value,
	run_operator_run_activity_websocket_broadcasts, run_operator_state_endpoint, thread,
};

pub(crate) struct OperatorStateEndpoint {
	pub(crate) listen_address: SocketAddr,
	pub(crate) snapshot: Arc<Mutex<PublishedOperatorSnapshot>>,
	pub(in crate::orchestrator) dashboard_events: DashboardEventHub,
	pub(crate) control_requests: OperatorControlRequests,
	pub(crate) shutdown_tx: Sender<()>,
	pub(crate) activity_shutdown_tx: Sender<()>,
	pub(crate) server_thread: Option<JoinHandle<()>>,
	pub(crate) activity_thread: Option<JoinHandle<()>>,
}
impl OperatorStateEndpoint {
	pub(crate) fn start(
		listen_address: &str,
		state_store: Arc<StateStore>,
	) -> crate::prelude::Result<Self> {
		let listener = TcpListener::bind(listen_address).map_err(|error| {
			eyre::eyre!("Failed to bind operator state endpoint on `{listen_address}`: {error}")
		})?;
		let listen_address = listener.local_addr().map_err(|error| {
			eyre::eyre!(
				"Failed to resolve operator state endpoint address for `{listen_address}`: {error}"
			)
		})?;

		listener
			.set_nonblocking(true)
			.map_err(|error| eyre::eyre!("Failed to configure operator state endpoint: {error}"))?;

		let snapshot = Arc::new(Mutex::new(PublishedOperatorSnapshot::default()));
		let dashboard_events = DashboardEventHub::default();
		let shared_snapshot = Arc::clone(&snapshot);
		let server_dashboard_events = dashboard_events.clone();
		let control_requests = OperatorControlRequests::default();
		let server_control_requests = control_requests.clone();
		let server_state_store = Arc::clone(&state_store);
		let (shutdown_tx, shutdown_rx) = mpsc::channel();
		let server_thread = thread::spawn(move || {
			run_operator_state_endpoint(
				listener,
				shared_snapshot,
				server_dashboard_events,
				server_control_requests,
				server_state_store,
				shutdown_rx,
			);
		});
		let activity_dashboard_events = dashboard_events.clone();
		let (activity_shutdown_tx, activity_shutdown_rx) = mpsc::channel();
		let activity_thread = thread::spawn(move || {
			run_operator_run_activity_websocket_broadcasts(
				state_store,
				activity_dashboard_events,
				activity_shutdown_rx,
			);
		});

		Ok(Self {
			listen_address,
			snapshot,
			dashboard_events,
			control_requests,
			shutdown_tx,
			activity_shutdown_tx,
			server_thread: Some(server_thread),
			activity_thread: Some(activity_thread),
		})
	}

	pub(crate) fn listen_address(&self) -> SocketAddr {
		self.listen_address
	}

	pub(crate) fn publish_snapshot(
		&self,
		snapshot: &OperatorStatusSnapshot,
	) -> crate::prelude::Result<()> {
		let snapshot_value = operator_snapshot_json_value(snapshot)?;
		let snapshot_json = serde_json::to_vec(&snapshot_value)?;
		let last_publish_unix_epoch = OffsetDateTime::now_utc().unix_timestamp();
		let mut guard = self
			.snapshot
			.lock()
			.map_err(|error| eyre::eyre!("Operator state snapshot lock poisoned: {error}"))?;

		*guard = PublishedOperatorSnapshot {
			snapshot_json: Some(snapshot_json),
			last_publish_unix_epoch: Some(last_publish_unix_epoch),
		};

		drop(guard);

		self.dashboard_events.broadcast(
			"snapshot",
			json!({
				"snapshotPublishedAtUnixEpoch": last_publish_unix_epoch,
				"snapshot": snapshot_value,
			}),
		);

		Ok(())
	}

	pub(crate) fn drain_linear_scan_requests(
		&self,
	) -> crate::prelude::Result<Vec<OperatorLinearScanRequest>> {
		self.control_requests.drain_linear_scan_requests()
	}
}

impl Drop for OperatorStateEndpoint {
	fn drop(&mut self) {
		let _ = self.shutdown_tx.send(());
		let _ = self.activity_shutdown_tx.send(());

		if let Some(server_thread) = self.server_thread.take() {
			let _ = server_thread.join();
		}
		if let Some(activity_thread) = self.activity_thread.take() {
			let _ = activity_thread.join();
		}
	}
}

#[derive(Clone, Default)]
pub(crate) struct PublishedOperatorSnapshot {
	pub(crate) snapshot_json: Option<Vec<u8>>,
	pub(crate) last_publish_unix_epoch: Option<i64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct OperatorLinearScanRequest {
	pub(crate) project_id: Option<String>,
}

#[derive(Clone, Default)]
pub(crate) struct OperatorControlRequests {
	pub(crate) linear_scan_requests: Arc<Mutex<Vec<OperatorLinearScanRequest>>>,
}
impl OperatorControlRequests {
	pub(crate) fn request_linear_scan(
		&self,
		project_id: Option<String>,
	) -> crate::prelude::Result<()> {
		let mut requests = self
			.linear_scan_requests
			.lock()
			.map_err(|error| eyre::eyre!("Operator control request lock poisoned: {error}"))?;

		requests.push(OperatorLinearScanRequest { project_id });

		Ok(())
	}

	pub(crate) fn drain_linear_scan_requests(
		&self,
	) -> crate::prelude::Result<Vec<OperatorLinearScanRequest>> {
		let mut requests = self
			.linear_scan_requests
			.lock()
			.map_err(|error| eyre::eyre!("Operator control request lock poisoned: {error}"))?;

		Ok(requests.drain(..).collect())
	}
}
