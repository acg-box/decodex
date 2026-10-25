//! Read the exact loaded source's native memory without changing execution or memory policy.
use std::{future::Future, time::Duration};

use decodex_protocol::{AgentMemoryResult, EntityId, MEMORY_PAGE_BYTES, WireText};
use sha2::{Digest as _, Sha256};

use crate::agent_usage_estimate::Source;

pub(crate) async fn read<F, Fut>(
	source: F,
	digest: Option<&str>,
	offset: usize,
) -> AgentMemoryResult
where
	F: Fn() -> Fut,
	Fut: Future<Output = Option<Source>>,
{
	let Some(before) = source().await else { return AgentMemoryResult::Unavailable };
	let result = tokio::time::timeout(
		Duration::from_secs(25),
		before.client.memory_summary(&before.key.thread),
	)
	.await;
	let Some(after) = source().await else { return AgentMemoryResult::Unavailable };
	if before.key != after.key
		|| before.client.connection_identity() != after.client.connection_identity()
	{
		return AgentMemoryResult::Unavailable;
	}
	let (Ok(work_id), Ok(thread_id)) =
		(EntityId::new(before.key.work.clone()), WireText::new(before.key.thread.clone()))
	else {
		return AgentMemoryResult::Unavailable;
	};
	let Ok(Ok(summary)) = result else { return AgentMemoryResult::Unavailable };
	let Some(text) = summary else {
		return if offset == 0 && digest.is_none() {
			AgentMemoryResult::Empty { work_id, thread_id }
		} else {
			AgentMemoryResult::Unavailable
		};
	};
	page(&before, work_id, thread_id, &text, digest, offset)
		.unwrap_or(AgentMemoryResult::Unavailable)
}

fn page(
	source: &Source,
	work_id: EntityId,
	thread_id: WireText,
	text: &str,
	expected: Option<&str>,
	offset: usize,
) -> Option<AgentMemoryResult> {
	if offset >= text.len() || !text.is_char_boundary(offset) || (offset > 0 && expected.is_none())
	{
		return None;
	}
	let identity = serde_json::json!([
		source.key.work,
		source.key.thread,
		source.key.generation.as_str(),
		source.key.account.as_str(),
		source.key.revision,
		source.key.history_revision,
		source.client.connection_identity(),
		text
	]);
	let digest = Sha256::digest(identity.to_string().as_bytes())
		.iter()
		.map(|b| format!("{b:02x}"))
		.collect::<String>();
	if expected.is_some_and(|d| d != digest) {
		return None;
	}
	let mut end = (offset + MEMORY_PAGE_BYTES).min(text.len());
	while !text.is_char_boundary(end) {
		end -= 1;
	}
	Some(AgentMemoryResult::Page {
		work_id,
		thread_id,
		digest,
		offset,
		total_bytes: text.len(),
		text: text[offset..end].into(),
		next_offset: (end < text.len()).then_some(end),
	})
}

#[cfg(test)]
#[path = "agent_memory_tests.rs"]
mod tests;
