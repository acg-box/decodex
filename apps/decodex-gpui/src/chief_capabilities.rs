//! Runtime model catalog. Loading metadata never sends a conversation message.
use super::{ChiefClient, ChiefSurface, Context};
use decodex_protocol::{ChiefCapabilitiesResult, ChiefModelDto, ConversationReasoningEffort};

impl ChiefSurface {
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
