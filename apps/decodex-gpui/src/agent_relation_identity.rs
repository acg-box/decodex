//! Display runtime-owned slots without allocating identities or writing during rendering.
use std::collections::BTreeMap;

use decodex_protocol::AgentSnapshotDto;

pub(super) fn labels(
	snapshot: Option<&AgentSnapshotDto>,
	keys: impl Iterator<Item = String>,
) -> BTreeMap<String, String> {
	let Some(snapshot) = snapshot else { return BTreeMap::new() };
	keys.filter_map(|key| snapshot.agent_slots.get(&key).map(|number| (key, format!("A{number}"))))
		.collect()
}
