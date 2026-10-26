//! Merge current editor changes without keeping alternative profile snapshots.
use crate::{DesktopDraftDocument, DesktopProfileDraft};
use std::{collections::BTreeSet, mem};

impl DesktopDraftDocument {
	/// Keep the current edit where this writer changed it; adopt unrelated newer profiles.
	/// Preserve every unresolved execution receipt. This operation never sends a message.
	pub fn merge_edits(&self, baseline: &Self, latest: &Self) -> Result<Self, &'static str> {
		self.validate()?;
		baseline.validate()?;
		latest.validate()?;
		let mut result = latest.clone();
		let scopes: BTreeSet<_> = self.profiles.keys().chain(baseline.profiles.keys()).collect();
		for scope in scopes {
			let local = self.profiles.get(scope);
			if local == baseline.profiles.get(scope) || local == latest.profiles.get(scope) {
				continue;
			}
			if let Some(local) = local {
				let mut selected = local.clone();
				if let Some(remote) = latest.profiles.get(scope) {
					retain_receipts(&mut selected, remote)?;
				}
				result.profiles.insert(scope.clone(), selected);
			} else if !latest
				.profiles
				.get(scope)
				.is_some_and(DesktopProfileDraft::has_unconfirmed_delivery)
			{
				result.profiles.remove(scope);
			}
		}
		if self.unbound != baseline.unbound {
			result.unbound = self.unbound.clone();
		}
		let directories: BTreeSet<_> =
			self.unbound_ordinary.keys().chain(baseline.unbound_ordinary.keys()).collect();
		for directory in directories {
			let local = self.unbound_ordinary.get(directory);
			if local == baseline.unbound_ordinary.get(directory) {
				continue;
			}
			if let Some(local) = local {
				result.unbound_ordinary.insert(directory.clone(), local.clone());
			} else {
				result.unbound_ordinary.remove(directory);
			}
		}
		result.encode()?;
		Ok(result)
	}

	/// Move service-free editors to the selected profile and retain unresolved receipts.
	pub fn adopt_unbound_ordinary(&self, scope: &str) -> Result<Self, &'static str> {
		self.validate()?;
		let mut result = self.clone();
		if !self.unbound_ordinary.is_empty() {
			let selected = result.profiles.entry(scope.into()).or_default();
			selected.ordinary.extend(mem::take(&mut result.unbound_ordinary));
			if let Some(saved) = self.profiles.get(scope) {
				retain_receipts(selected, saved)?;
			}
		}
		result.encode()?;
		Ok(result)
	}
}

fn retain_receipts(
	selected: &mut DesktopProfileDraft,
	other: &DesktopProfileDraft,
) -> Result<(), &'static str> {
	for (review, draft) in &other.prompt_edits {
		if draft.pending_send.is_some() || draft.fork.is_some() {
			if selected.prompt_edits.get(review).is_some_and(|local| local != draft) {
				return Err("A history edit is still being sent in another window. Keep this editor open until its status is resolved.");
			}
			selected.prompt_edits.insert(review.clone(), draft.clone());

			continue;
		}
		if draft.handback_pending {
			let local =
				selected.prompt_edits.entry(review.clone()).or_insert_with(|| draft.clone());

			if local.work_id == draft.work_id && local.thread_id == draft.thread_id {
				if local.pending_send.is_some() {
					continue;
				}

				local.receipt_id = local.receipt_id.or(draft.receipt_id);
				local.handback_pending = true;
				local.confirmation_key = if local.receipt_id.is_some() {
					None
				} else {
					local.confirmation_key.clone().or_else(|| draft.confirmation_key.clone())
				};
			} else {
				selected.uncertain = true;
			}
		}
	}
	for (directory, remote) in &other.ordinary {
		if remote.unconfirmed.is_empty() {
			continue;
		}

		let local = selected.ordinary.entry(directory.clone()).or_insert_with(|| remote.clone());

		for command in &remote.unconfirmed {
			if !local.unconfirmed.contains(command) {
				local.unconfirmed.push(command.clone());
			}
		}
	}

	for (key, message) in &other.messages {
		selected.messages.entry(key.clone()).or_insert_with(|| message.clone());
	}
	selected.uncertain |= other.uncertain;

	for key in other.unconfirmed_commands.iter().chain(
		other
			.pending
			.as_ref()
			.and_then(|pending| pending.steer.as_ref())
			.map(|steer| &steer.submission_id),
	) {
		if !selected.unconfirmed_commands.contains(key) {
			selected.unconfirmed_commands.push(key.clone());
		}
	}
	Ok(())
}

#[cfg(test)]
mod tests {
	use crate::{
		AgentActionDto, DesktopDraftDocument, DesktopMessage, DesktopMessageState,
		DesktopProfileDraft, EntityId, HistoryText, IdempotencyKey,
	};

	#[test]
	fn current_edits_merge_other_profiles_and_exact_unconfirmed_messages_without_snapshots() {
		let scope = "a".repeat(64);
		let other = "b".repeat(64);
		let mut baseline = DesktopDraftDocument::default();
		baseline.profiles.insert(scope.clone(), DesktopProfileDraft::default());
		let mut local = baseline.clone();
		local.profiles.get_mut(&scope).unwrap().composer.text = "Current input".into();
		let mut latest = baseline.clone();
		let profile = latest.profiles.get_mut(&scope).unwrap();
		profile.composer.text = "Older competing input".into();
		let key = IdempotencyKey::new("unconfirmed-input").unwrap();
		let message = DesktopMessage {
			action: AgentActionDto::Send {
				root_id: EntityId::new("agent").unwrap(),
				text: HistoryText::new("Already submitted").unwrap(),
			},
			owner: Some(EntityId::new("agent").unwrap()),
			thread_id: None,
			text: "Already submitted".into(),
			state: DesktopMessageState::Unconfirmed,
			error: None,
		};
		profile.messages.insert(key.clone(), message.clone());
		profile.uncertain = true;
		profile.unconfirmed_commands.push(key.clone());
		let mut foreign = DesktopProfileDraft::default();
		foreign.composer.text = "Another profile".into();
		latest.profiles.insert(other.clone(), foreign.clone());
		let merged = local.merge_edits(&baseline, &latest).unwrap();
		assert_eq!(merged.profiles[&scope].composer.text, "Current input");
		assert_eq!(merged.profiles[&scope].messages[&key], message);
		assert_eq!(merged.profiles[&scope].unconfirmed_commands, vec![key]);
		assert!(merged.profiles[&other] == foreign);
		assert!(!String::from_utf8(merged.encode().unwrap()).unwrap().contains("recovered"));
	}

	#[test]
	fn a_competing_pending_prompt_does_not_replace_the_current_editor() {
		use crate::{DesktopPromptEditDraft, PromptDraft, WireText};
		let scope = "a".repeat(64);
		let review = "b".repeat(64);
		let input = PromptDraft::new(vec![serde_json::json!({"type":"text","text":"Original"})]).unwrap();
		let draft = DesktopPromptEditDraft {
			work_id: EntityId::new("work").unwrap(),
			thread_id: WireText::new("thread").unwrap(),
			before_turn_id: WireText::new("turn").unwrap(),
			item_id: WireText::new("item").unwrap(),
			original_hash: input.fingerprint().unwrap(),
			review_token: WireText::new(&review).unwrap(),
			receipt_id: Some(42), confirmation_key: None, fork: None,
			pending_send: None, handback_pending: false, input,
		};
		let mut baseline = DesktopDraftDocument::default();
		baseline.profiles.entry(scope.clone()).or_default().prompt_edits.insert(review.clone(), draft.clone());
		let mut local = baseline.clone();
		local.profiles.get_mut(&scope).unwrap().prompt_edits.get_mut(&review).unwrap().input.replace_text(0, 0..8, "Current edit").unwrap();
		let mut remote = baseline.clone();
		remote.profiles.get_mut(&scope).unwrap().prompt_edits.insert(review.clone(), draft.begin_send(7, IdempotencyKey::new("send").unwrap(), Default::default()).unwrap());
		assert!(local.merge_edits(&baseline, &remote).is_err());
		assert_eq!(local.profiles[&scope].prompt_edits[&review].input.parts()[0]["text"], "Current edit");
	}
}
