//! Project confirmed lifecycle changes into reusable display slots.
use std::collections::HashSet;

use serde_json::Value;

use decodex_database::{AgentSlot, SqliteStore, StoreError};
use decodex_protocol::AgentSnapshotDto;

pub(crate) async fn synchronize(
	store: &SqliteStore,
	snapshot: &mut AgentSnapshotDto,
	previous: Vec<AgentSlot>,
	archived: &HashSet<String>,
) -> Result<(), StoreError> {
	if snapshot.connection_initializing {
		return Ok(());
	}

	let active_work: Vec<_> = snapshot
		.work_items
		.iter()
		.filter(|work| {
			!work.codex_thread_id.as_ref().is_some_and(|thread| archived.contains(thread))
		})
		.collect();
	let retained: HashSet<_> = active_work.iter().map(|work| work.id.as_str()).collect();
	let retired = previous
		.iter()
		.filter(|slot| {
			!retained.contains(slot.work_id.as_str())
				|| slot.thread_id.as_ref().is_some_and(|thread| archived.contains(thread))
		})
		.map(|slot| slot.key.clone())
		.collect();
	let active = active_work
		.into_iter()
		.map(|work| (work.id.clone(), work.id.clone(), work.codex_thread_id.clone()))
		.collect();

	snapshot.agent_slots = store
		.reconcile_agent_slots(active, retired)
		.await?
		.into_iter()
		.filter(|slot| retained.contains(slot.work_id.as_str()))
		.map(|slot| (slot.key, slot.number))
		.collect();

	Ok(())
}

pub(crate) async fn observe_native_close(
	store: &SqliteStore,
	item: &Value,
) -> Result<(), StoreError> {
	if item["type"] == "collabAgentToolCall"
		&& item["tool"] == "closeAgent"
		&& item["status"] == "completed"
	{
		let threads = item["receiverThreadIds"]
			.as_array()
			.into_iter()
			.flatten()
			.filter_map(|value| value.as_str().map(str::to_owned))
			.collect();

		store.release_native_agent_slots(threads).await?;
	}

	Ok(())
}
