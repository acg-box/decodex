//! Lossless local editing of canonical app-server user input.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::ops::Range;

/// Canonical input parts retained together through editing, undo and persistence.
///
/// This is local draft data, not permission to revert history or submit a turn.
/// Non-text parts and unknown fields remain unchanged. Callers must qualify the
/// installed native input contract before confirmation or submission.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct PromptDraft(Vec<Value>);

/// A canonical editor retained in its exact service profile before draft handback.
/// The native receipt remains the authority for mutation and acknowledgement.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopPromptEditDraft {
	/// Local task whose native history was reviewed.
	pub work_id: crate::EntityId,
	/// Exact native thread; a new thread cannot inherit this editor.
	pub thread_id: crate::WireText,
	/// First native turn excluded if the reviewed edit is confirmed.
	pub before_turn_id: crate::WireText,
	/// Original user item used to reopen an expired review.
	pub item_id: crate::WireText,
	/// Digest of the unedited native input used to detect a changed history source.
	pub original_hash: crate::Sha256Digest,
	/// Service-held review identity, also retained after native confirmation.
	pub review_token: crate::WireText,
	/// Durable native edit receipt, absent while the user is still reviewing.
	pub receipt_id: Option<i64>,
	/// Exact confirmation command retained before dispatch until a native receipt is read.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub confirmation_key: Option<crate::IdempotencyKey>,
	/// A single pending send; input must remain unchanged until acceptance is resolved.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub pending_send: Option<crate::PromptInputSend>,
	/// Confirmation may be in flight, or draft handback is not yet confirmed by the service.
	pub handback_pending: bool,
	/// Complete editable input, never a flattened history preview.
	pub input: PromptDraft,
}

impl DesktopPromptEditDraft {
	/// Bind a single send to the complete saved input before any submission.
	pub fn begin_send(
		&self,
		input_id: i64,
		command_key: crate::IdempotencyKey,
		execution: crate::ChiefExecutionOverrides,
	) -> Result<Self, &'static str> {
		self.validate()?;
		if self.pending_send.is_some() || self.receipt_id.is_none() || self.handback_pending {
			return Err("Resolve the existing edit or send before submitting input");
		}
		self.input.validate_native_input()?;
		let mut pending = self.clone();
		pending.pending_send = Some(crate::PromptInputSend {
			input_id,
			sha256: self.input.fingerprint()?,
			command_key,
			execution,
		});
		pending.validate()?;
		Ok(pending)
	}

	/// Read the unchanged owner and send identity for acceptance reconciliation.
	pub fn send_identity(&self) -> Result<crate::PromptInputSendIdentity, &'static str> {
		self.validate()?;
		Ok(crate::PromptInputSendIdentity {
			work_id: self.work_id.clone(),
			thread_id: self.thread_id.clone(),
			edit_receipt_id: self.receipt_id.ok_or("Edit receipt is missing")?,
			send: self.pending_send.clone().ok_or("No retained send identity")?,
		})
	}

	/// Retain confirmation intent before any native history mutation can be sent.
	pub fn begin_confirmation(&self, key: crate::IdempotencyKey) -> Result<Self, &'static str> {
		self.validate()?;
		if self.receipt_id.is_some() || self.handback_pending || self.confirmation_key.is_some() {
			return Err("Recover the existing history edit first");
		}
		let mut pending = self.clone();
		pending.confirmation_key = Some(key);
		pending.handback_pending = true;
		Ok(pending)
	}

	/// Bind a recovered native receipt without replacing the user's edited input.
	pub fn recover_receipt(
		&self,
		status: &crate::PromptEditStatus,
		original: &PromptDraft,
	) -> Result<Self, &'static str> {
		self.validate()?;
		let evidence = status.evidence.as_ref().ok_or("History edit receipt is unavailable")?;
		if !status.is_valid()
			|| status.work_id != self.work_id
			|| status.thread_id != self.thread_id
			|| evidence.review_token != self.review_token
			|| evidence.before_turn_id != self.before_turn_id
			|| evidence.item_id != self.item_id
			|| original.fingerprint()? != self.original_hash
			|| self.receipt_id.is_some_and(|id| Some(id) != evidence.receipt_id)
			|| !matches!(
				status.phase,
				crate::PromptEditPhase::Uncertain
					| crate::PromptEditPhase::Applied
					| crate::PromptEditPhase::Restored
					| crate::PromptEditPhase::Unchanged
			) {
			return Err("History edit source changed; retain this draft separately");
		}
		let mut recovered = self.clone();
		recovered.confirmation_key = None;
		recovered.receipt_id = if status.phase == crate::PromptEditPhase::Unchanged {
			None
		} else {
			evidence.receipt_id
		};
		recovered.handback_pending = matches!(
			status.phase,
			crate::PromptEditPhase::Uncertain | crate::PromptEditPhase::Applied
		);
		Ok(recovered)
	}

	/// Use a fresh review only when its original input still matches this edited draft.
	pub fn refresh_review(&self, fresh: &Self) -> Result<Self, &'static str> {
		self.validate()?;
		fresh.validate()?;
		if self.receipt_id.is_some()
			|| fresh.receipt_id.is_some()
			|| self.handback_pending
			|| fresh.handback_pending
		{
			return Err("Recover the existing history edit first");
		}
		if self.work_id != fresh.work_id
			|| self.thread_id != fresh.thread_id
			|| self.before_turn_id != fresh.before_turn_id
			|| self.item_id != fresh.item_id
			|| self.original_hash != fresh.original_hash
			|| fresh.input.fingerprint()? != fresh.original_hash
		{
			return Err(
				"Original input changed; keep this draft and review the current history separately",
			);
		}
		let mut result = fresh.clone();
		result.input = self.input.clone();
		Ok(result)
	}

	pub(crate) fn validate(&self) -> Result<(), &'static str> {
		if self.thread_id.as_str().is_empty()
			|| self.before_turn_id.as_str().is_empty()
			|| self.item_id.as_str().is_empty()
			|| self.review_token.as_str().len() != 64
			|| !self.review_token.as_str().bytes().all(|b| b.is_ascii_hexdigit())
			|| self.receipt_id.is_some_and(|id| id <= 0)
			|| self.handback_pending && self.receipt_id.is_none() && self.confirmation_key.is_none()
			|| self.confirmation_key.is_some()
				&& (!self.handback_pending || self.receipt_id.is_some())
		{
			return Err("Prompt draft source is invalid");
		}
		if let Some(send) = &self.pending_send
			&& (send.input_id <= 0
				|| self.receipt_id.is_none()
				|| self.handback_pending
				|| self.confirmation_key.is_some()
				|| send.sha256 != self.input.fingerprint()?)
		{
			return Err("Pending prompt send does not match the retained draft");
		}

		self.input.validate()
	}
}

impl PromptDraft {
	/// Check the seven public input variants at the fixed Codex cutoff.
	/// Local file readability and the complete native request envelope are separate checks.
	pub fn validate_native_input(&self) -> Result<(), &'static str> {
		self.validate()?;
		let mut text_chars = 0usize;
		for part in &self.0 {
			let string = |key: &str| part.get(key).and_then(Value::as_str).is_some();
			let valid = match part["type"].as_str() {
				Some("text") => {
					text_chars = text_chars.saturating_add(
						part["text"].as_str().ok_or("Input text is missing")?.chars().count(),
					);
					true
				},
				Some("image") => string("url") || string("fileId"),
				Some("localImage" | "localAudio") => string("path"),
				Some("audio") => string("url"),
				Some("skill" | "mention") => string("name") && string("path"),
				_ =>
					return Err(
						"This native input type is not supported by the qualified Codex contract",
					),
			};
			if !valid {
				return Err("A native input part is incomplete");
			}
			if matches!(part["type"].as_str(), Some("image" | "localImage"))
				&& !part["detail"].is_null()
				&& !matches!(part["detail"].as_str(), Some("auto" | "low" | "high" | "original"))
			{
				return Err("Image detail is not supported by the qualified Codex contract");
			}
		}
		// Upstream protocol::user_input and TurnProcessor::validate_v2_input_limit.
		if text_chars > 1 << 20 {
			return Err("Input exceeds the native limit of 1048576 text characters");
		}
		Ok(())
	}

	/// Hash complete canonical parts without flattening native input or ignoring fields.
	pub fn fingerprint(&self) -> Result<crate::Sha256Digest, &'static str> {
		let bytes = serde_json::to_vec(&self.0).map_err(|_| "Prompt encoding failed")?;
		crate::Sha256Digest::new(decodex_core::BlobHash::digest(&bytes).to_hex())
			.map_err(|_| "Prompt digest is invalid")
	}

	/// Retain complete native parts after checking editable UTF-8 text markers.
	pub fn new(parts: Vec<Value>) -> Result<Self, &'static str> {
		let draft = Self(parts);
		draft.validate()?;
		Ok(draft)
	}

	/// Read the complete canonical input without flattening attachments or bindings.
	pub fn parts(&self) -> &[Value] {
		&self.0
	}

	/// Capture one edited part without replacing the surrounding attachments and bindings.
	pub fn replace_part(&mut self, index: usize, part: Value) -> Result<(), &'static str> {
		Self::new(vec![part.clone()])?;
		*self.0.get_mut(index).ok_or("Input part is missing")? = part;
		Ok(())
	}

	/// Remove an explicitly selected input part. An empty prompt cannot be submitted.
	pub fn remove_part(&mut self, index: usize) -> Result<(), &'static str> {
		if index >= self.0.len() {
			return Err("Input part is missing");
		}
		if self.0.len() == 1 {
			return Err("Keep at least one input part");
		}
		self.0.remove(index);
		Ok(())
	}

	/// Remove an input and explicitly identified text markers as one local edit.
	///
	/// Marker identities refer to the original draft. Callers must establish their
	/// association from native evidence or an explicit user choice, not text search.
	/// Any ambiguous overlap leaves the complete draft unchanged.
	pub fn remove_bound_part(
		&mut self,
		part_index: usize,
		markers: &[(usize, usize)],
	) -> Result<(), &'static str> {
		self.validate()?;
		if self.0.get(part_index).is_none_or(|part| part["type"] == "text") {
			return Err("Select a non-text input to remove");
		}
		let mut selected = std::collections::BTreeMap::<usize, Vec<(usize, Range<usize>)>>::new();
		let mut identities = std::collections::BTreeSet::new();
		for &(text_index, element_index) in markers {
			if !identities.insert((text_index, element_index)) {
				return Err("Text marker was selected twice");
			}
			let part = self.0.get(text_index).ok_or("Input part is missing")?;
			if part["type"] != "text" {
				return Err("Selected marker does not belong to text");
			}
			let element = elements(part)?.get(element_index).ok_or("Text marker is missing")?;
			selected.entry(text_index).or_default().push((element_index, element_range(element)?));
		}
		let mut updated = self.clone();
		for (text_index, mut targets) in selected {
			let original = elements(&self.0[text_index])?;
			let retained: Vec<_> = original
				.iter()
				.enumerate()
				.filter(|(index, _)| !identities.contains(&(text_index, *index)))
				.map(|(_, element)| element.clone())
				.collect();
			updated.0[text_index]["text_elements"] = Value::Array(retained);
			targets.sort_by_key(|(_, range)| (range.start, range.end));
			if targets.windows(2).any(|pair| pair[0].1.end > pair[1].1.start) {
				return Err("Selected text markers overlap");
			}
			for (_, range) in targets.into_iter().rev() {
				updated.replace_text(text_index, range, "")?;
			}
		}
		updated.remove_part(part_index)?;
		*self = updated;
		Ok(())
	}

	/// Check locally editable structure, including data loaded from disk.
	pub fn validate(&self) -> Result<(), &'static str> {
		if self.0.is_empty() {
			return Err("Prompt has no input parts");
		}
		for part in &self.0 {
			let kind = part.get("type").and_then(Value::as_str).ok_or("Input type is missing")?;
			if kind == "text" {
				let text =
					part.get("text").and_then(Value::as_str).ok_or("Input text is missing")?;
				for element in elements(part)? {
					let range = element_range(element)?;
					if text.get(range).is_none() {
						return Err("Text marker is outside a UTF-8 boundary");
					}
				}
			}
		}
		Ok(())
	}

	/// Replace a UTF-8 text range while preserving every other native input field.
	///
	/// Markers are atomic: an edit inside one is rejected without changing the draft.
	/// Insertions at a marker's start precede it; insertions at its end follow it.
	/// An explicit marker-removal control must handle removing bound input separately.
	pub fn replace_text(
		&mut self,
		part_index: usize,
		range: Range<usize>,
		replacement: &str,
	) -> Result<(), &'static str> {
		self.validate()?;
		let part = self.0.get(part_index).ok_or("Input part is missing")?;
		if part.get("type").and_then(Value::as_str) != Some("text") {
			return Err("Input part is not text");
		}
		let text = part["text"].as_str().ok_or("Input text is missing")?;
		if text.get(range.clone()).is_none() {
			return Err("Edit is outside a UTF-8 boundary");
		}
		let mut updated = part.clone();
		if let Some(markers) = updated.get_mut("text_elements").and_then(Value::as_array_mut) {
			for marker in markers {
				let old = element_range(marker)?;
				if range.start < old.end && range.end > old.start
					|| range.is_empty() && old.start < range.start && range.start < old.end
				{
					return Err("Remove the bound marker explicitly before editing it");
				}
				if range.end <= old.start {
					let start = old.start - range.len() + replacement.len();
					let end = old.end - range.len() + replacement.len();
					marker["byteRange"]["start"] = Value::from(start);
					marker["byteRange"]["end"] = Value::from(end);
				}
			}
		}
		let mut text = text.to_owned();
		text.replace_range(range, replacement);
		updated["text"] = Value::String(text);
		self.0[part_index] = updated;
		Ok(())
	}
}

fn elements(part: &Value) -> Result<&[Value], &'static str> {
	match part.get("text_elements") {
		None => Ok(&[]),
		Some(Value::Array(elements)) => Ok(elements),
		Some(_) => Err("Text markers are invalid"),
	}
}

fn element_range(element: &Value) -> Result<Range<usize>, &'static str> {
	let offset = |name| {
		element
			.get("byteRange")
			.and_then(|v| v.get(name))
			.and_then(Value::as_u64)
			.and_then(|n| usize::try_from(n).ok())
			.ok_or("Text marker offset is invalid")
	};
	let start = offset("start")?;
	let end = offset("end")?;
	if start > end {
		return Err("Text marker range is reversed");
	}
	Ok(start..end)
}

#[cfg(test)]
mod tests {
	use super::*;
	use serde_json::json;

	fn sample() -> PromptDraft {
		PromptDraft::new(vec![
			json!({"type":"text","text":"你 $skill end","text_elements":[{"byteRange":{"start":4,"end":10},"placeholder":"$skill","extension":true}],"extension":"retain"}),
			json!({"type":"skill","name":"skill","path":"/tmp/SKILL.md"}),
			json!({"type":"mention","name":"app","path":"app://exact-id"}),
			json!({"type":"image","fileId":"native-file","detail":"original"}),
			json!({"type":"audio","url":"data:audio/wav;base64,example"}),
			json!({"type":"future-input","evidence":{"keep":true}}),
		]).unwrap()
	}

	#[test]
	fn native_input_qualification_counts_unicode_across_parts_and_preserves_all_variants() {
		let mut draft = PromptDraft::new(vec![
			json!({"type":"text","text":"界".repeat(1 << 19),"text_elements":[]}),
			json!({"type":"text","text":"a".repeat(1 << 19)}),
			json!({"type":"image","fileId":"file","detail":"original","extension":true}),
			json!({"type":"localImage","path":"/fixture/image.png"}),
			json!({"type":"audio","url":"data:audio/wav;base64,AA=="}),
			json!({"type":"localAudio","path":"/fixture/audio.wav"}),
			json!({"type":"skill","name":"skill","path":"/fixture/SKILL.md"}),
			json!({"type":"mention","name":"App","path":"app://fixture"}),
		])
		.unwrap();
		let unchanged = draft.clone();
		draft.validate_native_input().unwrap();
		assert_eq!(draft, unchanged);
		draft.replace_text(1, 0..0, "x").unwrap();
		assert!(draft.validate_native_input().is_err());
		for part in [
			json!({"type":"image"}),
			json!({"type":"audio","audio_url":"old-internal-field"}),
			json!({"type":"localAudio"}),
			json!({"type":"skill","path":"/skill"}),
			json!({"type":"futureInput"}),
			json!({"type":"image","url":"data:image/png;base64,AA==","detail":"unsupported"}),
		] {
			let retained = PromptDraft::new(vec![part.clone()]).unwrap();
			assert!(retained.validate_native_input().is_err());
			assert_eq!(retained.parts(), &[part]);
		}
	}

	#[test]
	fn renewed_review_preserves_edits_only_for_the_same_original_input() {
		let mut saved = DesktopPromptEditDraft {
			work_id: crate::EntityId::new("work").unwrap(),
			thread_id: crate::WireText::new("thread").unwrap(),
			before_turn_id: crate::WireText::new("turn").unwrap(),
			item_id: crate::WireText::new("item").unwrap(),
			original_hash: sample().fingerprint().unwrap(),
			review_token: crate::WireText::new("a".repeat(64)).unwrap(),
			receipt_id: None,
			handback_pending: false,
			confirmation_key: None,
			pending_send: None,
			input: sample(),
		};
		let mut fresh = saved.clone();
		fresh.review_token = crate::WireText::new("b".repeat(64)).unwrap();
		saved.input.replace_text(0, 0..3, "Edited").unwrap();
		let renewed = saved.refresh_review(&fresh).unwrap();
		assert_eq!(renewed.input, saved.input);
		assert_eq!(renewed.review_token, fresh.review_token);
		fresh.input.replace_part(3, json!({"type":"image","fileId":"different"})).unwrap();
		assert!(saved.refresh_review(&fresh).is_err());
		fresh.original_hash = fresh.input.fingerprint().unwrap();
		assert!(saved.refresh_review(&fresh).is_err());
		fresh.input = sample();
		fresh.original_hash = fresh.input.fingerprint().unwrap();
		fresh.thread_id = crate::WireText::new("another-thread").unwrap();
		assert!(saved.refresh_review(&fresh).is_err());
		fresh.thread_id = saved.thread_id.clone();
		saved.receipt_id = Some(42);
		assert!(saved.refresh_review(&fresh).is_err());
	}

	#[test]
	fn recovered_receipt_preserves_edits_and_rejects_crossed_history() {
		let original = sample();
		let mut draft = DesktopPromptEditDraft {
			work_id: crate::EntityId::new("work").unwrap(),
			thread_id: crate::WireText::new("thread").unwrap(),
			before_turn_id: crate::WireText::new("turn").unwrap(),
			item_id: crate::WireText::new("item").unwrap(),
			original_hash: original.fingerprint().unwrap(),
			review_token: crate::WireText::new("a".repeat(64)).unwrap(),
			receipt_id: None,
			handback_pending: false,
			confirmation_key: None,
			pending_send: None,
			input: original.clone(),
		};
		draft.input.replace_text(0, 0..3, "Edited").unwrap();
		draft =
			draft.begin_confirmation(crate::IdempotencyKey::new("confirm-once").unwrap()).unwrap();
		assert!(draft.receipt_id.is_none() && draft.handback_pending);
		assert!(
			draft.begin_confirmation(crate::IdempotencyKey::new("do-not-repeat").unwrap()).is_err()
		);
		let mut document = crate::DesktopDraftDocument::default();
		document
			.profiles
			.entry("a".repeat(64))
			.or_default()
			.prompt_edits
			.insert(draft.review_token.as_str().into(), draft.clone());
		let directory = tempfile::tempdir().unwrap();
		let path = directory.path().canonicalize().unwrap().join("desktop");
		let store = crate::ClientDraftStore::open_at(&path).unwrap();
		store.save(0, &document.encode().unwrap()).unwrap();
		drop(store);
		let reopened = crate::ClientDraftStore::open_at(&path).unwrap();
		let restored =
			crate::DesktopDraftDocument::decode(&reopened.load().unwrap().payload).unwrap();
		let profile = &restored.profiles[&"a".repeat(64)];
		assert!(profile.has_unconfirmed_delivery());
		assert_pending_send_retained(&draft, &reopened);

		assert_eq!(profile.prompt_edits[draft.review_token.as_str()], draft);
		let fragment = serde_json::to_string(&original).unwrap();
		let mut status = crate::PromptEditStatus {
			work_id: draft.work_id.clone(),
			thread_id: draft.thread_id.clone(),
			phase: crate::PromptEditPhase::Applied,
			evidence: Some(crate::PromptEditEvidence {
				review_token: draft.review_token.clone(),
				receipt_id: Some(42),
				before_turn_id: draft.before_turn_id.clone(),
				item_id: draft.item_id.clone(),
				removed_turns: 2,
				content_bytes: fragment.len() as u64,
				offset: 0,
				fragment,
			}),
		};
		for phase in [
			crate::PromptEditPhase::Uncertain,
			crate::PromptEditPhase::Applied,
			crate::PromptEditPhase::Restored,
		] {
			status.phase = phase;
			let recovered = draft.recover_receipt(&status, &original).unwrap();
			assert_eq!(recovered.input, draft.input);
			assert_eq!(recovered.receipt_id, Some(42));
			assert!(recovered.confirmation_key.is_none());
			assert_eq!(recovered.handback_pending, phase != crate::PromptEditPhase::Restored);
		}
		status.phase = crate::PromptEditPhase::Unchanged;
		let unchanged = draft.recover_receipt(&status, &original).unwrap();
		assert!(
			unchanged.receipt_id.is_none()
				&& unchanged.confirmation_key.is_none()
				&& !unchanged.handback_pending
		);
		assert_eq!(unchanged.input, draft.input);
		assert!(draft.recover_receipt(&status, &draft.input).is_err());
		draft.confirmation_key = None;
		draft.receipt_id = Some(43);
		assert!(draft.recover_receipt(&status, &original).is_err());
		draft.receipt_id = None;
		status.thread_id = crate::WireText::new("other").unwrap();
		assert!(draft.recover_receipt(&status, &original).is_err());
	}

	fn assert_pending_send_retained(
		draft: &DesktopPromptEditDraft,
		reopened: &crate::ClientDraftStore,
	) {
		let mut restored_edit = draft.clone();
		restored_edit.confirmation_key = None;
		restored_edit.handback_pending = false;
		restored_edit.receipt_id = Some(42);
		assert!(
			restored_edit
				.begin_send(
					7,
					crate::IdempotencyKey::new("unsupported").unwrap(),
					Default::default()
				)
				.is_err()
		);
		restored_edit.input.remove_part(5).unwrap();
		let sending = restored_edit
			.begin_send(
				7,
				crate::IdempotencyKey::new("send-once").unwrap(),
				crate::ChiefExecutionOverrides::default(),
			)
			.unwrap();
		assert_eq!(sending.send_identity().unwrap().send.command_key.as_str(), "send-once");
		assert!(
			sending
				.begin_send(
					7,
					crate::IdempotencyKey::new("do-not-replay").unwrap(),
					Default::default()
				)
				.is_err()
		);
		let mut changed = sending.clone();
		changed.input.replace_text(0, 0..0, "changed").unwrap();
		assert!(changed.validate().is_err());
		let mut sending_document = crate::DesktopDraftDocument::default();
		sending_document
			.profiles
			.entry("a".repeat(64))
			.or_default()
			.prompt_edits
			.insert(sending.review_token.as_str().into(), sending.clone());
		let next_revision = reopened.load().unwrap().revision;
		reopened.save(next_revision, &sending_document.encode().unwrap()).unwrap();
		let reopened_send =
			crate::DesktopDraftDocument::decode(&reopened.load().unwrap().payload).unwrap();
		assert!(reopened_send.profiles[&"a".repeat(64)].has_unconfirmed_delivery());
		assert_eq!(
			reopened_send.profiles[&"a".repeat(64)].prompt_edits[sending.review_token.as_str()],
			sending
		);
	}

	#[test]
	fn unicode_edit_remaps_spans_without_losing_native_parts_or_extensions() {
		let mut draft = sample();
		let before = draft.clone();
		draft.replace_text(0, 0..3, "hello").unwrap();
		assert_eq!(draft.parts()[0]["text"], "hello $skill end");
		assert_eq!(draft.parts()[0]["text_elements"][0]["byteRange"], json!({"start":6,"end":12}));
		assert_eq!(draft.parts()[0]["text_elements"][0]["extension"], true);
		assert_eq!(draft.parts()[0]["extension"], "retain");
		assert_eq!(&draft.parts()[1..], &before.parts()[1..]);
		let saved = serde_json::to_vec(&draft).unwrap();
		let restored: PromptDraft = serde_json::from_slice(&saved).unwrap();
		restored.validate().unwrap();
		assert_eq!(restored, draft);
		// Undo retains the complete value, not just its visible text.
		draft = before;
		assert_eq!(draft, sample());
	}

	#[test]
	fn invalid_or_bound_edits_leave_the_entire_draft_unchanged() {
		for range in [1..2, Range { start: 8, end: 7 }, 0..100, 5..6, 5..5, 4..10] {
			let mut draft = sample();
			assert!(draft.replace_text(0, range, "x").is_err());
			assert_eq!(draft, sample());
		}
		assert!(sample().replace_text(3, 0..0, "x").is_err());
	}

	#[test]
	fn insertions_at_marker_edges_preserve_its_exact_text() {
		for position in [4, 10] {
			let mut draft = sample();
			draft.replace_text(0, position..position, "你好").unwrap();
			let part = &draft.parts()[0];
			let range = element_range(&part["text_elements"][0]).unwrap();
			assert_eq!(&part["text"].as_str().unwrap()[range], "$skill");
		}
	}

	#[test]
	fn large_text_is_never_silently_shortened() {
		let text = "界".repeat(20_000);
		let mut draft = PromptDraft::new(vec![json!({"type":"text","text":text})]).unwrap();
		draft.replace_text(0, 0..3, "hello").unwrap();
		assert_eq!(draft.parts()[0]["text"].as_str().unwrap().len(), text.len() + 2);
		assert!(draft.parts()[0].get("text_elements").is_none());
	}

	#[test]
	fn capturing_an_editor_part_preserves_attachments_and_rejects_invalid_replacements() {
		let mut draft = sample();
		let mut editor = PromptDraft::new(vec![draft.parts()[0].clone()]).unwrap();
		editor.replace_text(0, 0..3, "hello").unwrap();
		draft.replace_part(0, editor.parts()[0].clone()).unwrap();
		assert_eq!(&draft.parts()[1..], &sample().parts()[1..]);
		let before = draft.clone();
		assert!(draft.replace_part(0, serde_json::json!({"type":"text"})).is_err());
		assert_eq!(draft, before);
		draft.remove_part(3).unwrap();
		assert!(!draft.parts().iter().any(|part| part.get("fileId").is_some()));
		assert_eq!(draft.parts()[3]["type"], "audio");
		assert!(editor.remove_part(0).is_err());
	}

	#[test]
	fn removing_bound_input_is_atomic_and_does_not_search_plain_text() {
		let mut draft = sample();
		let image = draft.parts()[3].clone();
		draft.remove_bound_part(1, &[(0, 0)]).unwrap();
		assert_eq!(draft.parts()[0]["text"], "你  end");
		assert_eq!(draft.parts()[0]["text_elements"], json!([]));
		assert_eq!(draft.parts()[2], image);
		assert_eq!(draft.parts()[0]["extension"], "retain");
		let mut original = sample();
		assert!(original.remove_bound_part(1, &[(0, 0), (0, 0)]).is_err());
		assert_eq!(original, sample());
		assert!(original.remove_bound_part(1, &[(0, 99)]).is_err());
		assert_eq!(original, sample());
		original.remove_bound_part(1, &[]).unwrap();
		assert_eq!(original.parts()[0], sample().parts()[0]);
		let mut overlapping = sample();
		overlapping.0[0]["text_elements"]
			.as_array_mut()
			.unwrap()
			.push(json!({"byteRange":{"start":7,"end":10},"placeholder":"overlap"}));
		let before = overlapping.clone();
		assert!(overlapping.remove_bound_part(1, &[(0, 0)]).is_err());
		assert_eq!(overlapping, before);
		assert!(overlapping.remove_bound_part(1, &[(0, 0), (0, 1)]).is_err());
		assert_eq!(overlapping, before);
	}

	#[test]
	fn removing_multiple_markers_remaps_retained_unicode_ranges() {
		let mut draft = PromptDraft::new(vec![
			json!({"type":"text","text":"a界b好c","text_elements":[
				{"byteRange":{"start":1,"end":4},"placeholder":"first"},
				{"byteRange":{"start":5,"end":8},"placeholder":"keep"},
				{"byteRange":{"start":8,"end":9},"placeholder":"last"}]}),
			json!({"type":"image","fileId":"remove"}),
		])
		.unwrap();
		draft.remove_bound_part(1, &[(0, 2), (0, 0)]).unwrap();
		assert_eq!(draft.parts()[0]["text"], "ab好");
		assert_eq!(draft.parts()[0]["text_elements"][0]["byteRange"], json!({"start":2,"end":5}));
		assert_eq!(draft.parts()[0]["text_elements"][0]["placeholder"], "keep");
	}

	#[test]
	fn large_native_input_and_conflicting_copy_survive_the_existing_draft_store() {
		use crate::{
			ClientDraftStore, DesktopDraftDocument, DesktopProfileDraft, EntityId, WireText,
		};
		let input = PromptDraft::new(vec![
			json!({"type":"text","text":"Original"}),
			json!({"type":"image","url":format!("data:image/png;base64,{}", "A".repeat(6 * 1024 * 1024))}),
		])
		.unwrap();
		let review = "b".repeat(64);
		let scope = "a".repeat(64);
		let mut profile = DesktopProfileDraft::default();
		profile.composer.text = "Other unsent input".into();
		profile.prompt_edits.insert(
			review.clone(),
			DesktopPromptEditDraft {
				work_id: EntityId::new("work").unwrap(),
				thread_id: WireText::new("thread").unwrap(),
				before_turn_id: WireText::new("turn").unwrap(),
				item_id: WireText::new("item").unwrap(),
				original_hash: input.fingerprint().unwrap(),
				review_token: WireText::new(&review).unwrap(),
				receipt_id: Some(42),
				handback_pending: true,
				confirmation_key: None,
				pending_send: None,
				input,
			},
		);
		let mut baseline = DesktopDraftDocument::default();
		baseline.profiles.insert(scope.clone(), profile);
		let mut local = baseline.clone();
		local
			.profiles
			.get_mut(&scope)
			.unwrap()
			.prompt_edits
			.get_mut(&review)
			.unwrap()
			.input
			.replace_text(0, 0..8, "Local")
			.unwrap();
		let mut remote = baseline.clone();
		remote
			.profiles
			.get_mut(&scope)
			.unwrap()
			.prompt_edits
			.get_mut(&review)
			.unwrap()
			.input
			.replace_text(0, 0..8, "Remote")
			.unwrap();
		let merged = local.reconcile_keep_both(&baseline, &remote).unwrap();
		let encoded = merged.encode().unwrap();
		assert!(encoded.len() > 12 * 1024 * 1024);
		let directory = tempfile::tempdir().unwrap();
		let root = directory.path().canonicalize().unwrap().join("desktop");
		let store = ClientDraftStore::open_at(&root).unwrap();
		store.save(0, &encoded).unwrap();
		drop(store);
		let reopened = ClientDraftStore::open_at(&root).unwrap();
		let restored = DesktopDraftDocument::decode(&reopened.load().unwrap().payload).unwrap();
		assert!(restored == merged);
		assert_eq!(restored.profiles[&scope].composer.text, "Other unsent input");
		assert_eq!(restored.recovered.len(), 1);
	}

	#[test]
	fn saved_canonical_editor_survives_reopen_and_keeps_an_occupied_composer() {
		use crate::{
			ClientDraftStore, DesktopDraftDocument, DesktopProfileDraft, EntityId, WireText,
		};
		let mut document = DesktopDraftDocument::default();
		let scope = "a".repeat(64);
		let review = "b".repeat(64);
		let mut profile = DesktopProfileDraft::default();
		profile.composer.text = "Existing unsent input".into();
		profile.prompt_edits.insert(
			review.clone(),
			DesktopPromptEditDraft {
				work_id: EntityId::new("work").unwrap(),
				thread_id: WireText::new("thread").unwrap(),
				before_turn_id: WireText::new("turn").unwrap(),
				item_id: WireText::new("item").unwrap(),
				original_hash: sample().fingerprint().unwrap(),
				review_token: WireText::new(&review).unwrap(),
				receipt_id: Some(42),
				handback_pending: true,
				confirmation_key: None,
				pending_send: None,
				input: sample(),
			},
		);
		document.profiles.insert(scope.clone(), profile);
		let directory = tempfile::tempdir().unwrap();
		let root = directory.path().canonicalize().unwrap().join("desktop");
		let store = ClientDraftStore::open_at(&root).unwrap();
		store.save(0, &document.encode().unwrap()).unwrap();
		drop(store);
		let store = ClientDraftStore::open_at(&root).unwrap();
		let restored = DesktopDraftDocument::decode(&store.load().unwrap().payload).unwrap();
		assert!(restored == document);
		let mut local = restored.clone();
		local
			.profiles
			.get_mut(&scope)
			.unwrap()
			.prompt_edits
			.get_mut(&review)
			.unwrap()
			.input
			.replace_text(0, 0..3, "local")
			.unwrap();
		let mut remote = restored.clone();
		remote
			.profiles
			.get_mut(&scope)
			.unwrap()
			.prompt_edits
			.get_mut(&review)
			.unwrap()
			.input
			.replace_text(0, 0..3, "remote")
			.unwrap();
		let merged = local.reconcile_keep_both(&restored, &remote).unwrap();
		assert_eq!(merged.profiles[&scope].composer.text, "Existing unsent input");
		assert_eq!(merged.profiles[&scope].prompt_edits, local.profiles[&scope].prompt_edits);
		assert_eq!(merged.recovered[0].draft.prompt_edits, remote.profiles[&scope].prompt_edits);
		assert!(merged.remove_recovered_copy(&merged.recovered[0]).is_err());
		let selected = merged.restore_recovered_copy(&merged.recovered[0]).unwrap();
		assert_eq!(selected.profiles[&scope].prompt_edits, remote.profiles[&scope].prompt_edits);
		let mut removed = restored.clone();
		removed.profiles.get_mut(&scope).unwrap().prompt_edits.clear();
		let reconciled = removed.reconcile_keep_both(&restored, &remote).unwrap();
		assert_eq!(reconciled.profiles[&scope].prompt_edits, remote.profiles[&scope].prompt_edits);
		let old = DesktopDraftDocument::decode(br#"{"version":7,"profiles":{}}"#).unwrap();
		assert_eq!(old.version, 10);
		assert!(old.profiles.is_empty());
	}
}
