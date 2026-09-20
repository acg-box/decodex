//! Explicit desired-state archive restoration; native Codex owns persistence.
use super::{ChiefCoordinator, ChiefError, ClientError, json};
use decodex_codex::app_server_client::ThreadArchiveState;

impl ChiefCoordinator {
	/// Restore only the selected native identity. A restored task may subsequently
	/// consume its already accepted queue through the ordinary host wake path.
	pub async fn restore_archived_thread(
		&mut self,
		work: &str,
		thread: &str,
	) -> Result<(), ChiefError> {
		let reject = || {
			ChiefError::Rejected(
				"The task binding or archive state changed. Refresh it before restoring.".into(),
			)
		};
		let generation = self.native_generation.as_ref().map(|g| g.as_str().to_owned());
		if !self.store.chief_thread_is_owned(work.into(), thread.into(), generation.clone()).await?
		{
			return Err(reject());
		}
		let state = self.client.thread_archive_state(thread).await.map_err(|_| reject())?;
		if !matches!(state, ThreadArchiveState::Active | ThreadArchiveState::Archived) {
			return Err(reject());
		}
		if !self.store.chief_thread_is_owned(work.into(), thread.into(), generation.clone()).await?
		{
			return Err(reject());
		}
		if state == ThreadArchiveState::Active {
			self.reconcile_restored_turn(work, thread).await;
			return Ok(());
		}
		self.loaded_threads.remove(thread);
		let submitted = self.client.thread_unarchive(thread).await;
		// Even an explicit native error may mean another client restored it first.
		// Only desired-state readback can resolve an uncertain mutation.
		let observed = self.client.thread_archive_state(thread).await;
		if !self.store.chief_thread_is_owned(work.into(), thread.into(), generation).await? {
			return Err(ChiefError::UnknownDispatch);
		}
		match observed {
			Ok(ThreadArchiveState::Active) => {
				self.reconcile_restored_turn(work, thread).await;
				Ok(())
			},
			Ok(ThreadArchiveState::Archived)
				if matches!(submitted, Err(ClientError::Remote(_))) =>
				Err(reject()),
			_ => Err(ChiefError::UnknownDispatch),
		}
	}
}

impl ChiefCoordinator {
	// An archive can interrupt a turn while this client is disconnected. Recover
	// only positive terminal history; restoration itself never proves completion.
	async fn reconcile_restored_turn(&mut self, work: &str, thread: &str) {
		let _ = tokio::time::timeout(std::time::Duration::from_secs(8), async {
			let item = self.store.get_chief_work_item(work.into()).await?;
			let Some(turn) = item.active_turn_id.as_ref() else {
				return Ok::<(), ChiefError>(());
			};
			if item.codex_thread_id.as_deref() != Some(thread) {
				return Ok(());
			}
			let history = self.client.thread_read_turn(thread, turn).await?;
			if history["thread"]["id"].as_str() != Some(thread) {
				return Ok(());
			}
			let Some(exact_turn) = history["thread"]["turns"]
				.as_array()
				.and_then(|turns| turns.iter().find(|entry| entry["id"].as_str() == Some(turn)))
				.cloned()
			else {
				return Ok(());
			};
			if !matches!(
				exact_turn["status"].as_str(),
				Some("completed" | "failed" | "interrupted")
			) {
				return Ok(());
			}
			if !self
				.store
				.chief_thread_is_owned(
					work.into(),
					thread.into(),
					self.native_generation.as_ref().map(|g| g.as_str().into()),
				)
				.await?
			{
				return Ok(());
			}
			self.record_terminal(json!({"threadId":thread,"turn":exact_turn}), Ok(history), false)
				.await
		})
		.await;
	}
}
