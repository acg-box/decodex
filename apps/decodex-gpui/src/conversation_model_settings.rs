//! Resolve settings for an editor without borrowing another conversation's choices.
use super::{
	ConversationQueryPurpose, ConversationRouteOutcome, ConversationSummary, Conversations,
	QueryPayload, QueryResultPayload, State,
};
use decodex_protocol::ConversationModelSettingsResult;

pub(super) struct Observation {
	source: Box<ConversationSummary>,
	tier_known: bool,
}

impl Conversations {
	pub(crate) fn service_tier_unknown(&self) -> bool {
		let state = self.lock();
		state.execution_source.as_ref().is_some_and(|observed| {
			state.selected_task() == Some(observed.source.as_ref())
				&& !observed.tier_known
				&& !state.creation_intent.service_tier
		})
	}

	pub(crate) fn ensure_model_settings(&self) {
		let mut state = self.lock();
		state.prepare_execution_choice();
		let Some(source) = state.selected_task().cloned() else { return };
		if state.ordinary_execution_ready()
			|| state.model_settings_requested.as_deref() == Some(&source)
		{
			return;
		}
		let epoch = state.catalog_epoch;
		let queued = state.queue_query(
			QueryPayload::GetConversationModelSettings {
				conversation_id: source.conversation_id.clone(),
			},
			ConversationQueryPurpose::ModelSettings { epoch, source: Box::new(source.clone()) },
		);
		if queued {
			state.model_settings_requested = Some(Box::new(source));
		}
		drop(state);
		if queued {
			self.inner.notify.notify_one();
		}
	}
}

impl State {
	pub(super) fn prepare_execution_choice(&mut self) {
		let owner = self.selected.clone().or_else(|| self.requested_selection.clone());
		if self.execution_choice_owner != owner {
			self.creation_intent = Default::default();
			self.execution_choice_owner = owner;
		}
	}

	pub(super) fn ordinary_execution_ready(&self) -> bool {
		if matches!(&self.catalog_source, Some(super::CatalogSource::Review { .. }))
			&& self.current_catalog().is_some()
		{
			return true;
		}
		let owner = self.selected.as_ref().or(self.requested_selection.as_ref());
		owner.is_some()
			&& (self.execution_source.as_ref().is_some_and(|observed| {
				Some(&observed.source.conversation_id) == owner
					&& self.selected_task() == Some(observed.source.as_ref())
			}) || (self.execution_choice_owner.as_ref() == owner
				&& self.creation_intent.model
				&& self.creation_intent.reasoning
				&& self.creation_intent.service_tier))
	}

	pub(super) fn route_model_settings(
		&mut self,
		epoch: u64,
		source: &ConversationSummary,
		payload: &QueryResultPayload,
	) -> (ConversationRouteOutcome, bool) {
		if self.catalog_epoch != epoch || self.selected_task() != Some(source) {
			return (ConversationRouteOutcome::Refused, false);
		}
		let QueryResultPayload::ConversationModelSettings(
			ConversationModelSettingsResult::Available {
				model: Some(model),
				reasoning_effort,
				requested_service_tier,
				..
			},
		) = payload
		else {
			return (ConversationRouteOutcome::Fresh, false);
		};
		if !self.creation_intent.model {
			self.execution.model = model.clone();
		}
		if !self.creation_intent.reasoning {
			self.execution.reasoning_effort = reasoning_effort.clone();
		}
		if !self.creation_intent.service_tier
			&& let Some(tier) = requested_service_tier
		{
			self.execution = self.execution.clone().with_service_tier(tier.clone());
		}
		self.execution_source = Some(Observation {
			source: Box::new(source.clone()),
			tier_known: requested_service_tier.is_some(),
		});
		(ConversationRouteOutcome::Fresh, false)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use decodex_protocol::{ConversationModel, EntityRevision, ServiceTier};

	fn observed(model: &str) -> QueryResultPayload {
		QueryResultPayload::ConversationModelSettings(ConversationModelSettingsResult::Available {
			model_provider: Some("native-provider".into()),
			model: Some(ConversationModel::new(model).unwrap()),
			reasoning_effort: None,
			requested_service_tier: Some(ServiceTier::new("flex").unwrap()),
		})
	}

	#[test]
	fn observed_defaults_refresh_but_explicit_choices_survive_restore_and_reconnect() {
		let (conversations, server, mut source) =
			crate::conversations::tests::connected_conversations();
		{
			let mut state = conversations.lock();
			state.creation_intent = Default::default();
			let epoch = state.catalog_epoch;
			assert_eq!(
				state.route_model_settings(epoch, &source, &observed("native-a")).0,
				ConversationRouteOutcome::Fresh
			);
			assert!(state.ordinary_execution_ready());
		}
		let draft = conversations.ordinary_draft("Retained input").unwrap();
		assert!(conversations.restore_ordinary_draft(&draft));
		assert!(
			!conversations.snapshot().model_settings_ready,
			"disk values are not a fresh native observation"
		);
		{
			let mut state = conversations.lock();
			let epoch = state.catalog_epoch;
			state.route_model_settings(epoch, &source, &observed("native-b"));
			assert_eq!(state.execution.model.as_str(), "native-b");
			source.runtime_session_revision = Some(EntityRevision(2));
			state.tasks[0] = source.clone();
			assert!(
				!state.ordinary_execution_ready(),
				"changed session invalidates default settings"
			);
			state.route_model_settings(epoch, &source, &observed("native-c"));
			assert_eq!(state.execution.model.as_str(), "native-c");
		}
		conversations.cycle_model();
		let selected = conversations.snapshot().execution.model;
		conversations.session_ended(1);
		conversations.bind_session(2, server);
		assert!(!conversations.snapshot().model_settings_ready);
		{
			let mut state = conversations.lock();
			let epoch = state.catalog_epoch;
			state.route_model_settings(epoch, &source, &observed("native-d"));
			assert_eq!(
				state.execution.model, selected,
				"native refresh must not overwrite the explicit model"
			);
			assert!(state.ordinary_execution_ready());
		}
		conversations.refresh_catalog();
		assert!(
			!conversations.snapshot().model_settings_ready,
			"manual refresh invalidates observed defaults"
		);
		conversations.cycle_reasoning_effort();
		assert!(conversations.select_service_tier(ServiceTier::standard()));
		assert!(
			conversations.snapshot().model_settings_ready,
			"all explicit owner choices need no defaults"
		);
		let explicit = conversations.ordinary_draft("Still retained").unwrap();
		assert!(conversations.restore_ordinary_draft(&explicit));
		assert!(conversations.snapshot().model_settings_ready);
		assert_eq!(conversations.snapshot().execution.model, selected);
	}
	#[test]
	fn model_choice_keeps_its_required_effort_adjustment_during_a_native_read() {
		let (conversations, _, source) = crate::conversations::tests::connected_conversations();
		{
			let mut state = conversations.lock();
			state.creation_intent = Default::default();
			state.execution.reasoning_effort = Some(
				decodex_protocol::ConversationReasoningEffort::new("unsupported-old-effort")
					.unwrap(),
			);
		}
		conversations.cycle_model();
		let selected = conversations.snapshot().execution.reasoning_effort;
		let mut state = conversations.lock();
		assert!(state.creation_intent.reasoning);
		let epoch = state.catalog_epoch;
		state.route_model_settings(epoch, &source, &observed("native-model"));
		assert_eq!(state.execution.reasoning_effort, selected);
	}

	#[test]
	fn cold_settings_never_reuse_another_editors_unknown_tier() {
		let (conversations, server_id, source) =
			crate::conversations::tests::connected_conversations();
		let mut reply = observed("cold-native-model");
		let QueryResultPayload::ConversationModelSettings(
			ConversationModelSettingsResult::Available { requested_service_tier, .. },
		) = &mut reply
		else {
			panic!("settings")
		};
		*requested_service_tier = None;
		{
			let mut state = conversations.lock();
			state.creation_intent = Default::default();
			state.execution =
				state.execution.clone().with_service_tier(ServiceTier::new("flex").unwrap());
			let epoch = state.catalog_epoch;
			state.route_model_settings(epoch, &source, &reply);
		}
		assert!(conversations.snapshot().can_submit);
		assert!(conversations.service_tier_unknown());
		assert_eq!(conversations.snapshot().execution.model.as_str(), "cold-native-model");
		assert!(conversations.select_service_tier(ServiceTier::standard()));
		assert!(!conversations.service_tier_unknown());
		assert!(conversations.snapshot().can_submit);
		assert_eq!(conversations.snapshot().execution.effective_service_tier().as_str(), "default");
		conversations.submit("Inherit model and effort; explicitly use Standard").unwrap();
		let dispatch = conversations.try_take_dispatch(1, &server_id).unwrap();
		let original = dispatch.command().unwrap();
		let saved = serde_json::to_vec(original).unwrap();
		let restored: decodex_protocol::CommandEnvelope = serde_json::from_slice(&saved).unwrap();
		assert_eq!(&restored, original);
		let decodex_protocol::CommandPayload::SubmitConversationTurn {
			overrides: Some(intent),
			..
		} = restored.payload
		else {
			panic!("turn intent")
		};
		assert!(!intent.model && !intent.reasoning && intent.service_tier);
	}

	#[test]
	fn reverted_selection_cannot_admit_an_old_settings_reply() {
		let (conversations, _, source) = crate::conversations::tests::connected_conversations();
		let epoch = conversations.lock().catalog_epoch;
		let mut other = source.clone();
		other.conversation_id =
			decodex_protocol::EntityId::new("00000000-0000-4000-8000-000000000099").unwrap();
		conversations.lock().tasks.push(other.clone());
		conversations.select(other.conversation_id);
		conversations.select(source.conversation_id.clone());
		let mut state = conversations.lock();
		assert_eq!(
			state.route_model_settings(epoch, &source, &observed("stale-model")).0,
			ConversationRouteOutcome::Refused
		);
		assert!(!state.ordinary_execution_ready());
	}
}
