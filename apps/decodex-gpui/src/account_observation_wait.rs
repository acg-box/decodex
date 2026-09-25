//! A cancellable independent socket wait; never blocks the retained ingress reader.
use decodex_protocol::{
	AccountClient, AccountObservationSignal, ClientFailure, QueryEnvelope, QueryPayload,
};
use std::{future::Future, pin::Pin};
type Outcome = (QueryEnvelope, Result<AccountObservationSignal, ClientFailure>);
pub(super) type Wait = Pin<Box<dyn Future<Output = Outcome> + Send>>;
pub(super) fn start(client: AccountClient, query: QueryEnvelope) -> Wait {
	Box::pin(async move {
		let QueryPayload::WaitForAccountObservation { after_generation, request_refresh } =
			query.payload
		else {
			unreachable!("observation dispatcher")
		};
		let result = if request_refresh == Some(true) {
			client.request_observation_refresh(after_generation).await
		} else {
			client.wait_for_observation(after_generation).await
		};
		if result.is_err() {
			tokio::time::sleep(std::time::Duration::from_secs(1)).await;
		}
		(query, result)
	})
}
pub(super) async fn poll(wait: &mut Option<Wait>) -> Outcome {
	match wait {
		Some(wait) => wait.await,
		None => std::future::pending().await,
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::sync::{
		Arc,
		atomic::{AtomicUsize, Ordering},
	};

	struct Lifetime(Arc<AtomicUsize>);
	impl Drop for Lifetime {
		fn drop(&mut self) {
			self.0.fetch_add(1, Ordering::SeqCst);
		}
	}

	#[tokio::test]
	async fn other_session_events_do_not_restart_wait_and_session_end_drops_it() {
		let starts = Arc::new(AtomicUsize::new(0));
		let drops = Arc::new(AtomicUsize::new(0));
		let entered = starts.clone();
		let dropped = drops.clone();
		let mut wait: Option<Wait> = Some(Box::pin(async move {
			entered.fetch_add(1, Ordering::SeqCst);
			let _lifetime = Lifetime(dropped);
			std::future::pending().await
		}));
		for _ in 0..10 {
			tokio::select! {
				biased;
				_ = poll(&mut wait) => panic!("wait must remain pending"),
				() = std::future::ready(()) => {},
			}
		}
		assert_eq!(starts.load(Ordering::SeqCst), 1);
		assert_eq!(drops.load(Ordering::SeqCst), 0);
		drop(wait);
		assert_eq!(drops.load(Ordering::SeqCst), 1);
	}
}
