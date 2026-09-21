//! Resolve native voice references through the existing bounded exact-turn history adapter.
use super::{Content, ordinary};
use decodex_codex::app_server_client::AppServerClient;
use decodex_protocol::{ChiefTimelinePage, ChiefTimelinePromotedContent};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub(super) async fn enrich(client: &AppServerClient, page: &mut ChiefTimelinePage) {
	let mut missing = BTreeMap::<String, Vec<(usize, String)>>::new();
	let mut loaded = BTreeMap::new();
	for entry in &page.entries {
		if let Content::Item { turn_id, item_id, .. } = &entry.content {
			loaded
				.entry((turn_id.clone(), item_id.clone()))
				.and_modify(|value| *value = None)
				.or_insert_with(|| project(entry.content.clone()));
		}
	}
	for (index, entry) in page.entries.iter_mut().enumerate() {
		if let Content::Promotion { turn_id, agent_item_id, resolved, .. } = &mut entry.content {
			if let Some(Some(item)) = loaded.get(&(turn_id.clone(), agent_item_id.clone())) {
				*resolved = Some(item.clone());
			} else {
				missing.entry(turn_id.clone()).or_default().push((index, agent_item_id.clone()));
			}
		}
	}
	// At most one exact read per referenced turn per bounded page. Raw history is
	// released between turns; the caller bounds the complete operation and output.
	for (turn, references) in missing {
		let Ok(history) = client.thread_read_turn(&page.thread_id, &turn).await else { continue };
		for (index, item) in references {
			let resolved = exact_item(&history, &page.thread_id, &turn, &item)
				.and_then(|item| ordinary(&json!({"turnId":turn,"item":item})))
				.and_then(project);
			if let Content::Promotion { resolved: target, .. } = &mut page.entries[index].content {
				*target = resolved;
			}
		}
	}
}

fn project(content: Content) -> Option<ChiefTimelinePromotedContent> {
	let Content::Item { text, truncated, activity, attachments, .. } = content else { return None };
	Some(ChiefTimelinePromotedContent { text, truncated, activity, attachments })
}

pub(super) fn exact_item<'a>(
	history: &'a Value,
	thread: &str,
	turn: &str,
	item: &str,
) -> Option<&'a Value> {
	if history.pointer("/thread/id")?.as_str()? != thread {
		return None;
	}
	let mut turns = history
		.pointer("/thread/turns")?
		.as_array()?
		.iter()
		.filter(|value| value["id"].as_str() == Some(turn));
	let found = turns.next()?;
	if turns.next().is_some() {
		return None;
	}
	let mut items =
		found["items"].as_array()?.iter().filter(|value| value["id"].as_str() == Some(item));
	let found = items.next()?;
	items.next().is_none().then_some(found)
}

#[cfg(test)]
#[path = "promotion_tests.rs"]
mod tests;
