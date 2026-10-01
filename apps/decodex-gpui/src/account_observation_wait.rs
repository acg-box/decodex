//! A cancellable independent socket wait; never blocks the retained ingress reader.
use std::{
	future::{self, Future},
	pin::Pin,
	time::Duration,
};

use tokio::time;

use decodex_protocol::{
	AccountClient, AccountObservationSignal, ClientFailure, QueryEnvelope, QueryPayload,
};

pub(super) type Wait = Pin<Box<dyn Future<Output = Outcome> + Send>>;

type Outcome = (QueryEnvelope, Result<AccountObservationSignal, ClientFailure>);

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
			time::sleep(Duration::from_secs(1)).await;
		}

		(query, result)
	})
}

pub(super) async fn poll(wait: &mut Option<Wait>) -> Outcome {
	match wait {
		Some(wait) => wait.await,
		None => future::pending().await,
	}
}

#[cfg(test)]
mod tests {
	use std::{
		future,
		sync::{
			Arc,
			atomic::{AtomicUsize, Ordering},
		},
	};

	use crate::client_lifecycle::account_observation::{self, Wait};

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

			future::pending().await
		}));

		for _ in 0..10 {
			tokio::select! {
				biased;

				_ = account_observation::poll(&mut wait) => panic!("wait must remain pending"),
				() = future::ready(()) => {},
			}
		}

		assert_eq!(starts.load(Ordering::SeqCst), 1);
		assert_eq!(drops.load(Ordering::SeqCst), 0);

		drop(wait);

		assert_eq!(drops.load(Ordering::SeqCst), 1);
	}
}
