//! Keep the selected speed separate from the tier that the native catalog can use.
use decodex_protocol::{AgentActionDto, AgentModelDto, ServiceTier};

use crate::shell::agent_surface::{AgentSurface, Context};

pub(super) struct Selection {
	pub requested: ServiceTier,
	pub effective: ServiceTier,
}

impl Selection {
	pub(super) fn resolve(requested: ServiceTier, model: Option<&AgentModelDto>) -> Self {
		let effective = model
			.map_or_else(|| requested.clone(), |model| model.effective_service_tier(&requested));
		Self { requested, effective }
	}

	pub fn is_fallback(&self) -> bool {
		self.requested != self.effective
	}

	pub fn notice(&self) -> String {
		format!(
			"{} unavailable. Using {}.",
			crate::session_settings::speed_label(self.requested.as_str()),
			crate::session_settings::speed_label(self.effective.as_str()),
		)
	}
}

impl AgentSurface {
	pub(super) fn composer_speed(&self, cx: &Context<Self>) -> Option<Selection> {
		Some(Selection::resolve(self.composer_service_tier()?, self.selected_model(cx)))
	}

	/// Keep the preference through acceptance. The admitted runtime resolves the per-turn
	/// tier after account selection, without replacing the native conversation preference.
	pub(super) fn prepare_submission_speed(
		&mut self,
		action: &mut AgentActionDto,
		cx: &Context<Self>,
	) -> (Option<(String, u64)>, Option<ServiceTier>) {
		let (owner, execution) = match action {
			AgentActionDto::SendConfigured { root_id, execution, .. } => (root_id, execution),
			AgentActionDto::NewConversation { work_id, execution, .. } => (work_id, execution),
			AgentActionDto::StartConfigured { start, execution, .. } =>
				(&mut start.root_id, execution),
			_ => return (self.draft_profiles.execution.capture(action), None),
		};
		let current = self
			.composer_manager
			.clone()
			.or_else(|| self.root_id())
			.or_else(|| self.is_new_conversation().then(|| self.selected.clone()).flatten());
		if current.as_deref() != Some(owner.as_str()) {
			return (self.draft_profiles.execution.capture(action), None);
		}
		let requested = execution.selected_service_tier().or_else(|| self.composer_service_tier());
		let Some(selection) = requested
			.map(|tier| Selection::resolve(tier, self.selected_model(cx)))
			.filter(Selection::is_fallback)
		else {
			return (self.draft_profiles.execution.capture(action), None);
		};
		let owner = owner.as_str().to_owned();
		if self.draft_profiles.execution.choice(&owner).selected_service_tier()
			!= Some(selection.requested.clone())
		{
			self.draft_profiles.execution.change(owner, |choice| {
				choice.service_tier = Some(selection.requested.clone());
			});
		}
		execution.service_tier = Some(selection.requested.clone());
		let captured = self.draft_profiles.execution.capture(action);
		(captured, Some(selection.requested))
	}
}
