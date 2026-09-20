//! Runtime model catalog. Loading metadata never sends a conversation message.
use super::{ChiefClient, ChiefSurface, Context};
use decodex_protocol::{ChiefCapabilitiesResult, ChiefModelDto, ConversationReasoningEffort};
use gpui::prelude::*;

impl ChiefSurface {
	pub(super) fn service_tier_picker(&self, cx: &Context<Self>) -> gpui::AnyElement {
		let mut panel = gpui::div().id("service-tier-picker").flex().flex_col().gap_2();
		let mut tiers = vec![decodex_protocol::ChiefServiceTierDto {
			id: decodex_protocol::ServiceTier::standard(),
			name: "Standard".into(),
			description: String::new(),
		}];
		if let Some(model) = self.selected_model(cx) {
			tiers.extend(
				model.service_tiers.iter().filter(|tier| tier.id.as_str() != "default").cloned(),
			);
		}
		let selected = self
			.service_tier
			.clone()
			.unwrap_or_else(|| decodex_protocol::ServiceTier::from_fast(self.fast));
		for tier in tiers {
			let id = tier.id.clone();
			let chosen = selected == id;
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
					.child(super::muted(tier.description))
					.on_click(cx.listener(move |s, _, _, cx| {
						if id.as_str() == "default"
							|| s.selected_model(cx).is_some_and(|model| {
								model.service_tiers.iter().any(|tier| tier.id == id)
							}) {
							s.fast = id.as_str() == "priority";
							s.service_tier = Some(id.clone());
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
		}
		panel.into_any_element()
	}

	pub(super) fn load_capabilities(&mut self, cx: &mut Context<Self>) {
		if self.capability_task.is_some() {
			return;
		}
		let Some(profile) = self.profile.clone() else {
			return;
		};
		self.capabilities_checked = Some(std::time::Instant::now());
		let generation = self.generation;
		let request = cx.background_executor().spawn(async move {
			let runtime =
				tokio::runtime::Builder::new_current_thread().enable_all().build().ok()?;
			runtime.block_on(ChiefClient::new(profile).capabilities()).ok()
		});
		self.capability_task = Some(cx.spawn(async move |surface, cx| {
			let result = request.await;
			let _ = surface.update(cx, |surface, cx| {
				if surface.generation != generation {
					return;
				}
				surface.capability_task = None;
				surface.capabilities = result;
				surface.reconcile_model_options(cx);
				cx.notify();
			});
		}));
	}

	pub(super) fn selected_model(&self, cx: &Context<Self>) -> Option<&ChiefModelDto> {
		let Some(ChiefCapabilitiesResult::Available { models, .. }) = &self.capabilities else {
			return None;
		};
		models.iter().find(|model| model.model.as_str() == self.model.read(cx).content())
	}

	pub(super) fn model_efforts(&self, cx: &Context<Self>) -> Vec<ConversationReasoningEffort> {
		self.selected_model(cx).map_or_else(|| vec![self.effort], |model| model.efforts.clone())
	}

	pub(super) fn reconcile_model_options(&mut self, cx: &mut Context<Self>) {
		if let Some(model) = self.selected_model(cx).cloned() {
			if !model.efforts.contains(&self.effort)
				&& let Some(effort) =
					model.default_effort.or_else(|| model.efforts.first().copied())
			{
				self.effort = effort;
			}
			if !model.supports_fast {
				self.fast = false;
			}
			if self.service_tier.as_ref().is_some_and(|selected| {
				selected.as_str() != "default"
					&& !model.service_tiers.iter().any(|tier| &tier.id == selected)
			}) {
				self.service_tier = Some(decodex_protocol::ServiceTier::standard());
				self.fast = false;
			}
		}
	}

	pub(super) fn composer_capability_error(&self, cx: &Context<Self>) -> Option<&'static str> {
		let Some(model) = self.selected_model(cx) else {
			return (self.fast
				|| self.service_tier.as_ref().is_some_and(|tier| tier.as_str() != "default"))
			.then_some("Refresh model capabilities before selecting a service tier.");
		};
		if self.service_tier.as_ref().is_some_and(|selected| {
			selected.as_str() != "default"
				&& !model.service_tiers.iter().any(|tier| &tier.id == selected)
		}) {
			return Some("This service tier is unavailable for the selected model.");
		}
		if !model.efforts.contains(&self.effort) {
			return Some("This model's reasoning levels are not supported by this version.");
		}
		if self.fast && !model.supports_fast {
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
mod tests {
	use super::*;
	#[gpui::test]
	fn advertised_tier_selection_is_explicit_and_invalidated_by_catalog_changes(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.model.update(cx, |input, cx| input.set_content("custom", cx));
			s.capabilities = Some(ChiefCapabilitiesResult::Available {
				memory_enabled: None,
				models: vec![ChiefModelDto {
					model: decodex_protocol::ConversationModel::new("custom").unwrap(),
					name: "Custom".into(),
					efforts: vec![ConversationReasoningEffort::High],
					default_effort: Some(ConversationReasoningEffort::High),
					supports_fast: false,
					supports_images: true,
					availability: None,
					upgrade: None,
					service_tiers: vec![decodex_protocol::ChiefServiceTierDto {
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
		let bounds = visual.debug_bounds("tier-ultrafast").expect("advertised tier visible");
		visual.simulate_click(bounds.center(), gpui::Modifiers::default());
		surface.update(visual, |s, cx| {
			assert_eq!(s.service_tier.as_ref().unwrap().as_str(), "ultrafast");
			assert!(s.composer_capability_error(cx).is_none());
			if let Some(ChiefCapabilitiesResult::Available { models, .. }) = &mut s.capabilities {
				models[0].service_tiers.clear();
			}
			s.reconcile_model_options(cx);
			assert_eq!(s.service_tier.as_ref().unwrap().as_str(), "default");
		});
	}

	#[gpui::test]
	fn upgrade_notice_is_visible_without_replacing_selected_model(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.capabilities = Some(ChiefCapabilitiesResult::Available {
				memory_enabled: None,
				models: vec![ChiefModelDto {
					model: decodex_protocol::ConversationModel::new("current-model").unwrap(),
					name: "Current model".into(),
					efforts: vec![ConversationReasoningEffort::High],
					default_effort: Some(ConversationReasoningEffort::High),
					supports_fast: false,
					service_tiers: vec![],
					default_service_tier: None,
					supports_images: true,
					availability: Some("Available for this account".into()),
					upgrade: Some(decodex_protocol::ChiefModelUpgradeDto {
						model: decodex_protocol::ConversationModel::new("replacement").unwrap(),
						notice: Some("A replacement is available".into()),
						retirement_at: Some(1800000000),
					}),
				}],
			});
			s.model.update(cx, |input, cx| input.set_content("current-model", cx));
			s.reconcile_model_options(cx);
			s.composer_menu = Some("model");
			s.composer_menu_content = Some("model");
			assert_eq!(s.model.read(cx).content(), "current-model");
		});
		visual.update(|window, cx| {
			window.resize(gpui::size(gpui::px(1280.0), gpui::px(1200.0)));
			window.draw(cx).clear();
		});
		let bounds = visual.debug_bounds("model-catalog-notices").expect("visible model notices");
		assert!(bounds.size.height > gpui::px(20.0));
		surface.update(visual, |s, cx| assert_eq!(s.model.read(cx).content(), "current-model"));
	}
}
