//! Runtime model catalog. Loading metadata never sends a conversation message.
use std::time::Instant;

use gpui::{
	AnyElement, SharedString,
	prelude::{
		FluentBuilder, InteractiveElement, IntoElement, ParentElement, StatefulInteractiveElement,
		Styled,
	},
};
use tokio::runtime::Builder;

use crate::{
	shell::agent_surface::{AgentClient, AgentSurface, Context},
	ui_theme::{HOVER_FILL, SELECTED_HOVER_FILL},
};
use decodex_protocol::{
	AgentCapabilitiesResult, AgentModelDto, ConversationReasoningEffort,
	ConversationWorkingDirectory, InitialModelCatalogRequest, ModelCatalogPurpose, ServiceTier,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct CatalogContext {
	directory: String,
	account: String,
	has_root: bool,
	runtime_source: Option<decodex_protocol::EntityId>,
}

impl AgentSurface {
	pub(super) fn service_tier_picker(&self, cx: &Context<Self>) -> AnyElement {
		let mut panel = gpui::div().id("service-tier-picker").flex().items_center().gap_1();
		let supports_fast = self.selected_model(cx).is_some_and(|model| model.supports_fast);
		let tiers = [
			(ServiceTier::standard(), "Standard", true),
			(ServiceTier::from_fast(true), "Fast", supports_fast),
		];
		let selected = if let Some(owner) = self.composer_manager.clone().or_else(|| self.root_id())
		{
			self.draft_profiles.execution.choice(&owner).selected_service_tier()
		} else {
			Some(self.service_tier.clone().unwrap_or_else(|| ServiceTier::from_fast(self.fast)))
		};
		let selected = selected.unwrap_or_else(|| {
			self.service_tier.clone().unwrap_or_else(|| ServiceTier::from_fast(self.fast))
		});

		for (id, label, available) in tiers {
			let chosen = selected == id;

			panel = panel.child(
				gpui::div()
					.id(SharedString::from(format!("tier-{}", id.as_str())))
					.debug_selector({
						let label = format!("tier-{}", id.as_str());

						move || label.clone()
					})
					.when(available, |d| d.cursor_pointer())
					.when(!available, |d| d.opacity(0.4))
					.px_2()
					.py_1()
					.child(format!("{}{}", if chosen { "✓ " } else { "" }, label))
					.text_size(gpui::px(11.))
					.rounded(gpui::px(8.))
					.bg(gpui::rgba(if chosen { 0xffffff16 } else { 0x00000000 }))
					.hover(move |s| {
						s.bg(gpui::rgba(if chosen { SELECTED_HOVER_FILL } else { HOVER_FILL }))
					})
					.on_click(cx.listener(move |s, _, _, cx| {
						if available {
							s.fast = id.as_str() == "priority";
							s.service_tier = Some(id.clone());

							s.mark_tier_intent();
							s.save_draft_document(cx);
							cx.notify();
						}
					})),
			);
		}

		panel.into_any_element()
	}

	fn catalog_context(&self, cx: &Context<Self>) -> Option<CatalogContext> {
		Some(CatalogContext {
			directory: self.cwd.read(cx).content().to_owned(),
			account: self.account.read(cx).content().to_owned(),
			has_root: self
				.snapshot
				.as_ref()?
				.work_items
				.iter()
				.any(|work| work.parent_goal_id.is_none()),
			runtime_source: self.snapshot.as_ref()?.runtime_source.clone(),
		})
	}

	pub(super) fn current_model_catalog(
		&self,
		cx: &Context<Self>,
	) -> Option<&AgentCapabilitiesResult> {
		if self
			.capabilities_context
			.as_ref()
			.is_some_and(|source| Some(source) != self.catalog_context(cx).as_ref())
		{
			return None;
		}

		self.capabilities.as_ref()
	}

	pub(super) fn reset_capabilities(&mut self) {
		self.capability_generation += 1;
		self.capability_task = None;
		self.capabilities = None;
		self.capabilities_context = None;
		self.capabilities_checked = None;
		self.creation_defaults = None;
	}

	pub(super) fn load_capabilities(&mut self, cx: &mut Context<Self>) {
		if self.capability_task.is_some() {
			return;
		}

		let Some(profile) = self.profile.clone() else {
			return;
		};
		let Some(context) = self.catalog_context(cx) else {
			return;
		};
		let cold_request = if context.has_root {
			None
		} else {
			let Ok(working_directory) =
				ConversationWorkingDirectory::new(context.directory.clone())
			else {
				return;
			};
			let account_id = if context.account.is_empty() {
				None
			} else {
				let Ok(account) = decodex_protocol::EntityId::new(context.account.clone()) else {
					return;
				};

				Some(account)
			};

			Some(InitialModelCatalogRequest {
				working_directory,
				account_id,
				purpose: ModelCatalogPurpose::Agent,
			})
		};

		self.capabilities_checked = Some(Instant::now());
		self.capability_generation += 1;

		let generation = self.capability_generation;
		let request = cx.background_executor().spawn(async move {
			let runtime = Builder::new_current_thread().enable_all().build().ok()?;
			let client = AgentClient::new(profile);

			if let Some(request) = cold_request {
				let directory = request.working_directory.clone();

				match runtime.block_on(client.initial_model_catalog(request)).ok()? {
					result @ decodex_protocol::InitialModelCatalogResult::Available { .. } => {
						let decodex_protocol::InitialModelCatalogResult::Available {
							models,
							account_revision,
							working_directory,
							..
						} = &result
						else {
							unreachable!()
						};

						if *account_revision <= 0 || working_directory != &directory {
							return Some((AgentCapabilitiesResult::Unavailable, None));
						}

						Some((
							AgentCapabilitiesResult::Available {
								models: models.clone(),
								memory_enabled: None,
							},
							Some(result),
						))
					},
					_ => Some((AgentCapabilitiesResult::Unavailable, None)),
				}
			} else {
				runtime.block_on(client.capabilities()).ok().map(|result| (result, None))
			}
		});

		self.capability_task = Some(cx.spawn(async move |surface, cx| {
			let result = request.await;
			let _ = surface.update(cx, |surface, cx| {
				surface.finish_capabilities(generation, context, result, cx);
			});
		}));
	}

	fn finish_capabilities(
		&mut self,
		generation: u64,
		context: CatalogContext,
		result: Option<(
			AgentCapabilitiesResult,
			Option<decodex_protocol::InitialModelCatalogResult>,
		)>,
		cx: &mut Context<Self>,
	) {
		if self.capability_generation != generation {
			return;
		}

		self.capability_task = None;

		if self.catalog_context(cx).as_ref() != Some(&context) {
			self.capabilities_checked = None;

			self.load_capabilities(cx);

			return;
		}

		self.capabilities_context = Some(context);
		self.creation_defaults = result.as_ref().and_then(|(_, defaults)| defaults.clone());
		self.capabilities = result.map(|(capabilities, _)| capabilities);

		self.apply_creation_defaults(cx);
		self.reconcile_model_options(cx);
		self.save_draft_document(cx);
		cx.notify();
	}

	pub(super) fn selected_model(&self, cx: &Context<Self>) -> Option<&AgentModelDto> {
		let Some(AgentCapabilitiesResult::Available { models, .. }) =
			self.current_model_catalog(cx)
		else {
			return None;
		};
		let selected = self.composer_model_value(cx)?;

		models.iter().find(|model| model.model.as_str() == selected)
	}

	pub(super) fn model_efforts(&self, cx: &Context<Self>) -> Vec<ConversationReasoningEffort> {
		self.selected_model(cx)
			.map_or_else(|| vec![self.effort.clone()], |model| model.efforts.clone())
	}

	pub(super) fn reconcile_model_options(&mut self, cx: &mut Context<Self>) {
		let intent = self
			.composer_manager
			.clone()
			.or_else(|| self.root_id())
			.map(|owner| self.draft_profiles.execution.choice(&owner));
		let before_tier = self.service_tier.clone();

		if let Some(model) = self.selected_model(cx).cloned() {
			if !model.supports_fast {
				self.fast = false;
			}
			if self.service_tier.as_ref().is_some_and(|selected| {
				!matches!(selected.as_str(), "default" | "flex")
					&& !model.service_tiers.iter().any(|tier| &tier.id == selected)
			}) {
				self.service_tier = Some(ServiceTier::standard());
				self.fast = false;
			}
		}

		if intent.as_ref().is_some_and(|choice| choice.selected_service_tier().is_some())
			&& self.service_tier != before_tier
		{
			self.mark_tier_intent();
		}
	}

	pub(super) fn reconcile_selected_model_effort(&mut self, cx: &mut Context<Self>) {
		if self.root_id().is_none() && self.creation_inherit_effort {
			return;
		}

		if let Some(model) = self.selected_model(cx)
			&& !model.efforts.contains(&self.effort)
			&& let Some(effort) =
				model.default_effort.clone().or_else(|| model.efforts.first().cloned())
		{
			self.effort = effort;

			self.mark_effort_intent(cx);
		}
	}

	pub(super) fn composer_capability_error(&self, cx: &Context<Self>) -> Option<&'static str> {
		if self.creation_defaults_need_refresh(cx) {
			return Some("Refresh account defaults for this directory before sending.");
		}

		let owner = self.composer_manager.clone().or_else(|| self.root_id());
		let choice = owner.as_deref().map(|owner| self.draft_profiles.execution.choice(owner));
		let effort = choice
			.as_ref()
			.map_or(self.creation_effort(), |choice| choice.reasoning_effort.clone());
		let tier = choice.as_ref().map_or_else(
			|| Some(self.service_tier.clone().unwrap_or_else(|| ServiceTier::from_fast(self.fast))),
			|choice| choice.selected_service_tier(),
		);
		let Some(model) = self.selected_model(cx) else {
			return tier
				.as_ref()
				.is_some_and(|tier| !matches!(tier.as_str(), "default" | "flex"))
				.then_some("Refresh model capabilities before selecting a service tier.");
		};

		if tier.as_ref().is_some_and(|selected| {
			!matches!(selected.as_str(), "default" | "flex")
				&& !model.service_tiers.iter().any(|tier| &tier.id == selected)
		}) {
			return Some("This service tier is unavailable for the selected model.");
		}
		if !model.efforts.is_empty()
			&& effort.is_some_and(|effort| !model.efforts.contains(&effort))
		{
			return Some("This model's reasoning levels are not supported by this version.");
		}
		if tier.as_ref().is_some_and(|tier| tier.as_str() == "priority") && !model.supports_fast {
			return Some("Fast mode is not available for this model.");
		}
		if !model.supports_images && self.attachments.iter().any(|file| file.image) {
			return Some(
				"This model does not accept images. Choose another model or remove the image.",
			);
		}

		None
	}
}

#[cfg(test)]
#[path = "agent_effort_catalog_tests.rs"]
mod effort_tests;
#[cfg(test)]
mod tests {
	use gpui::AppContext as _;

	use crate::shell::agent_surface::capabilities::*;

	use std::{future, thread};

	#[gpui::test]
	fn catalog_reply_survives_unrelated_snapshot_refresh(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		surface.update(visual, |surface, cx| {
			surface.visual_workspace_fixture(cx);

			let context = surface.catalog_context(cx).unwrap();

			surface.capability_generation = 7;
			surface.capability_task = Some(cx.spawn(async |_, _| future::pending().await));
			surface.generation += 1;

			surface.finish_capabilities(
				7,
				context,
				Some((AgentCapabilitiesResult::Unavailable, None)),
				cx,
			);

			assert!(surface.capability_task.is_none(), "completed request releases its slot");
			assert!(matches!(
				surface.current_model_catalog(cx),
				Some(AgentCapabilitiesResult::Unavailable)
			));
		});
	}

	#[gpui::test]
	fn catalog_replies_cannot_cross_runtime_or_profile_boundaries(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		surface.update(visual, |surface, cx| {
			surface.visual_workspace_fixture(cx);

			let old = surface.catalog_context(cx).unwrap();

			surface.capabilities_context = Some(old.clone());
			surface.capabilities = Some(AgentCapabilitiesResult::Unavailable);
			surface.snapshot.as_mut().unwrap().runtime_source =
				Some(decodex_protocol::EntityId::new("replacement-runtime").unwrap());

			assert!(surface.current_model_catalog(cx).is_none());

			surface.capability_generation = 4;
			surface.capabilities_checked = Some(std::time::Instant::now());

			surface.finish_capabilities(4, old, None, cx);

			assert!(
				surface.capabilities_checked.is_none(),
				"source mismatch permits a fresh request"
			);
			assert!(surface.current_model_catalog(cx).is_none());

			let current = surface.catalog_context(cx).unwrap();

			surface.bind_profile(None, cx);

			let new_generation = surface.capability_generation;

			surface.capability_task = Some(cx.spawn(async |_, _| future::pending().await));

			surface.finish_capabilities(
				4,
				current,
				Some((AgentCapabilitiesResult::Unavailable, None)),
				cx,
			);

			assert!(surface.capabilities.is_none());
			assert_eq!(surface.capability_generation, new_generation);
			assert!(surface.capability_task.is_some(), "old reply cannot retire a newer request");
		});
	}

	#[gpui::test]
	fn disconnected_catalog_reply_cannot_publish_or_retire_a_new_request(
		cx: &mut gpui::TestAppContext,
	) {
		let surface = cx.new(AgentSurface::new);

		surface.update(cx, |s, cx| {
			s.visual_workspace_fixture(cx);

			let context = s.catalog_context(cx).unwrap();

			s.capability_generation = 7;
			s.capability_task = Some(cx.spawn(async |_, _| future::pending().await));

			s.mark_stale(cx);

			assert!(s.capability_task.is_none());

			s.capability_task = Some(cx.spawn(async |_, _| future::pending().await));

			s.finish_capabilities(
				7,
				context,
				Some((AgentCapabilitiesResult::Unavailable, None)),
				cx,
			);

			assert!(s.capabilities.is_none() && s.creation_defaults.is_none());
			assert!(s.capability_task.is_some());
		});
	}

	#[gpui::test]
	fn speed_picker_only_offers_standard_and_fast_and_rechecks_catalog(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.model.update(cx, |input, cx| input.set_content("custom", cx));
			s.mark_model_intent(cx);

			s.capabilities = Some(AgentCapabilitiesResult::Available {
				memory_enabled: None,
				models: vec![AgentModelDto {
					model: decodex_protocol::ConversationModel::new("custom").unwrap(),
					name: "Custom".into(),
					efforts: vec![ConversationReasoningEffort::High],
					default_effort: Some(ConversationReasoningEffort::High),
					supports_fast: true,
					available_cyber_programs: None,
					specialty: None,
					supports_images: true,
					availability: None,
					upgrade: None,
					service_tiers: vec![decodex_protocol::AgentServiceTierDto {
						id: decodex_protocol::ServiceTier::from_fast(true),
						name: "Priority".into(),
						description: "Increased usage".into(),
					}],
					default_service_tier: Some(
						decodex_protocol::ServiceTier::new("ultrafast").unwrap(),
					),
				}],
			});

			s.reconcile_model_options(cx);

			assert!(
				s.service_tier.is_none(),
				"catalog default must not silently opt into a paid tier"
			);

			s.composer_menu = Some("model");
			s.composer_menu_content = Some("model");
		});

		visual.update(|window, cx| {
			window.resize(gpui::size(gpui::px(1_280.), gpui::px(1_400.)));
			window.draw(cx).clear();
		});

		// The popover uses a real-time entrance translation; click its settled bounds.
		thread::sleep(std::time::Duration::from_millis(220));

		visual.update(|window, cx| {
			window.draw(cx).clear();
		});

		assert!(visual.debug_bounds("tier-inherited").is_none());
		assert!(visual.debug_bounds("tier-default").is_some());
		assert!(visual.debug_bounds("tier-ultrafast").is_none());

		let bounds = visual.debug_bounds("tier-priority").expect("Fast tier visible");

		visual.simulate_click(bounds.center(), gpui::Modifiers::default());

		surface.update(visual, |s, cx| {
			assert_eq!(s.service_tier.as_ref().unwrap().as_str(), "priority");
			assert!(s.composer_capability_error(cx).is_none());

			if let Some(AgentCapabilitiesResult::Available { models, .. }) = &mut s.capabilities {
				models[0].service_tiers.clear();

				models[0].supports_fast = false;
			}

			s.reconcile_model_options(cx);

			assert_eq!(s.service_tier.as_ref().unwrap().as_str(), "default");

			let owner = s.composer_manager.clone().or_else(|| s.root_id()).unwrap();

			assert_eq!(
				s.draft_profiles.execution.choice(&owner).selected_service_tier().unwrap().as_str(),
				"default"
			);
		});
	}

	#[gpui::test]
	fn catalog_context_changes_disable_previous_account_options(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.capabilities =
				Some(AgentCapabilitiesResult::Available { models: vec![], memory_enabled: None });
			s.capabilities_context = s.catalog_context(cx);

			assert!(s.current_model_catalog(cx).is_some());

			s.account.update(cx, |input, cx| {
				input.set_content("00000000-0000-4000-8000-000000000099", cx)
			});

			assert!(s.current_model_catalog(cx).is_none());

			s.capabilities_context = s.catalog_context(cx);

			assert!(s.current_model_catalog(cx).is_some());

			s.cwd.update(cx, |input, cx| input.set_content("/different-project", cx));

			assert!(s.current_model_catalog(cx).is_none());
		});
	}

	#[gpui::test]
	fn configured_flex_survives_missing_catalog_and_fast_support(cx: &mut gpui::TestAppContext) {
		let surface = cx.new(AgentSurface::new);

		for catalog in [false, true] {
			surface.update(cx, |s, cx| {
				s.visual_workspace_fixture(cx);
				s.model.update(cx, |input, cx| input.set_content("configured-model", cx));
				s.mark_model_intent(cx);

				s.effort = ConversationReasoningEffort::High;

				s.mark_effort_intent(cx);

				s.fast = false;
				s.service_tier = Some(decodex_protocol::ServiceTier::new("flex").unwrap());

				s.mark_tier_intent();

				s.capabilities = if catalog {
					Some(AgentCapabilitiesResult::Available {
						memory_enabled: None,
						models: vec![AgentModelDto {
							model: decodex_protocol::ConversationModel::new("configured-model")
								.unwrap(),
							name: "Configured".into(),
							efforts: vec![ConversationReasoningEffort::High],
							default_effort: Some(ConversationReasoningEffort::High),
							supports_fast: false,
							available_cyber_programs: None,
							specialty: None,
							supports_images: true,
							availability: None,
							upgrade: None,
							service_tiers: vec![],
							default_service_tier: None,
						}],
					})
				} else {
					None
				};

				s.reconcile_model_options(cx);

				assert_eq!(s.service_tier.as_ref().unwrap().as_str(), "flex");

				let owner = s.composer_manager.clone().or_else(|| s.root_id()).unwrap();

				assert_eq!(
					s.draft_profiles
						.execution
						.choice(&owner)
						.selected_service_tier()
						.unwrap()
						.as_str(),
					"flex"
				);
				assert!(s.composer_capability_error(cx).is_none());
			});
		}
	}
}
