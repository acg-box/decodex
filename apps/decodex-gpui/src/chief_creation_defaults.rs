//! Resolve native defaults without converting observations into user intent.
use super::*;
use decodex_protocol::{DesktopCreationIntent, InitialExecutionDefaults, InitialModelDefaults};

fn resolve(
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

impl ChiefSurface {
	pub(super) fn creation_defaults_need_refresh(&self, cx: &Context<Self>) -> bool {
		if !self.creation_defaults_applied
			|| self.root_id().is_some()
			|| self.composer_manager.is_some()
			|| (self.creation_intent.model
				&& self.creation_intent.reasoning
				&& self.creation_intent.service_tier)
		{
			return false;
		}
		let Some(decodex_protocol::InitialModelCatalogResult::Available {
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
		if self.sending
			|| self.uncertain
			|| self.root_id().is_some()
			|| self.composer_manager.is_some()
			|| self.current_model_catalog(cx).is_none()
		{
			return;
		}
		let Some(decodex_protocol::InitialModelCatalogResult::Available {
			defaults: Some(defaults),
			working_directory,
			account_id,
			account_revision,
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
		let selected = resolve(
			defaults,
			&self.creation_intent,
			InitialExecutionDefaults {
				model: ConversationModel::new(self.model.read(cx).content()).ok(),
				reasoning_effort: self.creation_effort(),
				service_tier: self
					.service_tier
					.clone()
					.or_else(|| Some(decodex_protocol::ServiceTier::from_fast(self.fast))),
			},
		);
		self.creation_defaults_applied = true;
		// Keep incomplete explicit edits visible, but never manufacture a native default model.
		if !self.creation_intent.model {
			let Some(model) = selected.model else { return };
			self.model.update(cx, |input, cx| input.set_content(model.as_str(), cx));
		}
		self.creation_inherit_effort = selected.reasoning_effort.is_none();
		if let Some(effort) = selected.reasoning_effort {
			self.effort = effort;
		}
		self.service_tier = selected.service_tier;
		self.fast = self.service_tier.as_ref().is_some_and(|tier| tier.as_str() == "priority");
		self.creation_setup_present = true;
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use decodex_protocol::{InitialModelCatalogResult, ServiceTier};
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
	fn managed_pair_opts_out_per_field_and_survives_draft_restore(cx: &mut gpui::TestAppContext) {
		let surface = cx.new(ChiefSurface::new);
		let saved = surface.update(cx, |s, cx| {
			s.cwd.update(cx, |input, cx| input.set_content("/tmp", cx));
			s.capabilities = Some(decodex_protocol::ChiefCapabilitiesResult::Available {
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
			assert_eq!(s.model.read(cx).content(), "managed");
			assert_eq!(s.creation_effort(), Some(ConversationReasoningEffort::Low));
			assert!(!s.creation_intent.model && !s.creation_intent.reasoning);
			// Selecting effort opts out of the managed model, not just managed effort.
			s.effort = ConversationReasoningEffort::High;
			s.mark_effort_intent(cx);
			assert_eq!(s.model.read(cx).content(), "project");
			assert_eq!(s.creation_effort(), Some(ConversationReasoningEffort::High));
			assert_eq!(s.service_tier.as_ref().unwrap().as_str(), "priority");
			assert!(!s.creation_intent.model && s.creation_intent.reasoning);
			assert!(s.submission.command.is_none(), "metadata and selections do not send");
			s.creation_setup(cx).unwrap()
		});
		let restored = cx.new(ChiefSurface::new);
		restored.update(cx, |s, cx| {
			s.restore_creation_setup(Some(&saved), cx);
			assert_eq!(s.creation_intent, saved.intent.unwrap());
			assert!(s.creation_defaults.is_none(), "re-read source after restart");
			assert!(s.creation_defaults_need_refresh(cx));
		});
	}

	#[gpui::test]
	fn legacy_setup_stays_explicit_and_missing_default_model_cannot_send(
		cx: &mut gpui::TestAppContext,
	) {
		let surface = cx.new(ChiefSurface::new);
		surface.update(cx, |s, cx| {
			s.cwd.update(cx, |input, cx| input.set_content("/tmp", cx));
			let setup = s.creation_setup(cx).unwrap();
			let mut old = serde_json::to_value(setup).unwrap();
			old.as_object_mut().unwrap().remove("intent");
			old.as_object_mut().unwrap().remove("defaults_applied");
			s.restore_creation_setup(Some(&serde_json::from_value(old).unwrap()), cx);
			assert!(
				s.creation_intent.model
					&& s.creation_intent.reasoning
					&& s.creation_intent.service_tier
			);
			assert!(!s.creation_defaults_need_refresh(cx));
			s.creation_intent = Default::default();
			s.capabilities = Some(decodex_protocol::ChiefCapabilitiesResult::Available {
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
		let selected = resolve(
			&defaults(),
			&DesktopCreationIntent { model: true, reasoning: false, service_tier: false },
			InitialExecutionDefaults {
				model: Some(ConversationModel::new("chosen").unwrap()),
				reasoning_effort: Some(ConversationReasoningEffort::High),
				service_tier: None,
			},
		);
		assert_eq!(selected.model.unwrap().as_str(), "chosen");
		assert!(selected.reasoning_effort.is_none(), "do not retain stale managed effort");
		assert_eq!(selected.service_tier.unwrap().as_str(), "priority");
	}

	#[gpui::test]
	fn changed_source_and_pending_creation_do_not_apply_defaults(cx: &mut gpui::TestAppContext) {
		let surface = cx.new(ChiefSurface::new);
		surface.update(cx, |s, cx| {
			s.capabilities = Some(decodex_protocol::ChiefCapabilitiesResult::Available {
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
			assert_eq!(s.model.read(cx).content(), creation_setup::DEFAULT_MODEL);
			s.cwd.update(cx, |input, cx| input.set_content("/tmp", cx));
			s.account.update(cx, |input, cx| input.set_content("different", cx));
			s.apply_creation_defaults(cx);
			assert_eq!(s.model.read(cx).content(), creation_setup::DEFAULT_MODEL);
			s.account.update(cx, |input, cx| input.set_content("account", cx));
			s.uncertain = true;
			s.apply_creation_defaults(cx);
			assert_eq!(s.model.read(cx).content(), creation_setup::DEFAULT_MODEL);
		});
	}
}
