//! Real read-only engineering and research tasks through the product coordinator.
use super::{ChiefClient, SmokeResult, history, idle, wait_graph_for};
use decodex_protocol::{ChiefActivityDetailResult, ChiefWorkStatusDto, EntityId, WireText};

pub(super) fn prompt() -> String {
	"Validate this workspace with two actual read-only investigations. Create exactly two workers using Chief tools. Worker id engineering: inspect crates/decodex-runtime/src/chief_capabilities.rs and crates/decodex-runtime/src/account_launch/chief_process.rs; assess whether every RPC used to read native capabilities is admitted by the retained bridge, and whether private configuration can leak through the returned projection. Cite exact source paths and relevant symbols, distinguish findings from uncertainty, do not modify files. Worker id research: read work/voice-input.md and work/agent-memory-research.md; write a concise decision brief on which voice and memory capabilities are ready, which need verification, and why, citing those documents and distinguishing evidence from proposals. Do not browse or change files. You may read files and run read-only commands. Use only these two workers, no native subagents or extra work. After each actual report, assess it and record the appropriate disposition with exact event IDs. Do not mark missing or inconclusive results resolved. Summarize both results after review. Finish your initial dispatch turn with SERVICE_READY.".into()
}

pub(super) async fn qualify(client: &ChiefClient) -> SmokeResult<()> {
	let graph = wait_graph_for(
		client,
		"real engineering and research reports",
		std::time::Duration::from_secs(480),
		|graph| {
			graph.work_items.len() == 3
				&& idle(graph)
				&& graph
					.work_items
					.iter()
					.filter(|item| item.parent_goal_id.is_some())
					.all(|item| item.status == ChiefWorkStatusDto::Resolved)
		},
	)
	.await?;
	let mut report = String::from(
		"# Live Chief acceptance\n\nActual read-only provider tasks. No simulated tool results.\n",
	);
	let mut inspected_tool = false;
	for (id, source) in [("engineering", "chief_capabilities"), ("research", "voice-input")] {
		let entries = history(client, id).await?;
		let answer = entries
			.iter()
			.filter(|entry| entry.kind == "assistant")
			.map(|entry| entry.text.as_str())
			.collect::<Vec<_>>()
			.join("\n\n");
		if !answer.contains(source) {
			return Err("real report did not cite the requested source".into());
		}
		report.push_str(&format!("\n## {id}\n\n{answer}\n"));
		for item in entries
			.iter()
			.filter_map(|entry| entry.activity.as_ref())
			.filter(|item| item.kind == "commandExecution" && item.status == "completed")
		{
			if let ChiefActivityDetailResult::Available { text, .. } = client
				.activity_detail(
					EntityId::new(id).map_err(|_| "invalid work identity")?,
					WireText::new(&item.turn_id).map_err(|_| "invalid turn identity")?,
					WireText::new(&item.item_id).map_err(|_| "invalid item identity")?,
				)
				.await? && !text.is_empty()
			{
				inspected_tool = true;
				break;
			}
		}
	}
	if !inspected_tool {
		return Err("no real worker tool evidence was readable".into());
	}
	let root_report = history(client, "chief-service-smoke")
		.await?
		.into_iter()
		.filter(|entry| entry.kind == "assistant")
		.map(|entry| entry.text)
		.collect::<Vec<_>>()
		.join("\n\n");
	report.push_str(&format!(
		"\n## Chief assessment\n\n{root_report}\n\nWork records: {}\n",
		graph.work_items.len()
	));
	let destination = std::path::Path::new("target/chief-live-acceptance.md");
	std::fs::write(destination, report)?;
	println!(
		"Real engineering and research reports verified; native worker evidence read successfully. Report: {}",
		destination.display()
	);
	Ok(())
}
