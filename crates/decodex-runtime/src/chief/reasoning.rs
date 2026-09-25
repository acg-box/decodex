//! Observe only the public summary channel. Native completion replaces partial parts.
use super::{ChiefCoordinator, ChiefError, Value, exact};
use decodex_database::ChiefReasoningSummaryChange;

impl ChiefCoordinator {
	pub(super) async fn observe_reasoning_summary(
		&self,
		method: &str,
		params: &Value,
	) -> Result<bool, ChiefError> {
		let (item, change) = match method {
			"item/started" | "item/completed" if voice_handoff(&params["item"]) =>
				(exact(params, "/item/id")?, ChiefReasoningSummaryChange::VoiceHandoff),
			"item/started" if params["item"]["type"] == "reasoning" => (
				exact(params, "/item/id")?,
				ChiefReasoningSummaryChange::Delta { index: 0, text: String::new() },
			),
			"item/reasoning/summaryTextDelta" => {
				let index = params["summaryIndex"]
					.as_u64()
					.and_then(|index| usize::try_from(index).ok())
					.ok_or_else(|| ChiefError::Invalid("Invalid reasoning summary part".into()))?;
				(
					exact(params, "/itemId")?,
					ChiefReasoningSummaryChange::Delta { index, text: exact(params, "/delta")? },
				)
			},
			"item/completed" if params["item"]["type"] == "reasoning" => {
				let parts = params["item"]["summary"]
					.as_array()
					.and_then(|parts| {
						parts.iter().map(|part| part.as_str().map(str::to_owned)).collect()
					})
					.ok_or_else(|| ChiefError::Invalid("Invalid reasoning summary".into()))?;
				(exact(params, "/item/id")?, ChiefReasoningSummaryChange::Completed { parts })
			},
			_ => return Ok(false),
		};
		self.store
			.update_chief_reasoning_summary(
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

pub(super) fn voice_handoff(item: &Value) -> bool {
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
