//! Native archive state controls the active projection, never the retained work history.
use std::{
	collections::{HashMap, HashSet},
	sync::Arc,
	time::{Duration, Instant},
};

use decodex_codex::app_server_client::AppServerClient;
use decodex_protocol::AgentSnapshotDto;
use tokio::sync::Mutex;

#[derive(Default)]
pub(crate) struct ArchiveVisibility {
	pub generation: Option<String>,
	pub checked_at: Option<Instant>,
	pub archived: HashSet<String>,
}

impl ArchiveVisibility {
	pub async fn refresh(cache: &Arc<Mutex<Self>>, generation: &str, client: AppServerClient) {
		let mut state = cache.lock().await;
		if !state.refresh_due(generation, Instant::now()) {
			return;
		}
		let cache = cache.clone();
		let generation = generation.to_owned();
		// The bounded native read can outlive a desktop snapshot request.
		// Refresh separately so ordinary queries use the last confirmed view.
		tokio::spawn(async move {
			if let Ok(archived) = client.archived_threads().await {
				cache.lock().await.apply(&generation, archived);
			}
		});
	}

	pub fn apply(&mut self, generation: &str, archived: HashSet<String>) {
		if self.generation.as_deref() == Some(generation) {
			self.archived = archived;
		}
	}

	pub fn refresh_due(&mut self, generation: &str, now: Instant) -> bool {
		if self.generation.as_deref() != Some(generation) {
			*self = Self { generation: Some(generation.into()), ..Self::default() };
		}
		if self.checked_at.is_some_and(|at| now.duration_since(at) < Duration::from_secs(15)) {
			return false;
		}
		self.checked_at = Some(now);
		true
	}
}

/// Remove only positively archived bindings from the shared active view.
/// Keep live descendants visible under their nearest retained ancestor.
pub(crate) fn retain_active(snapshot: &mut AgentSnapshotDto, archived: &HashSet<String>) {
	let hidden: HashSet<_> = snapshot
		.work_items
		.iter()
		.filter(|work| work.codex_thread_id.as_ref().is_some_and(|id| archived.contains(id)))
		.map(|work| work.id.clone())
		.collect();
	if hidden.is_empty() {
		return;
	}
	let parents: HashMap<_, _> = snapshot
		.work_items
		.iter()
		.map(|work| (work.id.clone(), work.parent_goal_id.clone()))
		.collect();
	snapshot.work_items.retain(|work| !hidden.contains(&work.id));
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
		!hidden.contains(&edge.work_item_id) && !hidden.contains(&edge.depends_on_id)
	});
	snapshot.pending_events.retain(|event| !hidden.contains(&event.work_item_id));
	snapshot.context_references.retain(|reference| {
		!hidden.contains(&reference.recipient_work_id)
			&& !hidden.contains(&reference.source_work_id)
	});
	for workspace in &mut snapshot.workspaces {
		workspace.work_ids.retain(|id| !hidden.contains(id));
	}
}

#[cfg(test)]
mod tests {
	use super::{ArchiveVisibility, retain_active};
	use decodex_codex::app_server_client::AppServerClient;
	use decodex_protocol::{
		AgentDependencyDto, AgentDispatchStateDto, AgentSnapshotDto, AgentWorkItemDto,
		AgentWorkKindDto, AgentWorkStatusDto, WorkspaceDto,
	};
	use std::{
		collections::HashSet,
		sync::Arc,
		time::{Duration, Instant},
	};
	use tokio::sync::{Mutex, mpsc};

	#[tokio::test]
	async fn slow_native_archive_read_does_not_block_snapshots_or_duplicate_refresh() {
		let cache = Arc::new(Mutex::new(ArchiveVisibility::default()));
		let (incoming, input) = mpsc::channel(8);
		let (output, mut outgoing) = mpsc::channel(8);
		let (client, _events) = AppServerClient::from_framed(1, input, output).unwrap();
		tokio::time::timeout(
			Duration::from_secs(1),
			ArchiveVisibility::refresh(&cache, "process", client.clone()),
		)
		.await
		.expect("snapshot scheduling must not wait for the native response");
		let request = outgoing.recv().await.unwrap();
		assert!(cache.lock().await.archived.is_empty());
		ArchiveVisibility::refresh(&cache, "process", client).await;
		assert!(outgoing.try_recv().is_err(), "one refresh per interval");
		incoming
			.send(Ok(serde_json::json!({
				"id":request["id"],"result":{"data":[{"id":"archived"}],"nextCursor":null}
			})))
			.await
			.unwrap();
		tokio::time::timeout(Duration::from_secs(1), async {
			loop {
				if cache.lock().await.archived.contains("archived") {
					break;
				}
				tokio::task::yield_now().await;
			}
		})
		.await
		.expect("the completed catalog becomes visible to later snapshots");
	}

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
			context_references: vec![],
			connection_initializing: false,
			runtime_source: None,
			workspaces: vec![WorkspaceDto {
				id: "workspace".into(),
				name: "Workspace".into(),
				directory: "/fixture".into(),
				work_ids: vec!["main".into(), "old".into(), "live".into()],
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
	fn archive_removes_only_exact_nodes_and_incident_edges_without_hiding_live_descendants() {
		let original = snapshot();
		let mut visible = original.clone();
		retain_active(&mut visible, &HashSet::from(["thread-old".into()]));
		assert_eq!(
			visible.work_items.iter().map(|w| w.id.as_str()).collect::<Vec<_>>(),
			["main", "live"]
		);
		assert_eq!(visible.work_items[1].parent_goal_id.as_deref(), Some("main"));
		assert_eq!(visible.workspaces[0].work_ids, ["main", "live"]);
		assert!(visible.dependencies.is_empty());
		assert!(visible.is_valid());
		assert_eq!(original.work_items.len(), 3, "retained source is unchanged");
		let mut restored = original.clone();
		retain_active(&mut restored, &HashSet::new());
		assert_eq!(restored, original, "unarchive restores the original hierarchy");
	}
	#[test]
	fn unknown_and_unbound_work_stays_visible() {
		let mut before = snapshot();
		before.work_items[1].codex_thread_id = None;
		let mut visible = before.clone();
		retain_active(&mut visible, &HashSet::from(["thread-old".into(), "unrelated".into()]));
		assert_eq!(visible, before);
	}
	#[test]
	fn archive_cache_is_scoped_to_generation_and_does_not_repoll_each_snapshot() {
		let now = Instant::now();
		let mut cache = ArchiveVisibility::default();
		assert!(cache.refresh_due("first", now));
		cache.archived.insert("old".into());
		assert!(!cache.refresh_due("first", now + Duration::from_secs(1)));
		assert!(cache.refresh_due("first", now + Duration::from_secs(15)));
		assert!(cache.archived.contains("old"), "a failed refresh retains confirmed state");
		assert!(cache.refresh_due("second", now + Duration::from_secs(16)));
		assert!(cache.archived.is_empty(), "another process cannot inherit archive observations");
		cache.apply("first", HashSet::from(["stale".into()]));
		assert!(cache.archived.is_empty(), "a previous process cannot publish a late refresh");
		cache.apply("second", HashSet::from(["current".into()]));
		assert!(cache.archived.contains("current"));
	}
}
