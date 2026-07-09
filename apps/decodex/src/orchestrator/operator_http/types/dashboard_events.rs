use std::{
	sync::{
		Arc, Mutex,
		mpsc::{self, Receiver, RecvTimeoutError, Sender},
	},
	time::Duration,
};

use serde_json::Value;

use crate::{
	orchestrator::operator_http::{assets::DASHBOARD_MAX_WEBSOCKET_CLIENTS, dashboard},
	prelude::eyre,
};

#[derive(Clone, Default)]
pub(crate) struct DashboardEventHub {
	clients: Arc<Mutex<Vec<DashboardClientHandle>>>,
	last_run_activity: Arc<Mutex<Option<DashboardBroadcastEvent>>>,
	next_client_id: Arc<Mutex<u64>>,
}
impl DashboardEventHub {
	pub(crate) fn subscribe(&self) -> crate::prelude::Result<DashboardClientRegistration> {
		let (event_tx, event_rx) = mpsc::channel();
		let mut clients = self
			.clients
			.lock()
			.map_err(|error| eyre::eyre!("Dashboard event client lock poisoned: {error}"))?;

		if clients.len() >= DASHBOARD_MAX_WEBSOCKET_CLIENTS {
			eyre::bail!(
				"Dashboard websocket client limit reached ({DASHBOARD_MAX_WEBSOCKET_CLIENTS})."
			);
		}

		let mut next_client_id = self
			.next_client_id
			.lock()
			.map_err(|error| eyre::eyre!("Dashboard event client id lock poisoned: {error}"))?;
		let id = *next_client_id;

		*next_client_id = next_client_id.saturating_add(1);

		clients.push(DashboardClientHandle { id, sender: event_tx });

		Ok(DashboardClientRegistration {
			id,
			receiver: event_rx,
			clients: Arc::clone(&self.clients),
		})
	}

	pub(crate) fn broadcast(&self, event_type: &'static str, payload: Value) {
		let event = DashboardBroadcastEvent { event_type, payload };

		if event_type == "runActivity"
			&& let Ok(mut last_run_activity) = self.last_run_activity.lock()
		{
			*last_run_activity = Some(event.clone());
		}

		let Ok(mut clients) = self.clients.lock() else {
			tracing::warn!(
				"Skipped operator event broadcast because the client list lock is poisoned."
			);

			return;
		};

		clients.retain(|client| client.sender.send(event.clone()).is_ok());
	}

	pub(crate) fn has_clients(&self) -> bool {
		self.clients.lock().is_ok_and(|clients| !clients.is_empty())
	}

	pub(crate) fn cached_run_activity_event(
		&self,
		subscription: &DashboardClientSubscription,
	) -> Option<DashboardBroadcastEvent> {
		self.last_run_activity.lock().ok().and_then(|event| {
			event
				.as_ref()
				.and_then(|event| dashboard::dashboard_event_for_subscription(event, subscription))
		})
	}

	#[cfg(test)]
	pub(crate) fn close_clients_for_test(&self) {
		if let Ok(mut clients) = self.clients.lock() {
			clients.clear();
		}
	}

	#[cfg(test)]
	pub(crate) fn client_count_for_test(&self) -> usize {
		self.clients.lock().map(|clients| clients.len()).unwrap_or_default()
	}
}

pub(crate) struct DashboardClientRegistration {
	id: u64,
	receiver: Receiver<DashboardBroadcastEvent>,
	clients: Arc<Mutex<Vec<DashboardClientHandle>>>,
}
impl DashboardClientRegistration {
	pub(crate) fn recv_timeout(
		&self,
		timeout: Duration,
	) -> std::result::Result<DashboardBroadcastEvent, RecvTimeoutError> {
		self.receiver.recv_timeout(timeout)
	}
}

impl Drop for DashboardClientRegistration {
	fn drop(&mut self) {
		if let Ok(mut clients) = self.clients.lock() {
			clients.retain(|client| client.id != self.id);
		}
	}
}

#[derive(Clone, Debug)]
pub(crate) struct DashboardBroadcastEvent {
	pub(crate) event_type: &'static str,
	pub(crate) payload: Value,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct DashboardClientSubscription {
	pub(crate) project_id: Option<String>,
	pub(crate) issue_id: Option<String>,
	pub(crate) run_id: Option<String>,
}

pub(crate) struct DashboardRunActivityEvent {
	pub(crate) fingerprint: Vec<u8>,
	pub(crate) event: DashboardBroadcastEvent,
}

#[derive(Debug)]
pub(in crate::orchestrator::operator_http) struct DashboardClientHandle {
	id: u64,
	sender: Sender<DashboardBroadcastEvent>,
}
