//! Task-scoped next-message choices, independent of observed native settings.
use super::*;
use decodex_protocol::ChiefExecutionOverrides;
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct Intents {
	revision: u64,
	choices: BTreeMap<String, (u64, ChiefExecutionOverrides)>,
}
impl Intents {
	pub(super) fn saved_choices(&self) -> (u64, BTreeMap<String, (u64, ChiefExecutionOverrides)>) {
		(self.revision, self.choices.clone())
	}

	pub(super) fn from_saved(
		revision: u64,
		choices: BTreeMap<String, (u64, ChiefExecutionOverrides)>,
	) -> Self {
		Self { revision, choices }
	}

	fn change(&mut self, owner: String, update: impl FnOnce(&mut ChiefExecutionOverrides)) {
		self.revision = self.revision.wrapping_add(1);
		let value = self.choices.entry(owner).or_default();
		value.0 = self.revision;
		update(&mut value.1);
	}

	pub(super) fn choice(&self, owner: &str) -> ChiefExecutionOverrides {
		self.choices.get(owner).map(|v| v.1.clone()).unwrap_or_default()
	}

	pub(super) fn capture(&self, action: &ChiefActionDto) -> Option<(String, u64)> {
		let ChiefActionDto::SendConfigured { root_id, execution, .. } = action else { return None };
		let (revision, choice) = self.choices.get(root_id.as_str())?;
		(choice == execution && !choice.is_empty()).then(|| (root_id.as_str().into(), *revision))
	}

	pub(super) fn accepted(&mut self, captured: Option<&(String, u64)>) {
		if let Some((owner, revision)) = captured
			&& self.choices.get(owner).is_some_and(|v| v.0 == *revision)
		{
			self.choices.remove(owner);
			self.revision = self.revision.wrapping_add(1);
		}
	}
}

impl ChiefSurface {
	pub(super) fn mark_model_intent(&mut self, cx: &Context<Self>) {
		let Some(owner) = self.composer_manager.clone().or_else(|| self.root_id()) else { return };
		let Ok(model) = ConversationModel::new(self.model.read(cx).content()) else { return };
		let effort = self.effort.clone();
		let tier = self
			.service_tier
			.clone()
			.unwrap_or_else(|| decodex_protocol::ServiceTier::from_fast(self.fast));
		self.draft_profiles.execution.change(owner, |choice| {
			choice.model = Some(model);
			choice.reasoning_effort = Some(effort);
			if choice.service_tier.is_some() || choice.fast.is_some() {
				choice.service_tier = Some(tier);
				choice.fast = None;
			}
		});
	}

	pub(super) fn mark_effort_intent(&mut self) {
		let Some(owner) = self.composer_manager.clone().or_else(|| self.root_id()) else { return };
		let effort = self.effort.clone();
		self.draft_profiles
			.execution
			.change(owner, |choice| choice.reasoning_effort = Some(effort));
	}

	pub(super) fn mark_tier_intent(&mut self) {
		let Some(owner) = self.composer_manager.clone().or_else(|| self.root_id()) else { return };
		let tier = self
			.service_tier
			.clone()
			.unwrap_or_else(|| decodex_protocol::ServiceTier::from_fast(self.fast));
		self.draft_profiles.execution.change(owner, |choice| {
			choice.service_tier = Some(tier);
			choice.fast = None;
		});
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	fn action(surface: &ChiefSurface, owner: &str) -> ChiefActionDto {
		surface.configured_send(
			EntityId::new(owner).unwrap(),
			HistoryText::new("continue").unwrap(),
			vec![],
		)
	}
	fn choice(action: ChiefActionDto) -> ChiefExecutionOverrides {
		let ChiefActionDto::SendConfigured { execution, .. } = action else {
			panic!("expected queued message")
		};
		execution
	}

	#[gpui::test]
	fn inherited_messages_and_effort_only_changes_do_not_reapply_display_defaults(
		cx: &mut gpui::TestAppContext,
	) {
		let surface = cx.new(ChiefSurface::new);
		surface.update(cx, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.steer = false;
			s.model.update(cx, |input, cx| input.set_content("stale-display-model", cx));
			s.effort = ConversationReasoningEffort::Ultra;
			s.fast = true;
			assert!(choice(action(s, "chief")).is_empty());
			s.mark_effort_intent();
			let selected = choice(action(s, "chief"));
			assert_eq!(selected.reasoning_effort, Some(ConversationReasoningEffort::Ultra));
			assert!(selected.model.is_none() && selected.selected_service_tier().is_none());
			assert!(choice(action(s, "other-manager")).is_empty());

			s.select_composer_option("effort", "provider-defined-effort", cx);
			assert_eq!(
				choice(action(s, "chief")).reasoning_effort.unwrap().as_str(),
				"provider-defined-effort"
			);
			let encoded = serde_json::to_value(action(s, "chief")).unwrap();
			let decoded = serde_json::from_value(encoded).unwrap();
			assert_eq!(
				choice(decoded).reasoning_effort.unwrap().as_str(),
				"provider-defined-effort"
			);
			s.select_composer_option("model", "explicit-model", cx);
			assert_eq!(choice(action(s, "chief")).model.unwrap().as_str(), "explicit-model");
			assert!(choice(action(s, "chief")).selected_service_tier().is_none());
		});
	}

	#[gpui::test]
	fn late_acceptance_clears_only_the_captured_choice_and_steering_keeps_it(
		cx: &mut gpui::TestAppContext,
	) {
		let surface = cx.new(ChiefSurface::new);
		surface.update(cx, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.steer = false;
			s.effort = ConversationReasoningEffort::High;
			s.mark_effort_intent();
			let older = s.draft_profiles.execution.capture(&action(s, "chief")).unwrap();
			s.effort = ConversationReasoningEffort::Low;
			s.mark_effort_intent();
			s.draft_profiles.execution.accepted(Some(&older));
			assert_eq!(
				choice(action(s, "chief")).reasoning_effort,
				Some(ConversationReasoningEffort::Low)
			);
			let current = s.draft_profiles.execution.capture(&action(s, "chief")).unwrap();
			s.draft_profiles.execution.accepted(Some(&current));
			assert!(choice(action(s, "chief")).is_empty());
			s.effort = ConversationReasoningEffort::Medium;
			s.mark_effort_intent();
			s.steer = true;
			let work = s
				.snapshot
				.as_mut()
				.unwrap()
				.work_items
				.iter_mut()
				.find(|w| w.id == "chief")
				.unwrap();
			work.dispatch_state = ChiefDispatchStateDto::Running;
			work.active_turn_id = Some("turn".into());
			let steering = action(s, "chief");
			assert!(matches!(steering, ChiefActionDto::Steer { .. }));
			assert!(s.draft_profiles.execution.capture(&steering).is_none());
			assert!(!s.draft_profiles.execution.choice("chief").is_empty());
		});
	}
}
