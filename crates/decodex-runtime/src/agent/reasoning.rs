//! Observe only the public summary channel. Native completion replaces partial parts.
use super::{AgentCoordinator, AgentError, Value, exact};
use decodex_database::AgentReasoningSummaryChange;

impl AgentCoordinator {
	pub(super) async fn observe_reasoning_summary(
		&self,
		method: &str,
		params: &Value,
	) -> Result<bool, AgentError> {
		let (item, change) = match method {
			"item/started" | "item/completed" if voice_handoff(&params["item"]) =>
				(exact(params, "/item/id")?, AgentReasoningSummaryChange::VoiceHandoff),
			"item/started" if params["item"]["type"] == "reasoning" => (
				exact(params, "/item/id")?,
				AgentReasoningSummaryChange::Delta { index: 0, text: String::new() },
			),
			"item/reasoning/summaryTextDelta" => {
				let index = params["summaryIndex"]
					.as_u64()
					.and_then(|index| usize::try_from(index).ok())
					.ok_or_else(|| AgentError::Invalid("Invalid reasoning summary part".into()))?;
				(
					exact(params, "/itemId")?,
					AgentReasoningSummaryChange::Delta { index, text: exact(params, "/delta")? },
				)
			},
			"item/completed" if params["item"]["type"] == "reasoning" => {
				let parts = params["item"]["summary"]
					.as_array()
					.and_then(|parts| {
						parts.iter().map(|part| part.as_str().map(str::to_owned)).collect()
					})
					.ok_or_else(|| AgentError::Invalid("Invalid reasoning summary".into()))?;
				(exact(params, "/item/id")?, AgentReasoningSummaryChange::Completed { parts })
			},
			_ => return Ok(false),
		};
		self.store
			.update_agent_reasoning_summary(
				exact(params, "/threadId")?,
				exact(params, "/turnId")?,
				item,
				self.native_generation.as_ref().map(|generation| generation.as_str().to_owned()),
				change,
			)
			.await?;
		Ok(true)
	}
}

pub(crate) fn voice_handoff(item: &Value) -> bool {
	if item["type"] != "userMessage" {
		return false;
	}
	let Some(content) = item["content"].as_array() else {
		return false;
	};
	let [part] = content.as_slice() else {
		return false;
	};
	if part["type"] != "text"
		|| part["textElements"].as_array().is_some_and(|elements| !elements.is_empty())
	{
		return false;
	}
	part["text"]
		.as_str()
		.and_then(|text| text.trim().strip_prefix("<realtime_delegation>"))
		.and_then(|body| body.strip_suffix("</realtime_delegation>"))
		.and_then(|body| body.split_once("<input>"))
		.is_some_and(|(_, input)| input.contains("</input>"))
}
