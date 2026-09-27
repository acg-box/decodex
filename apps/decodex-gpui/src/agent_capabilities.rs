//! Runtime model catalog. Loading metadata never sends a conversation message.
use super::{AgentClient, AgentSurface, Context};
use decodex_protocol::{AgentCapabilitiesResult, AgentModelDto, ConversationReasoningEffort};
use gpui::prelude::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct CatalogContext {
	directory: String,
	account: String,
	has_root: bool,
	runtime_source: Option<decodex_protocol::EntityId>,
}

impl AgentSurface {
	pub(super) fn service_tier_picker(&self, cx: &Context<Self>) -> gpui::AnyElement {
		let mut panel = gpui::div().id("service-tier-picker").flex().items_center().gap_1();
		let mut tiers = vec![decodex_protocol::AgentServiceTierDto {
			id: decodex_protocol::ServiceTier::standard(),
			name: "Standard".into(),
			description: String::new(),
		}];
		if let Some(model) = self.selected_model(cx) {
			tiers.extend(
				model.service_tiers.iter().filter(|tier| tier.id.as_str() != "default").cloned(),
			);
		}
		let selected = if let Some(owner) = self.composer_manager.clone().or_else(|| self.root_id())
		{
			self.draft_profiles.execution.choice(&owner).selected_service_tier()
		} else {
			Some(
				self.service_tier
					.clone()
					.unwrap_or_else(|| decodex_protocol::ServiceTier::from_fast(self.fast)),
			)
		};
		if selected.is_none() {
			panel = panel.child(
				gpui::div().debug_selector(|| "tier-inherited".into()).child(super::muted("Auto")),
			);
		}
		if selected.as_ref().is_some_and(|selected| {
			selected.as_str() == "flex" && !tiers.iter().any(|tier| &tier.id == selected)
		}) {
			panel = panel.child("Flex · configured");
		}
		for tier in tiers {
			let id = tier.id.clone();
			let chosen = selected.as_ref() == Some(&id);
			panel = panel.child(
				gpui::div()
					.id(gpui::SharedString::from(format!("tier-{}", id.as_str())))
					.debug_selector({
						let label = format!("tier-{}", id.as_str());
						move || label.clone()
					})
					.cursor_pointer()
					.px_2()
					.py_1()
					.child(format!("{}{}", if chosen { "✓ " } else { "" }, tier.name))
					.text_size(gpui::px(11.))
					.rounded(gpui::px(8.))
					.bg(gpui::rgba(if chosen { 0xffffff16 } else { 0x00000000 }))
					.hover(|s| s.bg(gpui::rgba(0xffffff20)))
					.on_click(cx.listener(move |s, _, _, cx| {
						if id.as_str() == "default"
							|| s.selected_model(cx).is_some_and(|model| {
								model.service_tiers.iter().any(|tier| tier.id == id)
							}) {
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

	pub(super) fn model_notice_panel(&self, cx: &Context<Self>) -> gpui::AnyElement {
		let mut panel = gpui::div()
			.id("model-catalog-notices")
			.debug_selector(|| "model-catalog-notices".into())
			.flex()
			.flex_col()
			.gap_2();
		if let Some(model) = self.selected_model(cx) {
			if let Some(programs) = &model.available_cyber_programs {
				let names = if programs.is_empty() {
					"None advertised".into()
				} else {
					programs.join(", ")
				};
				panel = panel.child(
					gpui::div()
						.id("model-access-programs")
						.debug_selector(|| "model-access-programs".into())
						.child(super::muted(format!("Catalog access programs: {names}"))),
				);
			}
			if let Some(notice) = &model.availability {
				panel = panel.child(super::muted(notice.clone()));
			}
			if let Some(upgrade) = &model.upgrade {
				panel = panel
					.child(super::muted(format!("Suggested upgrade: {}", upgrade.model.as_str())));
				if let Some(timestamp) = upgrade.retirement_at
					&& let Ok(date) = time::OffsetDateTime::from_unix_timestamp(timestamp)
				{
					panel = panel.child(super::muted(format!(
						"Scheduled retirement: {} (UTC)",
						date.date()
					)));
				}
				if let Some(notice) = &upgrade.notice {
					panel = panel.child(super::muted(notice.clone()));
				}
			}
		} else if matches!(
			self.current_model_catalog(cx),
			Some(AgentCapabilitiesResult::Available { .. })
		) && let Some(model) =
			self.composer_model_value(cx).filter(|model| !model.is_empty())
		{
			panel = panel.child(gpui::div().debug_selector(|| "model-not-listed".into()).child(
				super::muted(format!(
					"{model} is not listed in the current model catalog. You can choose a listed model below."
				)),
			));
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
				decodex_protocol::ConversationWorkingDirectory::new(context.directory.clone())
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
			Some(decodex_protocol::InitialModelCatalogRequest {
				working_directory,
				account_id,
				purpose: decodex_protocol::ModelCatalogPurpose::Agent,
			})
		};
		self.capabilities_checked = Some(std::time::Instant::now());
		self.capability_generation += 1;
		let generation = self.capability_generation;
		let request = cx.background_executor().spawn(async move {
			let runtime =
				tokio::runtime::Builder::new_current_thread().enable_all().build().ok()?;
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
				self.service_tier = Some(decodex_protocol::ServiceTier::standard());
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
			|| {
				Some(
					self.service_tier
						.clone()
						.unwrap_or_else(|| decodex_protocol::ServiceTier::from_fast(self.fast)),
				)
			},
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
	use super::*;
	#[gpui::test]
	fn catalog_reply_survives_unrelated_snapshot_refresh(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		surface.update(visual, |surface, cx| {
			surface.visual_workspace_fixture(cx);
			let context = surface.catalog_context(cx).unwrap();
			surface.capability_generation = 7;
			surface.capability_task = Some(cx.spawn(async |_, _| std::future::pending().await));
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
			surface.capability_task = Some(cx.spawn(async |_, _| std::future::pending().await));
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
			s.capability_task = Some(cx.spawn(async |_, _| std::future::pending().await));
			s.mark_stale(cx);
			assert!(s.capability_task.is_none());
			s.capability_task = Some(cx.spawn(async |_, _| std::future::pending().await));
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
	fn advertised_tier_selection_is_explicit_and_invalidated_by_catalog_changes(
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
					supports_fast: false,
					available_cyber_programs: None,
					supports_images: true,
					availability: None,
					upgrade: None,
					service_tiers: vec![decodex_protocol::AgentServiceTierDto {
						id: decodex_protocol::ServiceTier::new("ultrafast").unwrap(),
						name: "Ultrafast".into(),
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
			window.resize(gpui::size(gpui::px(1280.), gpui::px(1400.)));
			window.draw(cx).clear();
		});
		// The popover uses a real-time entrance translation; click its settled bounds.
		std::thread::sleep(std::time::Duration::from_millis(220));
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		assert!(visual.debug_bounds("tier-inherited").is_some());
		let bounds = visual.debug_bounds("tier-ultrafast").expect("advertised tier visible");
		visual.simulate_click(bounds.center(), gpui::Modifiers::default());
		surface.update(visual, |s, cx| {
			assert_eq!(s.service_tier.as_ref().unwrap().as_str(), "ultrafast");
			assert!(s.composer_capability_error(cx).is_none());
			if let Some(AgentCapabilitiesResult::Available { models, .. }) = &mut s.capabilities {
				models[0].service_tiers.clear();
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
	fn missing_model_notice_requires_a_current_catalog(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.expanded_records.insert("model-notices".into());
			s.model.update(cx, |input, cx| input.set_content("saved-model", cx));
			s.mark_model_intent(cx);
			s.composer_menu = Some("model");
			s.composer_menu_content = Some("model");
			s.capabilities_context = s.catalog_context(cx);
		});
		for (catalog, expected) in [
			(None, false),
			(Some(AgentCapabilitiesResult::Unavailable), false),
			(
				Some(AgentCapabilitiesResult::Available { models: vec![], memory_enabled: None }),
				true,
			),
		] {
			surface.update(visual, |s, cx| {
				s.capabilities = catalog;
				s.reconcile_model_options(cx);
				assert_eq!(s.composer_model_value(cx).as_deref(), Some("saved-model"));
				cx.notify();
			});
			visual.update(|window, cx| {
				window.resize(gpui::size(gpui::px(1280.0), gpui::px(1200.0)));
				window.draw(cx).clear();
			});
			assert_eq!(visual.debug_bounds("model-not-listed").is_some(), expected);
		}
		surface.update(visual, |s, cx| {
			s.account.update(cx, |input, cx| {
				input.set_content("00000000-0000-4000-8000-000000000099", cx)
			});
			cx.notify();
		});
		visual.update(|window, cx| window.draw(cx).clear());
		assert!(visual.debug_bounds("model-not-listed").is_none());
		surface.update(visual, |s, cx| {
			assert_eq!(s.composer_model_value(cx).as_deref(), Some("saved-model"));
			s.draft_profiles.execution = Default::default();
			s.capabilities_context = s.catalog_context(cx);
			assert_eq!(s.composer_model_value(cx), None);
			assert_eq!(s.composer_model_label(cx), "Task model");
			cx.notify();
		});
		visual.update(|window, cx| window.draw(cx).clear());
		assert!(visual.debug_bounds("model-not-listed").is_none());
	}

	#[gpui::test]
	fn model_notices_refresh_without_replacing_selected_model(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.capabilities = Some(AgentCapabilitiesResult::Available {
				memory_enabled: None,
				models: vec![AgentModelDto {
					model: decodex_protocol::ConversationModel::new("current-model").unwrap(),
					name: "Current model".into(),
					efforts: vec![ConversationReasoningEffort::High],
					default_effort: Some(ConversationReasoningEffort::High),
					supports_fast: false,
					service_tiers: vec![],
					default_service_tier: None,
					available_cyber_programs: None,
					supports_images: true,
					availability: Some("Available for this account".into()),
					upgrade: Some(decodex_protocol::AgentModelUpgradeDto {
						model: decodex_protocol::ConversationModel::new("replacement").unwrap(),
						notice: Some("A replacement is available".into()),
						retirement_at: Some(1800000000),
					}),
				}],
			});
			s.model.update(cx, |input, cx| input.set_content("current-model", cx));
			s.mark_model_intent(cx);
			s.reconcile_model_options(cx);
			s.composer_menu = Some("model");
			s.composer_menu_content = Some("model");
			assert_eq!(s.model.read(cx).content(), "current-model");
		});
		visual.update(|window, cx| {
			window.resize(gpui::size(gpui::px(1280.0), gpui::px(1200.0)));
			window.draw(cx).clear();
		});
		assert!(visual.debug_bounds("model-catalog-notices").is_none());
		std::thread::sleep(std::time::Duration::from_millis(220));
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		let info = visual.debug_bounds("model-info-toggle").unwrap();
		visual.simulate_click(info.center(), Default::default());
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		let bounds = visual.debug_bounds("model-catalog-notices").expect("visible model notices");
		assert!(bounds.size.height > gpui::px(20.0));
		surface.update(visual, |s, cx| assert_eq!(s.model.read(cx).content(), "current-model"));
		for programs in [Some(vec!["standard".into(), "daybreakBlue".into()]), Some(vec![]), None] {
			let visible = programs.is_some();
			surface.update(visual, |s, cx| {
				let Some(AgentCapabilitiesResult::Available { models, .. }) = &mut s.capabilities
				else {
					panic!("catalog fixture")
				};
				models[0].available_cyber_programs = programs;
				assert_eq!(s.model.read(cx).content(), "current-model");
				cx.notify();
			});
			visual.update(|window, cx| {
				window.draw(cx).clear();
			});
			assert_eq!(visual.debug_bounds("model-access-programs").is_some(), visible);
		}
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
