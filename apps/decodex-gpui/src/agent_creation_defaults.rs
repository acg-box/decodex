//! Resolve native defaults without converting observations into user intent.

#[cfg(test)]
use crate::shell::agent_surface::{
	ConversationReasoningEffort, ConversationWorkingDirectory, EntityId,
};
use crate::{
	creation_defaults,
	shell::agent_surface::{AgentSurface, Context, ConversationModel},
};
use decodex_protocol::{InitialExecutionDefaults, InitialModelCatalogResult, ServiceTier};

impl AgentSurface {
	pub(super) fn uses_initial_model_defaults(&self) -> bool {
		self.is_new_conversation()
			|| self.composer_reports_no_model()
			|| self
				.composer_manager
				.as_deref()
				.or(self.selected.as_deref())
				.and_then(|owner| self.conversation_record(owner))
				.is_none_or(|work| work.codex_thread_id.is_none())
	}

	pub(super) fn creation_defaults_need_refresh(&self, cx: &Context<Self>) -> bool {
		if (!self.is_new_conversation()
			&& (self.root_id().is_some() || self.composer_manager.is_some())
			&& !self.composer_needs_model_default(cx))
			|| (self.creation_intent.model
				&& self.creation_intent.reasoning
				&& self.creation_intent.service_tier)
		{
			return false;
		}
		if !self.creation_defaults_applied {
			return true;
		}

		let Some(InitialModelCatalogResult::Available {
			defaults: Some(defaults),
			working_directory,
			account_id,
			account_revision,
			..
		}) = self.creation_defaults.as_ref()
		else {
			return true;
		};
		let missing_model = !self.creation_intent.model
			&& (self.creation_intent.reasoning || defaults.managed.model.is_none())
			&& defaults.configured.model.is_none()
			&& defaults.catalog_model.is_none();

		missing_model
			|| self.current_model_catalog(cx).is_none()
			|| *account_revision <= 0
			|| working_directory.as_str() != self.cwd.read(cx).content()
			|| (!self.account.read(cx).content().is_empty()
				&& self.account.read(cx).content() != account_id.as_str())
	}

	pub(super) fn apply_creation_defaults(&mut self, cx: &mut Context<Self>) {
		let missing_native_model = self.composer_needs_model_default(cx);
		let owner =
			self.composer_manager.clone().or(self.selected.clone()).or_else(|| self.root_id());
		if self.sending
			|| self.uncertain
			|| (!self.uses_initial_model_defaults()
				&& (self.root_id().is_some() || self.composer_manager.is_some())
				&& !missing_native_model)
			|| self.current_model_catalog(cx).is_none()
		{
			return;
		}

		let Some(InitialModelCatalogResult::Available {
			defaults: Some(defaults),
			working_directory,
			account_id,
			account_revision,
			models,
			..
		}) = self.creation_defaults.as_ref()
		else {
			return;
		};

		if working_directory.as_str() != self.cwd.read(cx).content()
			|| *account_revision <= 0
			|| (!self.account.read(cx).content().is_empty()
				&& self.account.read(cx).content() != account_id.as_str())
		{
			return;
		}
		let current = self.composer_execution_defaults(cx);
		let retained = !self.uses_initial_model_defaults() && current.model.is_some();
		let pending = owner
			.as_deref()
			.map(|owner| self.draft_profiles.execution.choice(owner))
			.unwrap_or_default();
		let mut intent = self.creation_intent.clone();
		intent.model |= pending.model.is_some() || retained;
		intent.reasoning |= pending.reasoning_effort.is_some() || retained;
		intent.service_tier |= pending.service_tier.is_some() || retained;

		let selected = creation_defaults::resolve(
			defaults,
			&intent,
			InitialExecutionDefaults {
				model: pending.model.or(current.model),
				reasoning_effort: pending.reasoning_effort.or(current.reasoning_effort),
				service_tier: pending
					.service_tier
					.or(current.service_tier)
					.or_else(|| Some(ServiceTier::standard())),
			},
			models,
			&crate::session_settings::Defaults::load().last_execution(),
		);

		self.creation_defaults_applied = true;

		// Keep incomplete explicit edits visible, but never manufacture a native default model.
		if let Some(model) = selected.model {
			self.model.update(cx, |input, cx| input.set_content(model.as_str(), cx));
		} else if !self.creation_intent.model {
			return;
		}

		self.creation_inherit_effort = selected.reasoning_effort.is_none();

		if let Some(effort) = selected.reasoning_effort {
			self.effort = effort;
		}

		self.service_tier = selected.service_tier;
		self.creation_setup_present = true;

		// Retain the resolved group for this editor without starting a turn.
		if let Some(owner) = owner {
			let execution = decodex_protocol::AgentExecutionOverrides {
				model: ConversationModel::new(self.model.read(cx).content()).ok(),
				reasoning_effort: self.creation_effort(),
				service_tier: self.service_tier.clone(),
				permission_mode: pending.permission_mode,
			};
			if self.draft_profiles.execution.choice(&owner) != execution {
				self.draft_profiles.execution.change(owner, |choice| *choice = execution);
			}
		}
	}
}

#[cfg(test)]
mod tests {
	use gpui::AppContext as _;

	use crate::{
		creation_defaults,
		shell::agent_surface::creation_defaults::{
			AgentSurface, ConversationModel, ConversationReasoningEffort,
			ConversationWorkingDirectory, EntityId, InitialExecutionDefaults,
		},
	};
	use decodex_protocol::{
		DesktopCreationIntent, InitialModelCatalogResult, InitialModelDefaults, ServiceTier,
	};

	fn defaults() -> InitialModelDefaults {
		InitialModelDefaults {
			configured: InitialExecutionDefaults {
				model: Some(ConversationModel::new("project").unwrap()),
				reasoning_effort: None,
				service_tier: Some(ServiceTier::new("flex").unwrap()),
			},
			managed: InitialExecutionDefaults {
				model: Some(ConversationModel::new("managed").unwrap()),
				reasoning_effort: Some(ConversationReasoningEffort::Low),
				service_tier: Some(ServiceTier::new("priority").unwrap()),
			},
			catalog_model: Some(ConversationModel::new("catalog").unwrap()),
		}
	}

	#[gpui::test]
	fn fresh_creation_requires_observed_defaults_or_complete_explicit_intent(
		cx: &mut gpui::TestAppContext,
	) {
		let surface = cx.new(AgentSurface::new);

		surface.update(cx, |s, cx| {
			s.cwd.update(cx, |input, cx| input.set_content("/tmp", cx));
			s.composer.update(cx, |input, cx| input.set_content("Keep this unsent request", cx));

			assert!(s.creation_defaults_need_refresh(cx));
			assert!(s.composer_capability_error(cx).is_some());

			s.creation_defaults = Some(InitialModelCatalogResult::Unavailable);

			s.apply_creation_defaults(cx);

			assert!(s.creation_defaults_need_refresh(cx));
			assert_eq!(s.composer.read(cx).content(), "Keep this unsent request");
			assert!(s.submission.command.is_none() && s.submission.waiting.is_none());

			s.creation_intent.model = true;
			s.creation_intent.reasoning = true;

			assert!(s.creation_defaults_need_refresh(cx));

			s.creation_intent.service_tier = true;

			assert!(!s.creation_defaults_need_refresh(cx));

			s.creation_intent = Default::default();
			s.capabilities = Some(decodex_protocol::AgentCapabilitiesResult::Available {
				models: vec![],
				memory_enabled: None,
			});
			s.creation_defaults = Some(InitialModelCatalogResult::Available {
				account_id: EntityId::new("account").unwrap(),
				account_revision: 1,
				working_directory: ConversationWorkingDirectory::new("/tmp").unwrap(),
				models: vec![],
				defaults: Some(Box::new(defaults())),
			});

			s.apply_creation_defaults(cx);

			assert!(!s.creation_defaults_need_refresh(cx));
			assert_eq!(s.model.read(cx).content(), "catalog");
			assert_eq!(s.composer.read(cx).content(), "Keep this unsent request");
		});
	}

	#[gpui::test]
	fn latest_model_preserves_field_choices_and_survives_draft_restore(
		cx: &mut gpui::TestAppContext,
	) {
		let surface = cx.new(AgentSurface::new);
		let saved = surface.update(cx, |s, cx| {
			s.cwd.update(cx, |input, cx| input.set_content("/tmp", cx));

			s.capabilities = Some(decodex_protocol::AgentCapabilitiesResult::Available {
				models: vec![],
				memory_enabled: None,
			});
			s.creation_defaults = Some(InitialModelCatalogResult::Available {
				account_id: EntityId::new("account").unwrap(),
				account_revision: 1,
				working_directory: ConversationWorkingDirectory::new("/tmp").unwrap(),
				models: vec![],
				defaults: Some(Box::new(defaults())),
			});

			s.apply_creation_defaults(cx);

			assert_eq!(s.model.read(cx).content(), "catalog");
			assert_eq!(s.creation_effort(), Some(ConversationReasoningEffort::Low));
			assert!(!s.creation_intent.model && !s.creation_intent.reasoning);

			// An effort selection keeps the current default model.
			s.effort = ConversationReasoningEffort::High;

			s.mark_effort_intent(cx);

			assert_eq!(s.model.read(cx).content(), "catalog");
			assert_eq!(s.creation_effort(), Some(ConversationReasoningEffort::High));
			assert_eq!(s.service_tier.as_ref().unwrap().as_str(), "priority");
			assert!(!s.creation_intent.model && s.creation_intent.reasoning);
			assert!(s.submission.command.is_none(), "metadata and selections do not send");

			s.creation_setup(cx).unwrap()
		});
		let restored = cx.new(AgentSurface::new);

		restored.update(cx, |s, cx| {
			s.restore_creation_setup(Some(&saved), cx);

			assert_eq!(s.creation_intent, saved.intent);
			assert!(s.creation_defaults.is_none(), "re-read source after restart");
			assert!(s.creation_defaults_need_refresh(cx));
		});
	}

	#[gpui::test]
	fn missing_default_model_cannot_send(cx: &mut gpui::TestAppContext) {
		let surface = cx.new(AgentSurface::new);
		surface.update(cx, |s, cx| {
			s.cwd.update(cx, |input, cx| input.set_content("/tmp", cx));

			s.creation_intent = Default::default();
			s.capabilities = Some(decodex_protocol::AgentCapabilitiesResult::Available {
				models: vec![],
				memory_enabled: None,
			});
			s.creation_defaults = Some(InitialModelCatalogResult::Available {
				account_id: EntityId::new("account").unwrap(),
				account_revision: 1,
				working_directory: ConversationWorkingDirectory::new("/tmp").unwrap(),
				models: vec![],
				defaults: Some(Box::new(InitialModelDefaults {
					configured: Default::default(),
					managed: Default::default(),
					catalog_model: None,
				})),
			});

			s.apply_creation_defaults(cx);

			assert!(s.creation_defaults_need_refresh(cx));
			assert!(s.composer_capability_error(cx).is_some());
		});
	}

	#[test]
	fn explicit_model_keeps_native_effort_nullable_and_tier_independent() {
		let selected = creation_defaults::resolve(
			&defaults(),
			&DesktopCreationIntent { model: true, reasoning: false, service_tier: false },
			InitialExecutionDefaults {
				model: Some(ConversationModel::new("chosen").unwrap()),
				reasoning_effort: Some(ConversationReasoningEffort::High),
				service_tier: None,
			},
			&[],
			&Default::default(),
		);

		assert_eq!(selected.model.unwrap().as_str(), "chosen");
		assert!(selected.reasoning_effort.is_none(), "do not retain stale managed effort");
		assert_eq!(selected.service_tier.unwrap().as_str(), "priority");
	}

	#[gpui::test]
	fn retired_creation_model_uses_catalog_default_without_sending(cx: &mut gpui::TestAppContext) {
		let model = |id: &str| decodex_protocol::AgentModelDto {
			model: ConversationModel::new(id).unwrap(),
			name: id.into(),
			efforts: vec![ConversationReasoningEffort::High],
			default_effort: Some(ConversationReasoningEffort::High),
			supports_fast: false,
			service_tiers: vec![],
			default_service_tier: None,
			available_cyber_programs: None,
			specialty: None,
			supports_images: true,
			availability: None,
			upgrade: None,
		};
		let mut retired = model("old");
		retired.upgrade = Some(decodex_protocol::AgentModelUpgradeDto {
			model: ConversationModel::new("older-replacement").unwrap(),
			notice: None,
			retirement_at: Some(1),
		});
		let models = vec![retired, model("catalog")];
		let surface = cx.new(AgentSurface::new);
		surface.update(cx, |s, cx| {
			s.cwd.update(cx, |input, cx| input.set_content("/tmp", cx));
			s.model.update(cx, |input, cx| input.set_content("old", cx));
			s.composer.update(cx, |input, cx| input.set_content("Keep this draft", cx));
			s.creation_intent.model = true;
			s.capabilities = Some(decodex_protocol::AgentCapabilitiesResult::Available {
				models: models.clone(),
				memory_enabled: None,
			});
			s.creation_defaults = Some(InitialModelCatalogResult::Available {
				account_id: EntityId::new("account").unwrap(),
				account_revision: 1,
				working_directory: ConversationWorkingDirectory::new("/tmp").unwrap(),
				models,
				defaults: Some(Box::new(defaults())),
			});
			s.apply_creation_defaults(cx);
			assert_eq!(s.model.read(cx).content(), "catalog");
			assert_eq!(s.composer.read(cx).content(), "Keep this draft");
			assert!(s.submission.command.is_none() && s.submission.waiting.is_none());
		});
	}

	#[gpui::test]
	fn changed_source_and_pending_creation_do_not_apply_defaults(cx: &mut gpui::TestAppContext) {
		let surface = cx.new(AgentSurface::new);

		surface.update(cx, |s, cx| {
			s.capabilities = Some(decodex_protocol::AgentCapabilitiesResult::Available {
				models: vec![],
				memory_enabled: None,
			});
			s.creation_defaults = Some(InitialModelCatalogResult::Available {
				account_id: EntityId::new("account").unwrap(),
				account_revision: 1,
				working_directory: ConversationWorkingDirectory::new("/tmp").unwrap(),
				models: vec![],
				defaults: Some(Box::new(defaults())),
			});

			s.cwd.update(cx, |input, cx| input.set_content("/other", cx));
			s.apply_creation_defaults(cx);

			assert_eq!(s.model.read(cx).content(), "");

			s.cwd.update(cx, |input, cx| input.set_content("/tmp", cx));
			s.account.update(cx, |input, cx| input.set_content("different", cx));
			s.apply_creation_defaults(cx);

			assert_eq!(s.model.read(cx).content(), "");

			s.account.update(cx, |input, cx| input.set_content("account", cx));

			s.uncertain = true;

			s.apply_creation_defaults(cx);

			assert_eq!(s.model.read(cx).content(), "");
		});
	}
}
