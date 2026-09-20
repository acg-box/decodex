use super::*;
use decodex_protocol::ChiefInstallState;

impl ChiefCoordinator {
	pub(super) async fn install_request_guard(
		&self,
		event: i64,
	) -> Result<decodex_codex::app_server_client::ServerRequestGuard, ChiefError> {
		let id = self
			.pending_requests
			.iter()
			.find_map(|(id, current)| (*current == event).then_some(id))
			.ok_or_else(|| ChiefError::Rejected("Installation request is no longer live".into()))?;
		let event = self.store.get_chief_inbox_event(event).await?;
		let payload: Value = serde_json::from_str(&event.payload)
			.map_err(|_| ChiefError::Rejected("Invalid stored request".into()))?;
		self.client
			.server_request_guard(id, "mcpServer/elicitation/request", &payload["params"])
			.ok_or_else(|| {
				ChiefError::Rejected("Native installation request has already ended".into())
			})
	}

	async fn inspect_live_install(
		&self,
		work: &str,
		event: i64,
	) -> Result<crate::chief_install::Inspection, ChiefError> {
		let guard = self.install_request_guard(event).await?;
		if !self.pending_requests.values().any(|id| *id == event) {
			return Err(ChiefError::Rejected("Installation request is no longer live".into()));
		}
		let result = tokio::time::timeout(
			std::time::Duration::from_secs(40),
			crate::chief_install::inspect(&self.store, &self.client, work, event),
		)
		.await
		.ok()
		.flatten()
		.ok_or_else(|| ChiefError::Rejected("Installation state is unavailable".into()))?;
		if !self
			.store
			.chief_thread_is_owned(
				work.into(),
				result.thread.clone(),
				self.native_generation.as_ref().map(|g| g.as_str().into()),
			)
			.await?
		{
			return Err(ChiefError::Rejected("Installation request ownership changed".into()));
		}
		if !guard.is_live() {
			return Err(ChiefError::Rejected(
				"Native installation request has already ended".into(),
			));
		}
		Ok(result)
	}

	pub(crate) async fn install_suggested_plugin(
		&self,
		work: &str,
		event: i64,
		review: &str,
		key: &str,
	) -> Result<(), ChiefError> {
		let inspected = self.inspect_live_install(work, event).await?;
		let ChiefInstallState::Available { can_install: true, review_token, tool_id, .. } =
			&inspected.state
		else {
			return Err(ChiefError::Rejected(
				"Inspect installation state; this request cannot be installed again".into(),
			));
		};
		if review_token != review {
			return Err(ChiefError::Rejected("Plugin details changed; review them again".into()));
		}
		let target = inspected.target.ok_or_else(|| {
			ChiefError::Rejected("This suggestion requires connector authorization".into())
		})?;
		let guard = self.install_request_guard(event).await?;
		if !self
			.store
			.reserve_chief_install_attempt(decodex_database::ChiefInstallAttempt {
				event_id: event,
				work_id: work.into(),
				thread_id: inspected.thread.clone(),
				generation_id: self.native_generation.as_ref().map(|g| g.as_str().into()),
				attempt_id: key.into(),
				plugin_id: tool_id.clone(),
			})
			.await?
		{
			return Err(ChiefError::UnknownDispatch);
		}
		// Do not consume the elicitation. Installation/authentication and completion are separate.
		let submitted = self.client.install_catalog_plugin_guarded(&target, key, guard).await;
		if let Ok(receipt) = &submitted {
			let connector_ids = receipt
				.apps_needing_auth
				.iter()
				.map(|app| app["id"].as_str().map(str::to_owned))
				.collect::<Option<Vec<_>>>()
				.ok_or(ChiefError::UnknownDispatch)?;
			self.store
				.record_chief_install_requirements(
					event,
					key.into(),
					decodex_database::ChiefInstallRequirements {
						auth_policy: receipt.auth_policy.clone(),
						connector_ids,
					},
				)
				.await
				.map_err(|_| ChiefError::UnknownDispatch)?;
		}
		let observed = self
			.inspect_live_install(work, event)
			.await
			.map_err(|_| ChiefError::UnknownDispatch)?;
		if matches!(observed.state, ChiefInstallState::Available { installed: Some(true), .. }) {
			return Ok(());
		}
		let _ = submitted;
		Err(ChiefError::UnknownDispatch)
	}

	pub(super) async fn verify_install_suggestion_complete(
		&self,
		event: i64,
	) -> Result<(), ChiefError> {
		let pending = self.store.get_chief_inbox_event(event).await?;
		let inspected = self.inspect_live_install(&pending.work_item_id, event).await?;
		if !matches!(inspected.state, ChiefInstallState::Available { can_continue: true, .. }) {
			return Err(ChiefError::Rejected(
				"Install or connect this integration, then check its status before continuing"
					.into(),
			));
		}
		Ok(())
	}
}
