//! Runtime model catalog. Loading metadata never sends a conversation message.
use super::{ChiefClient, ChiefSurface, Context};
use decodex_protocol::{ChiefCapabilitiesResult, ChiefModelDto, ConversationReasoningEffort};
use gpui::prelude::*;

impl ChiefSurface {
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
		let request = cx.background_executor().spawn(async move {
			let runtime =
				tokio::runtime::Builder::new_current_thread().enable_all().build().ok()?;
			runtime.block_on(ChiefClient::new(profile).capabilities()).ok()
		});
		self.capability_task = Some(cx.spawn(async move |surface, cx| {
			let result = request.await;
			let _ = surface.update(cx, |surface, cx| {
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
		}
	}

	pub(super) fn composer_capability_error(&self, cx: &Context<Self>) -> Option<&'static str> {
		let model = self.selected_model(cx)?;
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
