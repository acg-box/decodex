//! Native archive state controls the active projection, never the retained work history.
use std::{
	collections::{HashMap, HashSet},
	sync::Arc,
	time::{Duration, Instant},
};

use tokio::sync::Mutex;

use crate::startup_trace;
use decodex_codex::app_server_client::{AppServerClient, ThreadArchiveState};
use decodex_protocol::AgentSnapshotDto;

#[derive(Default)]
pub(crate) struct ArchiveVisibility {
	pub generation: Option<String>,
	pub checked_at: Option<Instant>,
	pub archived: HashSet<String>,
	pub retired: HashSet<String>,
	refreshing: bool,
	threads: HashSet<String>,
	initial_pending: Option<HashSet<String>>,
	observation: u64,
}
impl ArchiveVisibility {
	pub fn confirm_active(&mut self, thread: &str) {
		self.observation = self.observation.wrapping_add(1);
		self.refreshing = false;
		self.checked_at = None;
		self.archived.remove(thread);
		self.retired.remove(thread);
		if let Some(pending) = &mut self.initial_pending {
			pending.remove(thread);
		}
	}

	pub fn ready_for(&self, generation: &str) -> bool {
		self.generation.as_deref() == Some(generation) && self.initial_pending.is_none()
	}

	pub fn project_view(&self, snapshot: &mut AgentSnapshotDto, archived: bool) {
		if !archived {
			self.project(snapshot);
			return;
		}
		snapshot.connection_initializing |=
			self.generation.is_none() || self.initial_pending.is_some();
		snapshot.archived_work_ids = snapshot
			.work_items
			.iter()
			.filter(|work| {
				work.codex_thread_id.as_ref().is_some_and(|id| self.archived.contains(id))
			})
			.map(|work| work.id.clone())
			.collect();
	}

	pub fn project(&self, snapshot: &mut AgentSnapshotDto) {
		// Finishing a request does not prove that the first archive view is complete.
		snapshot.connection_initializing |=
			self.generation.is_none() || self.initial_pending.is_some();

		retain_active(snapshot, &self.archived);
	}

	pub async fn refresh(
		cache: &Arc<Mutex<Self>>,
		generation: &str,
		client: AppServerClient,
		threads: HashSet<String>,
	) {
		let mut state = cache.lock().await;

		if !state.refresh_due(generation, &threads, Instant::now()) {
			return;
		}
		state.observation = state.observation.wrapping_add(1);
		let observation = state.observation;

		let cache = cache.clone();
		let generation = generation.to_owned();

		// The bounded native read can outlive a desktop snapshot request.
		// Refresh separately so ordinary queries use the last confirmed view.
		tokio::spawn(async move {
			let observed = client.observe_thread_archives(&threads).await;

			let mut cache = cache.lock().await;
			if cache.observation == observation {
				cache.apply(&generation, observed);
			}
		});
	}

	pub fn apply(&mut self, generation: &str, observed: HashMap<String, ThreadArchiveState>) {
		if self.generation.as_deref() == Some(generation) {
			for (id, state) in observed {
				if state != ThreadArchiveState::Changed
					&& let Some(pending) = &mut self.initial_pending
				{
					pending.remove(&id);
				}

				match state {
					ThreadArchiveState::Active => {
						self.archived.remove(&id);
						self.retired.remove(&id);
					},
					ThreadArchiveState::Archived => {
						self.archived.insert(id);
					},
					ThreadArchiveState::NotFound | ThreadArchiveState::Changed => {},
				}
			}

			if self.initial_pending.as_ref().is_some_and(HashSet::is_empty) {
				self.initial_pending = None;
			}

			startup_trace::archive_pending(self.initial_pending.as_ref().map_or(0, HashSet::len));

			self.refreshing = false;
			self.checked_at = Some(Instant::now());
		}
	}

	pub fn refresh_due(
		&mut self,
		generation: &str,
		threads: &HashSet<String>,
		now: Instant,
	) -> bool {
		if self.generation.as_deref() != Some(generation) {
			*self = Self {
				generation: Some(generation.into()),
				initial_pending: Some(threads.clone()),
				..Self::default()
			};
		}

		if let Some(pending) = &mut self.initial_pending {
			pending.retain(|id| threads.contains(id));
			pending.extend(threads.difference(&self.threads).cloned());
		}

		if self.refreshing
			|| (self.threads == *threads
				&& self
					.checked_at
					.is_some_and(|at| now.duration_since(at) < Duration::from_secs(5)))
		{
			return false;
		}

		self.refreshing = true;
		self.threads = threads.clone();

		self.archived.retain(|id| threads.contains(id));

		true
	}
}

/// Remove archived conversations, while retaining the personal coordinator entry.
/// Its native archive state still controls whether its conversation can accept input.
/// Keep live descendants visible under their nearest retained ancestor.
pub(crate) fn retain_active(snapshot: &mut AgentSnapshotDto, archived: &HashSet<String>) {
	snapshot.archived_work_ids.clear();

	let inactive: HashSet<_> = snapshot
		.work_items
		.iter()
		.filter(|work| work.codex_thread_id.as_ref().is_some_and(|id| archived.contains(id)))
		.map(|work| work.id.clone())
		.collect();

	if inactive.is_empty() {
		return;
	}

	let hidden: HashSet<_> = snapshot
		.work_items
		.iter()
		.filter(|work| work.parent_goal_id.is_some() && inactive.contains(&work.id))
		.map(|work| work.id.clone())
		.collect();
	let parents: HashMap<_, _> = snapshot
		.work_items
		.iter()
		.map(|work| (work.id.clone(), work.parent_goal_id.clone()))
		.collect();

	snapshot.work_items.retain(|work| !hidden.contains(&work.id));
	snapshot.archived_work_ids.extend(inactive.iter().filter(|id| !hidden.contains(*id)).cloned());

	for work in &mut snapshot.work_items {
		let mut seen = HashSet::new();

		while let Some(parent) = work.parent_goal_id.clone().filter(|id| hidden.contains(id)) {
			if !seen.insert(parent.clone()) {
				work.parent_goal_id = None;

				break;
			}

			work.parent_goal_id = parents.get(&parent).cloned().flatten();
		}
	}

	snapshot.dependencies.retain(|edge| {
		!inactive.contains(&edge.work_item_id) && !inactive.contains(&edge.depends_on_id)
	});
	snapshot.pending_events.retain(|event| !inactive.contains(&event.work_item_id));
	snapshot.context_references.retain(|reference| {
		!inactive.contains(&reference.recipient_work_id)
			&& !inactive.contains(&reference.source_work_id)
	});

	for workspace in &mut snapshot.projects {
		workspace.agent_ids.retain(|id| !hidden.contains(id));
	}
}

#[cfg(test)]
mod tests {
	use std::{
		collections::{HashMap, HashSet},
		sync::Arc,
		time::{Duration, Instant},
	};

	use tokio::{
		sync::{Mutex, mpsc},
		task, time,
	};

	use crate::agent_visibility::{self, ArchiveVisibility};
	use decodex_codex::app_server_client::{AppServerClient, ThreadArchiveState};
	use decodex_protocol::{
		AgentDependencyDto, AgentDispatchStateDto, AgentSnapshotDto, AgentWorkItemDto,
		AgentWorkKindDto, AgentWorkStatusDto, ProjectDto,
	};

	fn work(id: &str, parent: Option<&str>) -> AgentWorkItemDto {
		AgentWorkItemDto {
			id: id.into(),
			parent_goal_id: parent.map(str::to_owned),
			kind: AgentWorkKindDto::Goal,
			title: id.into(),
			codex_thread_id: Some(format!("thread-{id}")),
			active_turn_id: None,
			dispatch_state: AgentDispatchStateDto::Idle,
			status: AgentWorkStatusDto::Open,
			next_check_at_micros: None,
			created_at_micros: 1,
			updated_at_micros: 1,
		}
	}

	fn snapshot() -> AgentSnapshotDto {
		AgentSnapshotDto {
			agents: Default::default(),
			guardian: Default::default(),
			portfolio: Default::default(),
			archived_work_ids: Default::default(),
			agent_slots: Default::default(),
			context_references: vec![],
			connection_initializing: false,
			runtime_source: None,
			projects: vec![ProjectDto {
				native_project_id: None,
				lead_agent_id: None,
				id: "workspace".into(),
				name: "Workspace".into(),
				directory: "/fixture".into(),
				agent_ids: vec!["main".into(), "old".into(), "live".into()],
			}],
			work_items: vec![
				work("main", None),
				work("old", Some("main")),
				work("live", Some("old")),
			],
			dependencies: vec![AgentDependencyDto {
				work_item_id: "live".into(),
				depends_on_id: "old".into(),
			}],
			pending_events: vec![],
		}
	}

	#[test]
	fn disconnected_projection_waits_for_initial_visibility_and_keeps_confirmed_archives() {
		let mut cache = ArchiveVisibility::default();
		let mut initial = snapshot();

		cache.project(&mut initial);

		assert!(initial.connection_initializing);

		cache.generation = Some("known-process".into());

		cache.apply(
			"known-process",
			HashMap::from([("thread-old".into(), ThreadArchiveState::Archived)]),
		);

		let mut disconnected = snapshot();

		cache.project(&mut disconnected);

		assert!(!disconnected.connection_initializing);
		assert!(!disconnected.work_items.iter().any(|work| work.id == "old"));
		assert!(disconnected.work_items.iter().any(|work| work.id == "live"));
	}

	#[test]
	fn archive_removes_only_exact_nodes_and_incident_edges_without_hiding_live_descendants() {
		let original = snapshot();
		let mut visible = original.clone();

		agent_visibility::retain_active(&mut visible, &HashSet::from(["thread-old".into()]));

		assert_eq!(
			visible.work_items.iter().map(|w| w.id.as_str()).collect::<Vec<_>>(),
			["main", "live"]
		);
		assert_eq!(visible.work_items[1].parent_goal_id.as_deref(), Some("main"));
		assert_eq!(visible.projects[0].agent_ids, ["main", "live"]);
		assert!(visible.dependencies.is_empty());
		assert!(visible.is_valid());
		assert_eq!(original.work_items.len(), 3, "retained source is unchanged");

		let mut restored = original.clone();

		agent_visibility::retain_active(&mut restored, &HashSet::new());

		assert_eq!(restored, original, "unarchive restores the original hierarchy");
	}

	#[test]
	fn archive_catalog_retains_hidden_nodes_and_original_relationships() {
		let original = snapshot();
		let mut catalog = original.clone();
		let mut cache = ArchiveVisibility {
			generation: Some("native".into()),
			archived: HashSet::from(["thread-old".into()]),
			..Default::default()
		};
		cache.project_view(&mut catalog, true);
		assert_eq!(catalog.work_items, original.work_items);
		assert_eq!(catalog.projects, original.projects);
		assert_eq!(catalog.dependencies, original.dependencies);
		assert_eq!(catalog.archived_work_ids, std::collections::BTreeSet::from(["old".into()]));
		assert!(catalog.is_valid());

		let previous = cache.observation;
		cache.confirm_active("thread-old");
		assert_ne!(
			cache.observation, previous,
			"ignore an archive observation started before restoration"
		);
		let mut restored = original.clone();
		cache.project(&mut restored);
		assert_eq!(restored, original);
	}

	#[test]
	fn archiving_main_retains_its_entry_and_real_ownership_without_restoring_history() {
		let original = snapshot();
		let mut visible = original.clone();

		agent_visibility::retain_active(
			&mut visible,
			&HashSet::from(["thread-main".into(), "thread-old".into()]),
		);

		assert_eq!(
			visible.work_items.iter().map(|work| work.id.as_str()).collect::<Vec<_>>(),
			["main", "live"]
		);
		assert_eq!(
			visible.work_items[0], original.work_items[0],
			"keep the exact archived binding"
		);
		assert_eq!(visible.archived_work_ids, ["main".to_owned()].into());
		assert_eq!(visible.work_items[1].parent_goal_id.as_deref(), Some("main"));
		assert!(visible.dependencies.is_empty());
		assert!(visible.is_valid());

		agent_visibility::retain_active(
			&mut visible,
			&HashSet::from(["thread-main".into(), "thread-live".into()]),
		);

		assert_eq!(visible.work_items, vec![original.work_items[0].clone()]);
		assert!(visible.is_valid(), "Main remains the entry after all conversations are archived");
	}

	#[test]
	fn unknown_and_unbound_work_stays_visible() {
		let mut before = snapshot();

		before.work_items[1].codex_thread_id = None;

		let mut visible = before.clone();

		agent_visibility::retain_active(
			&mut visible,
			&HashSet::from(["thread-old".into(), "unrelated".into()]),
		);

		assert_eq!(visible, before);
	}

	#[test]
	fn archive_cache_is_scoped_to_generation_and_does_not_repoll_each_snapshot() {
		let now = Instant::now();
		let threads = HashSet::from(["old".into(), "current".into()]);
		let mut cache = ArchiveVisibility::default();

		assert!(cache.refresh_due("first", &threads, now));

		cache.archived.insert("old".into());

		assert!(!cache.refresh_due("first", &threads, now + Duration::from_secs(1)));
		assert!(
			!cache.refresh_due("first", &threads, now + Duration::from_secs(15)),
			"an unfinished scan must not overlap a later refresh"
		);

		cache.apply("first", HashMap::new());

		assert!(cache.refresh_due("first", &threads, now + Duration::from_secs(15)));
		assert!(cache.archived.contains("old"), "a failed refresh retains confirmed state");
		assert!(cache.refresh_due("second", &threads, now + Duration::from_secs(16)));
		assert!(cache.archived.is_empty(), "another process cannot inherit archive observations");

		cache.apply("first", HashMap::from([("stale".into(), ThreadArchiveState::Archived)]));

		assert!(cache.archived.is_empty(), "a previous process cannot publish a late refresh");

		cache.apply("second", HashMap::from([("current".into(), ThreadArchiveState::Archived)]));

		assert!(cache.archived.contains("current"));

		cache.apply("second", HashMap::from([("current".into(), ThreadArchiveState::Active)]));

		assert!(cache.archived.is_empty(), "positive active membership restores the same binding");
	}

	#[test]
	fn archive_refresh_keeps_ready_content_and_new_generation_waits_for_its_own_read() {
		let now = Instant::now();
		let threads = HashSet::from(["thread-old".into()]);
		let mut cache = ArchiveVisibility::default();

		cache.refresh_due("first", &threads, now);
		cache.apply("first", HashMap::from([("thread-old".into(), ThreadArchiveState::Archived)]));

		assert!(cache.refresh_due("first", &threads, now + Duration::from_secs(10)));

		let mut refreshing = snapshot();

		cache.project(&mut refreshing);

		assert!(!refreshing.connection_initializing);
		assert_eq!(refreshing.work_items.len(), 2);

		let mut connecting = snapshot();

		connecting.connection_initializing = true;

		cache.project(&mut connecting);

		assert!(
			connecting.connection_initializing,
			"archive readiness cannot end service recovery"
		);
		assert!(cache.refresh_due("second", &threads, now + Duration::from_secs(11)));

		cache.apply("first", HashMap::new());

		let mut replaced = snapshot();

		cache.project(&mut replaced);

		assert!(replaced.connection_initializing, "a stale result cannot reveal a new source");

		cache.apply("second", HashMap::new());

		let mut finished = snapshot();

		cache.project(&mut finished);

		assert!(finished.connection_initializing, "a failed read cannot reveal retained history");

		cache.apply("second", HashMap::from([("thread-old".into(), ThreadArchiveState::NotFound)]));

		let mut confirmed = snapshot();

		cache.project(&mut confirmed);

		assert!(!confirmed.connection_initializing, "complete absence can finish the first read");
	}

	#[test]
	fn partial_initial_archive_reads_never_reveal_unfiltered_history() {
		let source = snapshot();
		let threads =
			source.work_items.iter().filter_map(|work| work.codex_thread_id.clone()).collect();
		let mut cache = ArchiveVisibility::default();

		cache.refresh_due("process", &threads, Instant::now());

		assert!(!cache.ready_for("process"));

		for observed in [
			HashMap::new(),
			HashMap::from([("thread-main".into(), ThreadArchiveState::Active)]),
			HashMap::from([("thread-old".into(), ThreadArchiveState::Archived)]),
		] {
			cache.apply("process", observed);

			let mut pending = source.clone();

			cache.project(&mut pending);

			assert!(pending.connection_initializing);
			assert!(!cache.ready_for("process"));
		}

		cache.apply("process", HashMap::from([("thread-live".into(), ThreadArchiveState::Active)]));

		let mut ready = source;

		cache.project(&mut ready);

		assert!(!ready.connection_initializing);
		assert!(cache.ready_for("process"));
		assert!(!cache.ready_for("another-process"));
		assert_eq!(
			ready.work_items.iter().map(|work| work.id.as_str()).collect::<Vec<_>>(),
			["main", "live"]
		);
	}

	#[test]
	fn bindings_added_during_the_initial_read_wait_for_their_own_archive_state() {
		let now = Instant::now();
		let threads = HashSet::from(["thread-main".into()]);
		let mut cache = ArchiveVisibility::default();

		assert!(cache.refresh_due("process", &threads, now));

		let expanded = HashSet::from(["thread-main".into(), "thread-old".into()]);

		assert!(!cache.refresh_due("process", &expanded, now));

		cache.apply("process", HashMap::from([("thread-main".into(), ThreadArchiveState::Active)]));

		let mut pending = snapshot();

		cache.project(&mut pending);

		assert!(pending.connection_initializing, "the old request cannot confirm a new binding");
		assert!(cache.refresh_due("process", &expanded, now));

		cache
			.apply("process", HashMap::from([("thread-old".into(), ThreadArchiveState::Archived)]));

		let mut ready = snapshot();

		cache.project(&mut ready);

		assert!(!ready.connection_initializing);
		assert!(ready.work_items.iter().all(|work| work.id != "old"));
	}

	#[test]
	fn archive_new_bindings_refresh_without_waiting_and_unknown_bindings_keep_their_state() {
		let now = Instant::now();
		let threads = HashSet::from(["old".into()]);
		let mut cache = ArchiveVisibility::default();

		assert!(cache.refresh_due("process", &threads, now));

		cache.apply("process", HashMap::from([("old".into(), ThreadArchiveState::Archived)]));

		assert!(!cache.refresh_due("process", &threads, now + Duration::from_secs(1)));

		let expanded = HashSet::from(["old".into(), "new".into()]);

		assert!(cache.refresh_due("process", &expanded, now + Duration::from_secs(1)));

		cache.apply("process", HashMap::from([("new".into(), ThreadArchiveState::Active)]));

		assert!(cache.archived.contains("old"), "missing evidence must not unarchive a binding");
	}

	#[tokio::test]
	async fn slow_native_archive_read_does_not_block_snapshots_or_duplicate_refresh() {
		let cache = Arc::new(Mutex::new(ArchiveVisibility::default()));
		let (incoming, input) = mpsc::channel(8);
		let (output, mut outgoing) = mpsc::channel(8);
		let (client, _events) = AppServerClient::from_framed(1, input, output).unwrap();
		let threads = HashSet::from(["archived".to_owned()]);

		time::timeout(
			Duration::from_secs(1),
			ArchiveVisibility::refresh(&cache, "process", client.clone(), threads.clone()),
		)
		.await
		.expect("snapshot scheduling must not wait for the native response");

		let request = outgoing.recv().await.unwrap();

		assert!(cache.lock().await.archived.is_empty());

		let mut pending = snapshot();

		cache.lock().await.project(&mut pending);

		assert!(pending.connection_initializing, "do not reveal an unfiltered first snapshot");

		ArchiveVisibility::refresh(&cache, "process", client, threads).await;

		assert!(outgoing.try_recv().is_err(), "one refresh per interval");
		assert_eq!(request["method"], "thread/read");

		incoming
			.send(Ok(serde_json::json!({
				"id":request["id"],"result":{"thread":{"id":"archived","cwd":"/workspace"}}
			})))
			.await
			.unwrap();

		let request = outgoing.recv().await.unwrap();

		incoming
			.send(Ok(serde_json::json!({
				"id":request["id"],"result":{"data":[],"nextCursor":null}
			})))
			.await
			.unwrap();

		let request = outgoing.recv().await.unwrap();

		assert_eq!(request["params"]["archived"], true);

		incoming
			.send(Ok(serde_json::json!({
				"id":request["id"],"result":{"data":[{"id":"archived"}],"nextCursor":null}
			})))
			.await
			.unwrap();

		time::timeout(Duration::from_secs(1), async {
			loop {
				if cache.lock().await.archived.contains("archived") {
					break;
				}

				task::yield_now().await;
			}
		})
		.await
		.expect("the completed catalog becomes visible to later snapshots");

		let mut ready = snapshot();

		ready.work_items[1].codex_thread_id = Some("archived".into());

		cache.lock().await.project(&mut ready);

		assert!(!ready.connection_initializing);
		assert_eq!(
			ready.work_items.iter().map(|work| work.id.as_str()).collect::<Vec<_>>(),
			["main", "live"]
		);
	}
}
