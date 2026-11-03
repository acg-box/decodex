//! Speed fallback retains native model ownership and persistent user intent.
use super::{super::speed::Selection, *};
use decodex_protocol::{
	AgentActionDto, AgentCapabilitiesResult, AgentExecutionOverrides, AgentServiceTierDto,
	ConversationModel, ConversationReasoningEffort, EntityId, HistoryText, ServiceTier,
};
use gpui::AppContext as _;

fn model(tiers: &[&str]) -> AgentModelDto {
	AgentModelDto {
		model: ConversationModel::new("speed-model").unwrap(),
		name: "Speed model".into(),
		efforts: vec![ConversationReasoningEffort::High],
		default_effort: Some(ConversationReasoningEffort::High),
		supports_fast: tiers.contains(&"priority"),
		available_cyber_programs: None,
		specialty: None,
		supports_images: true,
		availability: None,
		upgrade: None,
		service_tiers: tiers
			.iter()
			.map(|id| AgentServiceTierDto {
				id: ServiceTier::new(*id).unwrap(),
				name: id.to_string(),
				description: String::new(),
			})
			.collect(),
		default_service_tier: None,
	}
}

fn setup(s: &mut AgentSurface, cx: &mut Context<AgentSurface>) {
	s.visual_workspace_fixture(cx);
	s.model.update(cx, |input, cx| input.set_content("speed-model", cx));
	s.mark_model_intent(cx);
	s.service_tier = Some(ServiceTier::new("ultrafast").unwrap());
	s.mark_tier_intent();
	s.model_settings.observe(
		"agent".into(),
		AgentModelSettingsResult::Available {
			work_id: EntityId::new("agent").unwrap(),
			thread_id: EntityId::new("native-agent").unwrap(),
			account_id: EntityId::new("account").unwrap(),
			model_provider: Some(WireText::new("openai").unwrap()),
			model: Some(WireText::new("speed-model").unwrap()),
			reasoning_effort: Some(WireText::new("high").unwrap()),
			service_tier: Some(WireText::new("priority").unwrap()),
		},
	);
	s.capabilities = Some(AgentCapabilitiesResult::Available {
		models: vec![model(&["priority"])],
		memory_enabled: None,
	});
}

fn followup(s: &AgentSurface) -> AgentActionDto {
	s.configured_send(
		EntityId::new("agent").unwrap(),
		HistoryText::new("continue").unwrap(),
		vec![],
	)
}

#[test]
fn speed_resolution_does_not_invent_availability_or_remove_explicit_standard_and_flex() {
	for (requested, tiers, expected) in [
		("ultrafast", vec!["priority", "ultrafast"], "ultrafast"),
		("ultrafast", vec!["priority"], "priority"),
		("ultrafast", vec![], "default"),
		("priority", vec![], "default"),
		("default", vec!["ultrafast"], "default"),
		("flex", vec![], "flex"),
	] {
		let resolved =
			Selection::resolve(ServiceTier::new(requested).unwrap(), Some(&model(&tiers)));
		assert_eq!(resolved.effective.as_str(), expected);
	}
	let unresolved = Selection::resolve(ServiceTier::new("ultrafast").unwrap(), None);
	assert!(!unresolved.is_fallback(), "an unavailable catalog is not proof of unavailable speed");
}

#[gpui::test]
fn fallback_sends_fast_and_retains_only_speed_across_acceptance_and_cold_restore(
	cx: &mut gpui::TestAppContext,
) {
	let surface = cx.new(AgentSurface::new);
	surface.update(cx, |s, cx| {
		setup(s, cx);
		let mut action = followup(s);
		let (captured, retained) = s.prepare_submission_speed(&mut action, cx);
		assert!(captured.is_some());
		let AgentActionDto::SendConfigured { execution, .. } = action else { unreachable!() };
		let mut native = serde_json::json!({});
		execution.apply_to_native_turn(&mut native);
		assert_eq!(native["serviceTier"], "priority");
		assert_eq!(native["serviceTierForTurn"], "priority");
		assert_eq!(native["model"], "speed-model");
		s.draft_profiles.execution.accepted(captured.as_ref(), retained);
		let saved = s.draft_profiles.execution.saved_choices();
		let wire = serde_json::to_vec(&saved).unwrap();
		let (revision, choices) = serde_json::from_slice(&wire).unwrap();
		s.draft_profiles.execution =
			super::super::execution_intent::Intents::from_saved(revision, choices);
		let remaining = s.draft_profiles.execution.choice("agent");
		assert_eq!(remaining.selected_service_tier().unwrap().as_str(), "ultrafast");
		assert!(
			remaining.model.is_none()
				&& remaining.reasoning_effort.is_none()
				&& remaining.fast.is_none(),
			"do not replay unrelated overrides on later turns"
		);
		assert_eq!(s.composer_speed_label(cx), "Fast");
		let mut next = followup(s);
		s.prepare_submission_speed(&mut next, cx);
		let AgentActionDto::SendConfigured { execution, .. } = next else { unreachable!() };
		assert_eq!(execution.selected_service_tier().unwrap().as_str(), "priority");
		assert!(execution.model.is_none() && execution.reasoning_effort.is_none());
	});
}

#[gpui::test]
fn fallback_receipt_cannot_overwrite_new_choices_and_missing_catalog_does_not_invent_a_fallback(
	cx: &mut gpui::TestAppContext,
) {
	let surface = cx.new(AgentSurface::new);
	surface.update(cx, |s, cx| {
		setup(s, cx);
		let mut action = followup(s);
		let (captured, retained) = s.prepare_submission_speed(&mut action, cx);
		s.service_tier = Some(ServiceTier::standard());
		s.mark_tier_intent();
		s.draft_profiles.execution.accepted(captured.as_ref(), retained);
		assert_eq!(s.composer_speed_label(cx), "Standard");

		s.service_tier = Some(ServiceTier::new("ultrafast").unwrap());
		s.mark_tier_intent();
		assert!(s.composer_speed(cx).unwrap().is_fallback());
		if let Some(AgentCapabilitiesResult::Available { models, .. }) = &mut s.capabilities {
			models[0] = model(&["priority", "ultrafast"]);
		}
		assert!(!s.composer_speed(cx).unwrap().is_fallback());
		assert_eq!(s.composer_speed_label(cx), "Ultrafast");

		s.reset_capabilities();
		assert!(!s.composer_speed(cx).unwrap().is_fallback());
		assert!(
			s.composer_capability_error(cx).is_some(),
			"no unverified tier is sent before the current account catalog arrives"
		);
	});
}

#[gpui::test]
fn new_and_inherited_messages_use_fallback_without_changing_another_agent(
	cx: &mut gpui::TestAppContext,
) {
	let surface = cx.new(AgentSurface::new);
	surface.update(cx, |s, cx| {
		setup(s, cx);
		let mut action = AgentActionDto::NewConversation {
			project_id: None,
			work_id: EntityId::new("agent").unwrap(),
			text: HistoryText::new("hello").unwrap(),
			execution: s.draft_profiles.execution.choice("agent"),
			permission_mode: None,
			attachments: vec![],
			task_references: vec![],
		};
		let (captured, retained) = s.prepare_submission_speed(&mut action, cx);
		let AgentActionDto::NewConversation { execution, .. } = action else { unreachable!() };
		assert_eq!(execution.selected_service_tier().unwrap().as_str(), "priority");
		s.draft_profiles.execution.accepted(captured.as_ref(), retained);
		s.draft_profiles.execution = Default::default();
		if let Some(AgentModelSettingsResult::Available { service_tier, .. }) =
			s.model_settings.observations.get_mut("agent")
		{
			*service_tier = Some(WireText::new("ultrafast").unwrap());
		}
		let mut inherited = followup(s);
		let (captured, retained) = s.prepare_submission_speed(&mut inherited, cx);
		assert!(captured.is_some());
		assert_eq!(retained.unwrap().as_str(), "ultrafast");
		let AgentActionDto::SendConfigured { execution, .. } = inherited else { unreachable!() };
		assert_eq!(execution.selected_service_tier().unwrap().as_str(), "priority");
		assert!(execution.model.is_none(), "an inherited speed must not pin an observed model");
		let mut other = AgentActionDto::SendConfigured {
			root_id: EntityId::new("another-agent").unwrap(),
			text: HistoryText::new("hello").unwrap(),
			execution: AgentExecutionOverrides {
				service_tier: Some(ServiceTier::new("ultrafast").unwrap()),
				..Default::default()
			},
			attachments: vec![],
			task_references: vec![],
		};
		assert!(
			s.prepare_submission_speed(&mut other, cx).1.is_none(),
			"the current model catalog cannot resolve another Agent's speed"
		);
	});
}
