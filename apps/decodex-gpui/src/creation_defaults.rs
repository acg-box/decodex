//! Native new-thread default precedence shared by desktop creation flows.
use decodex_protocol::{DesktopCreationIntent, InitialExecutionDefaults, InitialModelDefaults};

pub(crate) fn resolve(
	defaults: &InitialModelDefaults,
	intent: &DesktopCreationIntent,
	explicit: InitialExecutionDefaults,
) -> InitialExecutionDefaults {
	let managed_pair = !intent.model && !intent.reasoning;
	InitialExecutionDefaults {
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
	}
}
