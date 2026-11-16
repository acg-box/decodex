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
	remembered: &InitialExecutionDefaults,
) -> InitialExecutionDefaults {
	let managed_pair = !intent.model && !intent.reasoning;

	let mut selected = InitialExecutionDefaults {
		model: if intent.model {
			explicit.model
		} else {
			remembered
				.model
				.as_ref()
				.cloned()
				.or_else(|| defaults.catalog_model.clone())
				.or_else(|| managed_pair.then(|| defaults.managed.model.clone()).flatten())
				.or_else(|| defaults.configured.model.clone())
		},
		reasoning_effort: if intent.reasoning {
			explicit.reasoning_effort
		} else {
			remembered
				.reasoning_effort
				.clone()
				.or_else(|| {
					managed_pair.then(|| defaults.managed.reasoning_effort.clone()).flatten()
				})
				.or_else(|| defaults.configured.reasoning_effort.clone())
		},
		service_tier: if intent.service_tier {
			explicit.service_tier
		} else {
			remembered
				.service_tier
				.clone()
				.or_else(|| defaults.managed.service_tier.clone())
				.or_else(|| defaults.configured.service_tier.clone())
				.or_else(|| Some(decodex_protocol::ServiceTier::standard()))
		},
	};
	let now = SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.ok()
		.and_then(|value| i64::try_from(value.as_secs()).ok());
	if let Some((model, _)) = selected.model.as_ref().and_then(|current| {
		now.and_then(|now| {
			AgentModelDto::unavailable_default(
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
	if let Some(model) = selected
		.model
		.as_ref()
		.and_then(|selected| models.iter().find(|model| &model.model == selected))
		&& selected.reasoning_effort.as_ref().is_some_and(|effort| !model.efforts.contains(effort))
	{
		selected.reasoning_effort.clone_from(&model.default_effort);
	}
	if let Some(model) = selected
		.model
		.as_ref()
		.and_then(|selected| models.iter().find(|model| &model.model == selected))
	{
		selected.reasoning_effort =
			selected.reasoning_effort.or_else(|| model.default_effort.clone());
		selected.service_tier = Some(model.effective_service_tier(
			&selected.service_tier.unwrap_or_else(decodex_protocol::ServiceTier::standard),
		));
	}
	selected
}

#[cfg(test)]
mod tests {
	use super::{DesktopCreationIntent, InitialExecutionDefaults, InitialModelDefaults, resolve};
	use decodex_protocol::ConversationModel;

	#[test]
	fn remembered_model_precedes_the_current_catalog_default() {
		let model = |name| ConversationModel::new(name).unwrap();
		let defaults = InitialModelDefaults {
			configured: InitialExecutionDefaults {
				model: Some(model("old-config")),
				..Default::default()
			},
			managed: InitialExecutionDefaults {
				model: Some(model("old-default")),
				..Default::default()
			},
			catalog_model: Some(model("latest-default")),
		};
		let last = model("last-used");
		for (remembered, expected) in [(Some(&last), "last-used"), (None, "latest-default")] {
			let selected = resolve(
				&defaults,
				&DesktopCreationIntent::default(),
				Default::default(),
				&[],
				&InitialExecutionDefaults { model: remembered.cloned(), ..Default::default() },
			);
			assert_eq!(selected.model.unwrap().as_str(), expected);
		}
		let explicit = resolve(
			&defaults,
			&DesktopCreationIntent { model: true, ..Default::default() },
			InitialExecutionDefaults {
				model: Some(model("explicit-choice")),
				..Default::default()
			},
			&[],
			&InitialExecutionDefaults { model: Some(last), ..Default::default() },
		);
		assert_eq!(explicit.model.unwrap().as_str(), "explicit-choice");
	}
	fn catalog_model(id: &str) -> decodex_protocol::AgentModelDto {
		decodex_protocol::AgentModelDto {
			model: ConversationModel::new(id).unwrap(),
			name: id.into(),
			efforts: vec![
				decodex_protocol::ConversationReasoningEffort::Medium,
				decodex_protocol::ConversationReasoningEffort::High,
			],
			default_effort: Some(decodex_protocol::ConversationReasoningEffort::Medium),
			service_tiers: vec![decodex_protocol::AgentServiceTierDto {
				id: decodex_protocol::ServiceTier::priority(),
				name: "Fast".into(),
				description: String::new(),
			}],
			default_service_tier: None,
			supports_fast: true,
			supports_images: true,
			available_cyber_programs: None,
			specialty: None,
			availability: None,
			upgrade: None,
		}
	}

	#[test]
	fn new_conversations_use_the_recent_group_and_only_replace_unsupported_values() {
		use decodex_protocol::{ConversationReasoningEffort, ServiceTier};
		let models = vec![catalog_model("previous"), catalog_model("latest")];
		let defaults = InitialModelDefaults {
			configured: Default::default(),
			managed: Default::default(),
			catalog_model: Some(models[1].model.clone()),
		};
		let recent = InitialExecutionDefaults {
			model: Some(models[0].model.clone()),
			reasoning_effort: Some(ConversationReasoningEffort::High),
			service_tier: Some(ServiceTier::priority()),
		};
		let selected =
			resolve(&defaults, &Default::default(), Default::default(), &models, &recent);
		assert_eq!(
			selected, recent,
			"a newer catalog default must not replace supported recent choices"
		);

		let removed_model = InitialExecutionDefaults {
			model: Some(ConversationModel::new("removed").unwrap()),
			..recent.clone()
		};
		let selected =
			resolve(&defaults, &Default::default(), Default::default(), &models, &removed_model);
		assert_eq!(selected.model, defaults.catalog_model);
		assert_eq!(selected.reasoning_effort, recent.reasoning_effort);
		assert_eq!(selected.service_tier, recent.service_tier);

		let unsupported = InitialExecutionDefaults {
			model: recent.model.clone(),
			reasoning_effort: Some(ConversationReasoningEffort::Ultra),
			service_tier: Some(ServiceTier::new("ultrafast").unwrap()),
		};
		let selected =
			resolve(&defaults, &Default::default(), Default::default(), &models, &unsupported);
		assert_eq!(selected.model, recent.model);
		assert_eq!(selected.reasoning_effort, models[0].default_effort);
		assert_eq!(selected.service_tier, Some(ServiceTier::standard()));

		let initial = resolve(
			&defaults,
			&Default::default(),
			Default::default(),
			&models,
			&Default::default(),
		);
		assert_eq!(initial.model, defaults.catalog_model);
		assert_eq!(initial.reasoning_effort, models[1].default_effort);
		assert_eq!(initial.service_tier, Some(ServiceTier::standard()));
	}
}
