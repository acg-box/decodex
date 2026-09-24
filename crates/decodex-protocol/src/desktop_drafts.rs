//! Local desktop editor snapshots. These records never authorize execution.
use crate::{
	ChiefAttachmentDto, ChiefExecutionOverrides, ChiefSteerIdentity, ChiefTaskReferenceDto,
	EntityId, WireText,
};
use serde::{Deserialize, Serialize};
use std::{
	collections::{BTreeMap, BTreeSet},
	io::{self, Write},
};

#[path = "desktop_draft_recovery.rs"] mod recovery;
pub use recovery::DesktopRecoveredDraft;

/// Versioned local file payload, keyed by the exact client profile's opaque scope.
#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopDraftDocument {
	/// Source-bound alternatives retained by an explicit keep-both conflict action.
	#[serde(default)]
	pub recovered: Vec<DesktopRecoveredDraft>,
	/// Input entered before any service profile was selected. Never a work-owned draft.
	#[serde(default)]
	pub unbound: DesktopComposerDraft,
	/// Local storage schema version, independent of the service wire version.
	pub version: u32,
	/// Saved service-scoped drafts; a missing profile does not authorize migration.
	pub profiles: BTreeMap<String, DesktopProfileDraft>,
}
impl Default for DesktopDraftDocument {
	fn default() -> Self {
		Self {
			version: 1,
			profiles: BTreeMap::new(),
			unbound: Default::default(),
			recovered: vec![],
		}
	}
}

/// One service's unsent inputs and unresolved delivery identity.
#[derive(Clone, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopProfileDraft {
	/// Original command IDs whose delivery is not yet confirmed. Never replay these.
	#[serde(default)]
	pub unconfirmed_commands: Vec<crate::IdempotencyKey>,
	/// Currently displayed primary editor.
	pub composer: DesktopComposerDraft,
	/// Primary editors parked while another work item is selected.
	pub parked: BTreeMap<String, DesktopComposerDraft>,
	/// Explicit next-message settings and their local comparison revision.
	pub execution: BTreeMap<String, (u64, ChiefExecutionOverrides)>,
	/// Monotonic comparison revision used by the explicit settings owner.
	pub execution_revision: u64,
	/// Source-bound question editors, including the retained custom alternative.
	pub questions: Vec<DesktopQuestionDraft>,
	/// An unresolved previous command blocks automatic retry after restoration.
	pub uncertain: bool,
	/// Exact pending command input for receipt reconciliation, never resubmission.
	pub pending: Option<DesktopPendingDraft>,
}

/// Main composer state without runtime defaults or inferred authorization.
#[derive(Clone, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopComposerDraft {
	/// Original work owner; absent only before a task exists.
	pub work_id: Option<EntityId>,
	/// Original native thread, if bound when the draft was saved.
	pub thread_id: Option<WireText>,
	/// Complete editor text. An empty string is an explicit empty edit.
	pub text: String,
	/// Original selected files, not newly discovered attachments.
	pub attachments: Vec<ChiefAttachmentDto>,
	/// Original selected tasks and their native thread identity.
	pub references: Vec<ChiefTaskReferenceDto>,
}

/// A retained asynchronous question editor, bound to its original source.
#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopQuestionDraft {
	/// Local work that owns the native question.
	pub work_id: EntityId,
	/// Native thread used for fresh-history reconciliation.
	pub thread_id: WireText,
	/// Stable native question identity.
	pub question_id: WireText,
	/// Current editable text, including an empty user edit.
	pub text: String,
	/// Last selected named option, without implying that it was submitted.
	pub selected: Option<String>,
	/// Custom alternative retained while a named option is selected.
	pub custom: Option<String>,
	/// Whether the question group was collapsed by the user.
	pub collapsed: bool,
}

/// Captured input that may already have been accepted by the service.
#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopPendingDraft {
	/// Exact native steering receipt identity, when this was a steering command.
	pub steer: Option<ChiefSteerIdentity>,
	/// Original source editor's work owner.
	pub owner: Option<EntityId>,
	/// Text at dispatch time; later edits must not be cleared by its receipt.
	pub text: Option<String>,
	/// Files at dispatch time.
	pub attachments: Option<Vec<ChiefAttachmentDto>>,
	/// Task references at dispatch time.
	pub references: Option<Vec<ChiefTaskReferenceDto>>,
	/// Exact explicit-setting revision captured by the submitted message.
	pub execution: Option<(EntityId, u64)>,
}

impl DesktopDraftDocument {
	/// Decode bounded local data and reject unsupported or incomplete state.
	pub fn decode(bytes: &[u8]) -> Result<Self, &'static str> {
		if bytes.len() > crate::MAX_CLIENT_DRAFT_BYTES {
			return Err("Draft snapshot is too large");
		}
		let value: Self = serde_json::from_slice(bytes).map_err(|_| "Draft snapshot is invalid")?;
		value.validate()?;
		Ok(value)
	}

	/// Encode without permitting serialization to allocate an unbounded payload.
	pub fn encode(&self) -> Result<Vec<u8>, &'static str> {
		self.validate()?;
		let mut output = BoundedOutput(Vec::new());
		serde_json::to_writer(&mut output, self).map_err(|_| "Draft snapshot is too large")?;
		Ok(output.0)
	}

	fn validate(&self) -> Result<(), &'static str> {
		if self.version != 1 {
			return Err("Draft snapshot version is unsupported");
		}
		if self.profiles.len() > 64 {
			return Err("Too many draft profiles");
		}
		if self.recovered.len() > 32 {
			return Err("Too many recovered draft copies");
		}
		for saved in &self.recovered {
			saved.validate()?;
		}
		self.unbound.validate()?;
		if self.unbound.work_id.is_some() || self.unbound.thread_id.is_some() {
			return Err("Unbound draft cannot own a work or native thread");
		}
		for (scope, profile) in &self.profiles {
			if scope.len() != 64
				|| !scope.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
			{
				return Err("Draft profile identity is invalid");
			}
			profile.validate()?;
		}
		Ok(())
	}
}
impl DesktopProfileDraft {
	fn validate(&self) -> Result<(), &'static str> {
		self.composer.validate()?;
		if self.unconfirmed_commands.len() > 64
			|| (!self.unconfirmed_commands.is_empty() && !self.uncertain)
		{
			return Err("Unconfirmed commands require a bounded delivery fence");
		}
		if self.parked.len() > 256 || self.execution.len() > 256 || self.questions.len() > 1024 {
			return Err("Too many draft editors");
		}
		for (work, draft) in &self.parked {
			if draft.work_id.as_ref().map(EntityId::as_str) != Some(work.as_str()) {
				return Err("Draft owner does not match");
			}
			draft.validate()?;
		}
		for (work, (revision, _)) in &self.execution {
			EntityId::new(work).map_err(|_| "Draft execution owner is invalid")?;
			if *revision > self.execution_revision {
				return Err("Draft execution revision is invalid");
			}
		}
		let mut identities = BTreeSet::new();
		for question in &self.questions {
			if !identities.insert((
				question.work_id.as_str(),
				question.thread_id.as_str(),
				question.question_id.as_str(),
			)) {
				return Err("Duplicate question draft identity");
			}
			if question.thread_id.as_str().is_empty() || question.question_id.as_str().is_empty() {
				return Err("Question draft source is empty");
			}
			validate_text(&question.text)?;
			for value in [&question.selected, &question.custom].into_iter().flatten() {
				validate_text(value)?;
			}
		}
		if let Some(pending) = &self.pending {
			if !self.uncertain {
				return Err("Unconfirmed input requires a delivery fence");
			}
			if let Some(text) = &pending.text {
				validate_text(text)?;
			}
			if let Some(attachments) = &pending.attachments
				&& attachments.len() > 64
			{
				return Err("Too many pending attachments");
			}
			if let Some(references) = &pending.references
				&& references.len() > 64
			{
				return Err("Too many pending task references");
			}
			if let Some((owner, revision)) = &pending.execution
				&& (pending.owner.as_ref() != Some(owner) || *revision > self.execution_revision)
			{
				return Err("Pending execution identity does not match");
			}
			if let Some(steer) = &pending.steer
				&& (steer.thread_id.as_str().is_empty() || steer.turn_id.as_str().is_empty())
			{
				return Err("Pending steering source is empty");
			}
			if let Some(steer) = &pending.steer
				&& pending.owner.as_ref() != Some(&steer.work_id)
			{
				return Err("Pending steering owner does not match");
			}
		}
		Ok(())
	}
}
impl DesktopComposerDraft {
	fn validate(&self) -> Result<(), &'static str> {
		if self
			.thread_id
			.as_ref()
			.is_some_and(|thread| self.work_id.is_none() || thread.as_str().is_empty())
		{
			return Err("Draft thread has no work owner");
		}
		validate_text(&self.text)?;
		if self.attachments.len() > 64 || self.references.len() > 64 {
			return Err("Too many draft attachments or references");
		}
		Ok(())
	}
}
fn validate_text(text: &str) -> Result<(), &'static str> {
	if text.len() > 16 * 1024 { Err("Draft editor text is too large") } else { Ok(()) }
}
struct BoundedOutput(Vec<u8>);
impl Write for BoundedOutput {
	fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
		if self.0.len().saturating_add(bytes.len()) > crate::MAX_CLIENT_DRAFT_BYTES {
			return Err(io::Error::other("draft snapshot limit"));
		}
		self.0.extend_from_slice(bytes);
		Ok(bytes.len())
	}

	fn flush(&mut self) -> io::Result<()> {
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn document() -> DesktopDraftDocument {
		let mut profile = DesktopProfileDraft {
			composer: DesktopComposerDraft {
				work_id: Some(EntityId::new("work").unwrap()),
				thread_id: Some(WireText::new("native-thread").unwrap()),
				text: "Keep the draft — 未发送".into(),
				attachments: vec![ChiefAttachmentDto {
					path: crate::ConversationWorkingDirectory::new("/tmp/selected image.png")
						.unwrap(),
					image: true,
				}],
				references: vec![ChiefTaskReferenceDto {
					work_id: EntityId::new("related").unwrap(),
					thread_id: WireText::new("original-related-thread").unwrap(),
					title: WireText::new("Reference").unwrap(),
				}],
			},
			execution_revision: 4,
			uncertain: true,
			..Default::default()
		};
		profile.execution.insert(
			"work".into(),
			(
				4,
				ChiefExecutionOverrides {
					reasoning_effort: Some(crate::ConversationReasoningEffort::High),
					..Default::default()
				},
			),
		);
		profile.questions.push(DesktopQuestionDraft {
			work_id: EntityId::new("work").unwrap(),
			thread_id: WireText::new("native-thread").unwrap(),
			question_id: WireText::new("question").unwrap(),
			text: "".into(),
			selected: Some("Option".into()),
			custom: Some("Custom alternative".into()),
			collapsed: true,
		});
		profile.pending = Some(DesktopPendingDraft {
			steer: Some(ChiefSteerIdentity {
				work_id: EntityId::new("work").unwrap(),
				thread_id: WireText::new("native-thread").unwrap(),
				turn_id: WireText::new("native-turn").unwrap(),
				submission_id: crate::IdempotencyKey::new("original-submission").unwrap(),
			}),
			owner: Some(EntityId::new("work").unwrap()),
			text: Some("Older submitted text".into()),
			attachments: None,
			references: None,
			execution: Some((EntityId::new("work").unwrap(), 4)),
		});
		DesktopDraftDocument {
			version: 1,
			profiles: BTreeMap::from([("a".repeat(64), profile)]),
			recovered: vec![],
			unbound: Default::default(),
		}
	}

	#[test]
	fn draft_document_survives_file_reopen_with_empty_and_uncertain_input() {
		let directory = tempfile::tempdir().unwrap();
		let root = directory.path().canonicalize().unwrap().join("desktop");
		let store = crate::ClientDraftStore::open_at(&root).unwrap();
		let original = document();
		store.save(0, &original.encode().unwrap()).unwrap();
		drop(store);
		let reopened = crate::ClientDraftStore::open_at(&root).unwrap();
		let decoded = DesktopDraftDocument::decode(&reopened.load().unwrap().payload).unwrap();
		assert!(decoded == original);
		let profile = &decoded.profiles[&"a".repeat(64)];
		assert!(profile.questions[0].text.is_empty());
		assert_eq!(
			profile.pending.as_ref().unwrap().steer.as_ref().unwrap().submission_id.as_str(),
			"original-submission"
		);
		assert!(profile.uncertain);
	}

	#[test]
	fn draft_document_rejects_changed_contract_and_ambiguous_ownership() {
		let mut original = document();
		original.version = 2;
		assert!(original.encode().is_err());
		let mut json = serde_json::to_value(document()).unwrap();
		json["unexpected"] = serde_json::json!(true);
		assert!(DesktopDraftDocument::decode(&serde_json::to_vec(&json).unwrap()).is_err());
		let mut original = document();
		let profile = original.profiles.values_mut().next().unwrap();
		profile.questions.push(profile.questions[0].clone());
		assert!(original.encode().is_err());
		let mut original = document();
		original.profiles.values_mut().next().unwrap().uncertain = false;
		assert!(original.encode().is_err());
		let mut original = document();
		let profile = original.profiles.values_mut().next().unwrap();
		profile.parked.insert("different-work".into(), profile.composer.clone());
		assert!(original.encode().is_err());
	}

	#[test]
	fn draft_document_preserves_nonsteer_identity_and_requires_its_fence() {
		let mut doc = document();
		let profile = doc.profiles.values_mut().next().unwrap();
		profile.pending = None;
		profile.unconfirmed_commands =
			vec![crate::IdempotencyKey::new("original-command").unwrap()];
		let restored = DesktopDraftDocument::decode(&doc.encode().unwrap()).unwrap();
		assert_eq!(
			restored.profiles.values().next().unwrap().unconfirmed_commands[0].as_str(),
			"original-command"
		);
		doc.profiles.values_mut().next().unwrap().uncertain = false;
		assert!(doc.encode().is_err());
	}

	#[test]
	fn unbound_draft_requires_no_work_identity_and_preserves_old_documents() {
		let mut doc = DesktopDraftDocument::default();
		doc.unbound.text = "Unassigned input".into();
		assert_eq!(
			DesktopDraftDocument::decode(&doc.encode().unwrap()).unwrap().unbound.text,
			"Unassigned input"
		);
		doc.unbound.work_id = Some(EntityId::new("work").unwrap());
		assert!(doc.encode().is_err());
		let old = br#"{"version":1,"profiles":{}}"#;
		assert!(DesktopDraftDocument::decode(old).unwrap().unbound.text.is_empty());
	}

	#[test]
	fn draft_document_enforces_editor_and_encoded_aggregate_limits() {
		let mut original = document();
		original.profiles.values_mut().next().unwrap().composer.text = "x".repeat(16 * 1024 + 1);
		assert!(original.encode().is_err());
		let mut original = document();
		let profile = original.profiles.values_mut().next().unwrap();
		for index in 0..128 {
			profile.parked.insert(
				format!("work-{index}"),
				DesktopComposerDraft {
					work_id: Some(EntityId::new(format!("work-{index}")).unwrap()),
					text: "\0".repeat(16 * 1024),
					..Default::default()
				},
			);
		}
		assert_eq!(original.encode().unwrap_err(), "Draft snapshot is too large");
	}
}
