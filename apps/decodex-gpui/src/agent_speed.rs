//! Keep the selected speed separate from the tier that the native catalog can use.
use decodex_protocol::{AgentActionDto, AgentModelDto, ServiceTier};

use crate::shell::agent_surface::{AgentSurface, Context};

pub(super) struct Selection {
	pub requested: ServiceTier,
	pub effective: ServiceTier,
}

impl Selection {
	pub(super) fn resolve(requested: ServiceTier, model: Option<&AgentModelDto>) -> Self {
		let effective = match model {
			Some(model)
				if !matches!(requested.as_str(), "default" | "flex")
					&& !model.service_tiers.iter().any(|tier| tier.id == requested) =>
			{
				if requested.as_str() == "ultrafast"
					&& model.service_tiers.iter().any(|tier| tier.id.as_str() == "priority")
				{
					ServiceTier::from_fast(true)
				} else {
					ServiceTier::standard()
				}
			},
			_ => requested.clone(),
		};
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

	/// Capture the user's choice before substituting the usable native tier. Keep only
	/// the speed preference after acceptance so a fallback remains visible on later turns.
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
				choice.fast = None;
			});
		}
		execution.service_tier = Some(selection.requested.clone());
		execution.fast = None;
		let captured = self.draft_profiles.execution.capture(action);
		let execution = match action {
			AgentActionDto::SendConfigured { execution, .. }
			| AgentActionDto::NewConversation { execution, .. }
			| AgentActionDto::StartConfigured { execution, .. } => execution,
			_ => unreachable!(),
		};
		execution.service_tier = Some(selection.effective);
		(captured, Some(selection.requested))
	}
}
