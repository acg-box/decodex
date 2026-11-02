//! Private routes for ephemeral structured requests on one native connection.
use super::{ClientError, ServerEvent, ServerRequests};
use std::{
	collections::BTreeMap,
	sync::{Arc, Mutex},
};
use tokio::sync::mpsc;

type Routes = BTreeMap<String, Option<mpsc::Sender<ServerEvent>>>;

#[derive(Clone, Default)]
pub(super) struct TemporaryEvents(Arc<Mutex<Routes>>);
impl TemporaryEvents {
	pub(super) fn register(
		&self,
		thread: &str,
	) -> Result<mpsc::Receiver<ServerEvent>, ClientError> {
		let mut routes = self.0.lock().map_err(|_| ClientError::Closed)?;
		if routes.len() >= 32 || routes.contains_key(thread) {
			return Err(ClientError::CapacityExceeded);
		}
		let (send, receive) = mpsc::channel(64);
		routes.insert(thread.into(), Some(send));
		Ok(receive)
	}

	pub(super) fn remove(&self, thread: &str) {
		if let Ok(mut routes) = self.0.lock() {
			routes.remove(thread);
		}
	}

	pub(super) fn route(
		&self,
		event: ServerEvent,
		requests: &ServerRequests,
	) -> Result<Option<ServerEvent>, ClientError> {
		let thread = match &event {
			ServerEvent::Notification { params, .. } | ServerEvent::Request { params, .. } =>
				params["threadId"].as_str(),
			_ => None,
		};
		let mut routes = self.0.lock().map_err(|_| ClientError::Closed)?;
		if let Some(thread) = thread
			&& let Some(route) = routes.get(thread)
		{
			requests.observe_thread_state(&event);
			let thread = thread.to_owned();
			if let Some(route) = route
				&& route.try_send(event).is_err()
			{
				routes.insert(thread, None);
			}
			return Ok(None);
		}
		Ok(Some(event))
	}

	pub(super) fn close(&self, reason: &ClientError) {
		if let Ok(mut routes) = self.0.lock() {
			for route in std::mem::take(&mut *routes).into_values().flatten() {
				let _ = route.try_send(ServerEvent::Closed(reason.clone()));
			}
		}
	}
}

#[cfg(test)]
mod tests {
	use super::{ServerEvent, ServerRequests, TemporaryEvents};
	#[tokio::test]
	async fn overflowing_private_route_stays_private_until_cleanup() {
		let routes = TemporaryEvents::default();
		let requests = ServerRequests::default();
		let mut receiver = routes.register("temporary").unwrap();
		for _ in 0..66 {
			let event = ServerEvent::Notification {
				method: "item/completed".into(),
				params: serde_json::json!({"threadId":"temporary"}),
			};
			assert!(routes.route(event, &requests).unwrap().is_none());
		}
		while receiver.recv().await.is_some() {}
		assert!(routes.register("temporary").is_err());
		routes.remove("temporary");
		assert!(routes.register("temporary").is_ok());
	}
}
