//! Ordinary editors and unresolved commands retained by the shared desktop draft owner.
use crate::{
	CommandEnvelope, CommandPayload, ConversationExecutionSettings, ConversationWorkingDirectory,
	DesktopCreationIntent, EntityId,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashSet};

/// One ordinary editor, without a claim that native defaults are still current.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopOrdinaryComposerDraft {
	/// Logical conversation owner, absent only before creation.
	pub conversation_id: Option<EntityId>,
	/// Complete text, including an explicit empty edit.
	pub text: String,
	/// Displayed settings; non-explicit values must be observed again after restore.
	pub execution: ConversationExecutionSettings,
	/// Explicit creation choices, kept separately from displayed defaults.
	pub creation_intent: DesktopCreationIntent,
}

/// Ordinary state for one directory within an exact service profile.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopOrdinaryDraft {
	/// Directory used by this controller, not a new launch authorization.
	pub working_directory: ConversationWorkingDirectory,
	/// Currently displayed editor.
	pub composer: DesktopOrdinaryComposerDraft,
	/// Unsent new-conversation editor parked while an existing conversation is selected.
	#[serde(default)]
	pub new_conversation: Option<DesktopOrdinaryComposerDraft>,
	/// Editors parked under their exact conversation identity.
	pub parked: BTreeMap<String, DesktopOrdinaryComposerDraft>,
	/// Original commands whose delivery is unresolved. Never replay these automatically.
	pub unconfirmed: Vec<CommandEnvelope>,
}

impl DesktopOrdinaryComposerDraft {
	pub(super) fn validate(&self) -> Result<(), &'static str> {
		super::desktop_drafts::validate_text(&self.text)
	}
}

impl DesktopOrdinaryDraft {
	pub(super) fn validate(&self) -> Result<(), &'static str> {
		self.composer.validate()?;
		if let Some(draft) = &self.new_conversation {
			if draft.conversation_id.is_some() {
				return Err("New ordinary editor cannot own a conversation");
			}
			draft.validate()?;
		}
		if self.parked.len() > 256 || self.unconfirmed.len() > 64 {
			return Err("Too many ordinary draft records");
		}
		for (owner, draft) in &self.parked {
			if draft.conversation_id.as_ref().map(EntityId::as_str) != Some(owner.as_str()) {
				return Err("Ordinary draft owner does not match");
			}
			draft.validate()?;
		}
		let mut ids = HashSet::new();
		let mut keys = BTreeSet::new();
		for command in &self.unconfirmed {
			if !ids.insert(&command.client_command_id)
				|| !keys.insert(command.idempotency_key.as_str())
			{
				return Err("Duplicate ordinary delivery identity");
			}
			match &command.payload {
				CommandPayload::CreateConversation { working_directory, .. } => {
					if working_directory != &self.working_directory {
						return Err("Ordinary creation directory does not match");
					}
				},
				CommandPayload::SubmitConversationTurn { .. }
				| CommandPayload::ArchiveConversation { .. }
				| CommandPayload::CreateConversationRoutingSuccessor { .. }
				| CommandPayload::InterruptConversation { .. }
				| CommandPayload::RefreshConversation { .. }
				| CommandPayload::ResumeConversationEstablishment { .. }
				| CommandPayload::ResumeConversationRouting { .. } => {},
				_ => return Err("Draft command is not an ordinary conversation operation"),
			}
		}
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{
		CURRENT_VERSION, ClientCommandId, ClientDraftStore, ConversationModel, CorrelationId,
		DesktopDraftDocument, DesktopProfileDraft, HistoryText, IdempotencyKey,
	};

	fn draft(text: &str, key: &str) -> DesktopOrdinaryDraft {
		let execution = ConversationExecutionSettings {
			model: ConversationModel::new("provider-model").unwrap(),
			reasoning_effort: None,
			fast: false,
			service_tier: Some(crate::ServiceTier::new("flex").unwrap()),
		};
		let working_directory = ConversationWorkingDirectory::new("/tmp/work").unwrap();
		DesktopOrdinaryDraft {
			working_directory: working_directory.clone(),
			composer: DesktopOrdinaryComposerDraft {
				conversation_id: None,
				text: text.into(),
				execution: execution.clone(),
				creation_intent: DesktopCreationIntent {
					model: true,
					reasoning: false,
					service_tier: true,
				},
			},
			new_conversation: None,
			parked: BTreeMap::new(),
			unconfirmed: vec![CommandEnvelope {
				version: CURRENT_VERSION,
				client_command_id: ClientCommandId::new(key).unwrap(),
				idempotency_key: IdempotencyKey::new(key).unwrap(),
				expected_revision: None,
				correlation_id: CorrelationId::new(key).unwrap(),
				causation_id: None,
				payload: CommandPayload::CreateConversation {
					conversation_id: EntityId::new("original-conversation").unwrap(),
					message: HistoryText::new("Original submitted input").unwrap(),
					working_directory,
					execution,
				},
			}],
		}
	}

	fn document(text: &str, key: &str) -> DesktopDraftDocument {
		DesktopDraftDocument {
			profiles: BTreeMap::from([(
				"a".repeat(64),
				DesktopProfileDraft {
					ordinary: BTreeMap::from([("/tmp/work".into(), draft(text, key))]),
					..Default::default()
				},
			)]),
			..Default::default()
		}
	}

	#[test]
	fn shared_store_retains_original_command_and_later_editor_after_reopen() {
		let root = tempfile::tempdir().unwrap();
		let path = root.path().canonicalize().unwrap().join("drafts");
		let store = ClientDraftStore::open_at(&path).unwrap();
		let mut original = document("Later input", "command-one");
		let ordinary = original
			.profiles
			.get_mut(&"a".repeat(64))
			.unwrap()
			.ordinary
			.get_mut("/tmp/work")
			.unwrap();
		let mut parked_new = ordinary.composer.clone();
		parked_new.text = "Parked new-conversation input".into();
		ordinary.new_conversation = Some(parked_new);
		store.save(0, &original.encode().unwrap()).unwrap();
		drop(store);
		let reopened = ClientDraftStore::open_at(&path).unwrap();
		let decoded = DesktopDraftDocument::decode(&reopened.load().unwrap().payload).unwrap();
		assert!(decoded == original);
		let saved = &decoded.profiles[&"a".repeat(64)].ordinary["/tmp/work"];
		assert_eq!(saved.composer.text, "Later input");
		assert!(saved.composer.execution.reasoning_effort.is_none());
		assert!(saved.composer.creation_intent.model && !saved.composer.creation_intent.reasoning);
		assert!(
			matches!(&saved.unconfirmed[0].payload, CommandPayload::CreateConversation { message,.. } if message.as_str()=="Original submitted input")
		);
		let old = DesktopDraftDocument::decode(br#"{"version":4,"profiles":{}}"#).unwrap();
		assert_eq!(old.version, 5);
	}

	#[test]
	fn keep_both_and_restore_preserve_every_ordinary_delivery_fence() {
		let local = document("Local edit", "local-command");
		let remote = document("Remote edit", "remote-command");
		let merged = local.reconcile_keep_both(&DesktopDraftDocument::default(), &remote).unwrap();
		let active = &merged.profiles[&"a".repeat(64)];
		assert_eq!(active.ordinary["/tmp/work"].composer.text, "Local edit");
		assert_eq!(active.ordinary["/tmp/work"].unconfirmed.len(), 2);
		assert!(active.has_unconfirmed_delivery() && !active.uncertain);
		let copy = &merged.recovered[0];
		assert!(merged.remove_recovered_copy(copy).is_err());
		let restored = merged.restore_recovered_copy(copy).unwrap();
		let selected = &restored.profiles[&"a".repeat(64)].ordinary["/tmp/work"];
		assert_eq!(selected.composer.text, "Remote edit");
		assert_eq!(selected.unconfirmed.len(), 2);
		let deleted = DesktopDraftDocument::default().reconcile_keep_both(&local, &remote).unwrap();
		assert!(
			deleted.profiles.contains_key(&"a".repeat(64)),
			"deletion cannot hide unresolved delivery"
		);
	}

	#[test]
	fn ordinary_records_reject_foreign_commands_and_ambiguous_owners() {
		let mut value = document("Input", "command");
		let profile = value.profiles.get_mut(&"a".repeat(64)).unwrap();
		profile.ordinary.get_mut("/tmp/work").unwrap().unconfirmed[0].payload =
			CommandPayload::SetDesktopSettings {
				show_in_menu_bar: true,
				auto_activate_quota: None,
			};
		assert!(value.encode().is_err());
		let mut value = document("Input", "command");
		let ordinary =
			value.profiles.get_mut(&"a".repeat(64)).unwrap().ordinary.get_mut("/tmp/work").unwrap();
		ordinary.unconfirmed.push(ordinary.unconfirmed[0].clone());
		assert!(value.encode().is_err());
		let mut value = document("Input", "command");
		let profile = value.profiles.get_mut(&"a".repeat(64)).unwrap();
		profile.ordinary.insert("/wrong".into(), draft("Wrong directory", "second"));
		assert!(value.encode().is_err());
	}
}
