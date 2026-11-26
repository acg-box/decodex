//! Resolve native voice references by native item ID.
use std::collections::BTreeMap;

use crate::agent::timeline::{self, AgentTimelineContent};
use decodex_codex::app_server_client::AppServerClient;
use decodex_protocol::{AgentTimelinePage, AgentTimelinePromotedContent};

pub(super) async fn enrich(client: &AppServerClient, page: &mut AgentTimelinePage) {
	let mut missing = BTreeMap::<String, Vec<(usize, String)>>::new();
	let mut loaded = BTreeMap::new();

	for entry in &page.entries {
		if let AgentTimelineContent::Item { turn_id, item_id, .. } = &entry.content {
			loaded
				.entry((turn_id.clone(), item_id.clone()))
				.and_modify(|value| *value = None)
				.or_insert_with(|| project(entry.content.clone()));
		}
	}
	for (index, entry) in page.entries.iter_mut().enumerate() {
		if let AgentTimelineContent::Promotion { turn_id, agent_item_id, resolved, .. } =
			&mut entry.content
		{
			if let Some(Some(item)) = loaded.get(&(turn_id.clone(), agent_item_id.clone())) {
				*resolved = Some(item.clone());
			} else {
				missing.entry(turn_id.clone()).or_default().push((index, agent_item_id.clone()));
			}
		}
	}
	// The timeline page is bounded to 30 entries, below the native 100-ID limit.
	for (turn, references) in missing {
		let mut ids: Vec<_> = references.iter().map(|(_, id)| id.as_str()).collect();
		ids.sort_unstable();
		ids.dedup();
		let Ok(items) = client.thread_read_items(&page.thread_id, &turn, &ids).await else {
			continue;
		};
		for (index, item) in references {
			let resolved = items
				.get(&item)
				.and_then(|item| {
					timeline::ordinary(&serde_json::json!({"turnId":turn,"item":item}))
				})
				.and_then(project);

			if let AgentTimelineContent::Promotion { resolved: target, .. } =
				&mut page.entries[index].content
			{
				*target = resolved;
			}
		}
	}
}

fn project(content: AgentTimelineContent) -> Option<AgentTimelinePromotedContent> {
	let AgentTimelineContent::Item { text, truncated, activity, attachments, .. } = content else {
		return None;
	};

	Some(AgentTimelinePromotedContent { text, truncated, activity, attachments })
}

#[cfg(test)]
#[path = "promotion_tests.rs"]
mod tests;
