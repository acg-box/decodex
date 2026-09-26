use super::{AgentCoordinator, AgentError, Value};
use decodex_protocol::AgentInstallState;

impl AgentCoordinator {
	pub(super) async fn install_request_guard(
		&self,
		event: i64,
	) -> Result<decodex_codex::app_server_client::ServerRequestGuard, AgentError> {
		let id = self
			.pending_requests
			.iter()
			.find_map(|(id, current)| (*current == event).then_some(id))
			.ok_or_else(|| AgentError::Rejected("Installation request is no longer live".into()))?;
		let event = self.store.get_agent_inbox_event(event).await?;
		let payload: Value = serde_json::from_str(&event.payload)
			.map_err(|_| AgentError::Rejected("Invalid stored request".into()))?;
		self.client
			.server_request_guard(id, "mcpServer/elicitation/request", &payload["params"])
			.ok_or_else(|| {
				AgentError::Rejected("Native installation request has already ended".into())
			})
	}

	async fn inspect_live_install(
		&self,
		work: &str,
		event: i64,
	) -> Result<crate::agent_install::Inspection, AgentError> {
		let guard = self.install_request_guard(event).await?;
		if !self.pending_requests.values().any(|id| *id == event) {
			return Err(AgentError::Rejected("Installation request is no longer live".into()));
		}
		let result = tokio::time::timeout(
			std::time::Duration::from_secs(40),
			crate::agent_install::inspect(&self.store, &self.client, work, event),
		)
		.await
		.ok()
		.flatten()
		.ok_or_else(|| AgentError::Rejected("Installation state is unavailable".into()))?;
		if !self
			.store
			.agent_thread_is_owned(
				work.into(),
				result.thread.clone(),
				self.native_generation.as_ref().map(|g| g.as_str().into()),
			)
			.await?
		{
			return Err(AgentError::Rejected("Installation request ownership changed".into()));
		}
		if !guard.is_live() {
			return Err(AgentError::Rejected(
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
	) -> Result<(), AgentError> {
		let inspected = self.inspect_live_install(work, event).await?;
		let AgentInstallState::Available { can_install: true, review_token, tool_id, .. } =
			&inspected.state
		else {
			return Err(AgentError::Rejected(
				"Inspect installation state; this request cannot be installed again".into(),
			));
		};
		if review_token != review {
			return Err(AgentError::Rejected("Plugin details changed; review them again".into()));
		}
		let target = inspected.target.ok_or_else(|| {
			AgentError::Rejected("This suggestion requires connector authorization".into())
		})?;
		let guard = self.install_request_guard(event).await?;
		if !self
			.store
			.reserve_agent_install_attempt(decodex_database::AgentInstallAttempt {
				event_id: event,
				work_id: work.into(),
				thread_id: inspected.thread.clone(),
				generation_id: self.native_generation.as_ref().map(|g| g.as_str().into()),
				attempt_id: key.into(),
				plugin_id: tool_id.clone(),
			})
			.await?
		{
			return Err(AgentError::UnknownDispatch);
		}
		// Do not consume the elicitation. Installation/authentication and completion are separate.
		let submitted = self.client.install_catalog_plugin_guarded(&target, key, guard).await;
		if let Ok(receipt) = &submitted {
			let connector_ids = receipt
				.apps_needing_auth
				.iter()
				.map(|app| app["id"].as_str().map(str::to_owned))
				.collect::<Option<Vec<_>>>()
				.ok_or(AgentError::UnknownDispatch)?;
			self.store
				.record_agent_install_requirements(
					event,
					key.into(),
					decodex_database::AgentInstallRequirements {
						auth_policy: receipt.auth_policy.clone(),
						connector_ids,
					},
				)
				.await
				.map_err(|_| AgentError::UnknownDispatch)?;
		}
		let observed = self
			.inspect_live_install(work, event)
			.await
			.map_err(|_| AgentError::UnknownDispatch)?;
		if matches!(observed.state, AgentInstallState::Available { installed: Some(true), .. }) {
			return Ok(());
		}
		let _ = submitted;
		Err(AgentError::UnknownDispatch)
	}

	pub(super) async fn verify_install_suggestion_complete(
		&self,
		event: i64,
	) -> Result<(), AgentError> {
		let pending = self.store.get_agent_inbox_event(event).await?;
		let inspected = self.inspect_live_install(&pending.work_item_id, event).await?;
		if !matches!(inspected.state, AgentInstallState::Available { can_continue: true, .. }) {
			return Err(AgentError::Rejected(
				"Install or connect this integration, then check its status before continuing"
					.into(),
			));
		}
		Ok(())
	}
}
