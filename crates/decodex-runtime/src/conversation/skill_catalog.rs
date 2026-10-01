//! Discover skills before first input through the existing account-bound metadata owner.
use std::time::{Duration, Instant};

use tokio::{sync::oneshot, time};

use crate::{
	agent_skill_roots::RuntimeSkillRoots, agent_skills, conversation::ConversationRuntime,
};
use decodex_protocol::{
	AgentSkillsResult, AgentSkillsTarget, InitialModelCatalogRequest, ModelCatalogPurpose,
};

impl ConversationRuntime {
	pub(crate) async fn initial_skills(
		&self,
		key: &str,
		request: InitialModelCatalogRequest,
		filter: String,
		roots: RuntimeSkillRoots,
	) -> AgentSkillsResult {
		if request.purpose != ModelCatalogPurpose::Agent {
			return AgentSkillsResult::Unavailable;
		}

		// Opening the attachment menu also refreshes the model catalog. Wait for that
		// shared metadata owner instead of reporting a spurious unavailable skill list.
		let started = Instant::now();
		let Ok(permit) =
			time::timeout(Duration::from_secs(25), self.inner.initial_catalog.clone().lock_owned())
				.await
		else {
			return AgentSkillsResult::Unavailable;
		};
		let (reply, received) = oneshot::channel();
		let mut workers = self.inner.workers.lock().await;

		if self.is_shutting_down() {
			return AgentSkillsResult::Unavailable;
		}

		while workers.try_join_next().is_some() {}

		let runtime = self.clone();
		let key = key.to_owned();

		workers.spawn(async move {
			let _permit = permit;
			let target = AgentSkillsTarget::New { request: request.clone() };
			let observation = runtime
				.discover_initial_metadata(&key, request, move |child, cwd| {
					let (result, events) = child.read_ordinary_skills(cwd, roots.values());

					child.retain_ordinary_events(events).ok()?;

					agent_skills::project(&result.ok()?, cwd, &filter)
				})
				.await;
			let result = observation.map_or(AgentSkillsResult::Unavailable, |(_, _, _, page)| {
				AgentSkillsResult::Available { target, page }
			});
			let _ = reply.send(result);
		});

		drop(workers);

		time::timeout(Duration::from_secs(35).saturating_sub(started.elapsed()), received)
			.await
			.ok()
			.and_then(Result::ok)
			.unwrap_or(AgentSkillsResult::Unavailable)
	}
}
