//! Native new-thread default precedence shared by desktop creation flows.
use decodex_protocol::{
	AgentModelDto, DesktopCreationIntent, InitialExecutionDefaults, InitialModelDefaults,
};
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn resolve(
	defaults: &InitialModelDefaults,
	intent: &DesktopCreationIntent,
	explicit: InitialExecutionDefaults,
	models: &[AgentModelDto],
) -> InitialExecutionDefaults {
	let managed_pair = !intent.model && !intent.reasoning;

	let mut selected = InitialExecutionDefaults {
		model: if intent.model {
			explicit.model
		} else {
			managed_pair
				.then(|| defaults.managed.model.clone())
				.flatten()
				.or_else(|| defaults.configured.model.clone())
				.or_else(|| defaults.catalog_model.clone())
		},
		reasoning_effort: if intent.reasoning {
			explicit.reasoning_effort
		} else {
			managed_pair
				.then(|| defaults.managed.reasoning_effort.clone())
				.flatten()
				.or_else(|| defaults.configured.reasoning_effort.clone())
		},
		service_tier: if intent.service_tier {
			explicit.service_tier
		} else {
			defaults
				.managed
				.service_tier
				.clone()
				.or_else(|| defaults.configured.service_tier.clone())
		},
	};
	let now = SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.ok()
		.and_then(|value| i64::try_from(value.as_secs()).ok());
	if let Some((model, _)) = selected.model.as_ref().and_then(|current| {
		now.and_then(|now| {
			AgentModelDto::retired_default(
				current.as_str(),
				models,
				defaults.catalog_model.as_ref(),
				now,
			)
		})
	}) {
		selected.model = Some(model.model.clone());
		if selected.reasoning_effort.as_ref().is_some_and(|effort| !model.efforts.contains(effort))
		{
			selected.reasoning_effort = model.default_effort.clone();
		}
	}
	selected
}
