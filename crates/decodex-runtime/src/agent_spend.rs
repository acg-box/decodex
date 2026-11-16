//! Add exact-turn API price equivalents to the existing history projections.
use std::collections::BTreeSet;

use crate::{agent_api_pricing, agent_host::AgentHost, agent_usage_estimate};
use decodex_protocol::{
	AgentHistoryResult, AgentSpendDto, AgentTimelineContent, AgentTimelineResult, AgentTurnUsageDto,
};

type Usage<'a> = (String, String, &'a mut AgentTurnUsageDto);

impl AgentHost {
	pub(crate) async fn enrich_timeline_spend(&self, work: &str, result: &mut AgentTimelineResult) {
		self.enrich_spend(work, timeline_usages(result)).await;
		// Optional pricing must not replace or shorten native conversation content.
		if serde_json::to_vec(result).is_ok_and(|bytes| bytes.len() > 60 * 1024) {
			clear_spend(timeline_usages(result));
		}
	}

	pub(crate) async fn enrich_history_spend(&self, work: &str, history: &mut AgentHistoryResult) {
		let Ok(owner) = self.store.get_agent_work_item(work.into()).await else { return };
		let Some(thread) = owner.codex_thread_id else { return };
		self.enrich_spend(work, history_usages(history, &thread)).await;
		if serde_json::to_vec(history).is_ok_and(|bytes| bytes.len() > 60 * 1024) {
			clear_spend(history_usages(history, &thread));
		}
	}

	async fn enrich_spend(&self, work: &str, mut usages: Vec<Usage<'_>>) {
		if usages.is_empty() {
			return;
		}
		let catalog = self.api_prices.snapshot().await;
		let threads = usages.iter().map(|(thread, _, _)| thread.clone()).collect::<BTreeSet<_>>();
		for thread in threads {
			let Some(before) = self.timeline_source(work, &thread).await else { continue };
			let Ok((rows, truncated)) =
				self.store.read_agent_cost_observations(work.into(), thread.clone()).await
			else {
				continue;
			};
			let estimates = agent_api_pricing::calculate_turns(&catalog, &thread, &rows, truncated);
			if self
				.timeline_source(work, &thread)
				.await
				.is_none_or(|after| !agent_usage_estimate::same_owner(&before, &after))
			{
				continue;
			}
			for (_, turn, usage) in usages.iter_mut().filter(|(source, _, _)| source == &thread) {
				if let Some(estimate) = estimates.get(turn) {
					usage.details.get_or_insert_with(Default::default).spend=Some(AgentSpendDto {
						usd_micros:estimate.usd_micros,estimated:true,partial:estimate.partial,
						explanation:"API price equivalent from recorded model, speed and token usage, including cached input. This is an estimate of model usage value, not a subscription invoice.".into(),
					});
				}
			}
		}
	}
}

fn timeline_usages(result: &mut AgentTimelineResult) -> Vec<Usage<'_>> {
	match result {
		AgentTimelineResult::Available { page, .. } => page
			.entries
			.iter_mut()
			.filter_map(|entry| {
				let thread = page.source_threads.get(&entry.position).unwrap_or(&page.thread_id);
				content_usage(&mut entry.content, thread)
			})
			.collect(),
		AgentTimelineResult::Saved { items, thread_id, .. }
		| AgentTimelineResult::Summary { items, thread_id, .. } =>
			items.iter_mut().filter_map(|content| content_usage(content, thread_id)).collect(),
		_ => Vec::new(),
	}
}

fn content_usage<'a>(content: &'a mut AgentTimelineContent, thread: &str) -> Option<Usage<'a>> {
	match content {
		AgentTimelineContent::TurnBoundary {
			turn_id, completed: true, usage: Some(usage), ..
		} => Some((thread.into(), turn_id.clone(), usage)),
		_ => None,
	}
}

fn history_usages<'a>(history: &'a mut AgentHistoryResult, thread: &str) -> Vec<Usage<'a>> {
	let AgentHistoryResult::Available { entries, .. } = history else { return Vec::new() };
	entries
		.iter_mut()
		.filter_map(|entry| {
			Some((
				entry.native_source.as_ref().map_or(thread, |source| &source.thread_id).into(),
				entry.turn_id.clone()?,
				entry.usage.as_mut()?,
			))
		})
		.collect()
}

fn clear_spend(usages: Vec<Usage<'_>>) {
	for (_, _, usage) in usages {
		if let Some(details) = usage.details.as_mut() {
			details.spend = None;
		}
	}
}
