//! Preserve source-bound alternatives when the user elects to keep both drafts.
use super::{DesktopDraftDocument, DesktopProfileDraft};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// A complete alternative editor state. Selecting it never authorizes submission.
#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopRecoveredDraft {
	/// Exact opaque profile namespace; absent only for input before profile selection.
	pub scope: Option<String>,
	/// Original inputs, source bindings, execution choices, and unresolved delivery.
	pub draft: DesktopProfileDraft,
}
impl DesktopRecoveredDraft {
	pub(super) fn validate(&self) -> Result<(), &'static str> {
		self.draft.validate()?;
		if let Some(scope) = &self.scope {
			if scope.len() != 64
				|| !scope.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
			{
				return Err("Recovered draft profile identity is invalid");
			}
		} else if self.draft.composer.work_id.is_some()
			|| self.draft.composer.thread_id.is_some()
			|| !self.draft.parked.is_empty()
			|| !self.draft.execution.is_empty()
			|| !self.draft.questions.is_empty()
			|| self.draft.uncertain
			|| self.draft.pending.is_some()
			|| !self.draft.unconfirmed_commands.is_empty()
			|| !self.draft.prompt_edits.is_empty()
		{
			return Err("Recovered unbound draft cannot own service state");
		}
		if self.scope.is_none() {
			crate::desktop_ordinary_drafts::validate_unbound(&self.draft.ordinary)?;
		}
		Ok(())
	}
}

impl DesktopDraftDocument {
	/// Reconcile an explicit keep-both choice against a freshly read disk document.
	///
	/// Changed local profiles take precedence, with replaced disk states retained
	/// as source-bound alternatives. Unchanged local profiles keep newer disk data.
	/// The caller must publish using the freshly read revision's compare-and-swap,
	/// and reconcile edits made while that write was in flight before updating UI.
	pub fn reconcile_keep_both(
		&self,
		baseline: &Self,
		latest: &Self,
	) -> Result<Self, &'static str> {
		self.validate()?;
		baseline.validate()?;
		latest.validate()?;
		let mut result = latest.clone();
		for saved in &self.recovered {
			if !baseline.recovered.contains(saved) {
				result.retain_alternative(saved.clone());
			}
		}
		let scopes: BTreeSet<_> = self.profiles.keys().chain(baseline.profiles.keys()).collect();
		for scope in scopes {
			let local = self.profiles.get(scope);
			if local == baseline.profiles.get(scope) || local == latest.profiles.get(scope) {
				continue;
			}
			if let Some(remote) = latest.profiles.get(scope) {
				result.retain_alternative(DesktopRecoveredDraft {
					scope: Some(scope.clone()),
					draft: remote.clone(),
				});
			}
			if let Some(local) = local {
				let mut selected = local.clone();
				if let Some(remote) = latest.profiles.get(scope) {
					if displaces_pending_prompt(&selected, remote) {
						result.retain_alternative(DesktopRecoveredDraft {
							scope: Some(scope.clone()),
							draft: selected.clone(),
						});
					}
					retain_fences(&mut selected, remote);
				}
				result.profiles.insert(scope.clone(), selected);
			} else {
				// Deleting an editor must not hide an unresolved remote delivery.
				if !latest
					.profiles
					.get(scope)
					.is_some_and(|remote| remote.has_unconfirmed_delivery())
				{
					result.profiles.remove(scope);
				}
			}
		}
		let before = result.unbound_profile();
		if self.unbound != baseline.unbound && self.unbound != latest.unbound {
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
		if result.unbound_profile() != before && before != DesktopProfileDraft::default() {
			result.retain_alternative(DesktopRecoveredDraft { scope: None, draft: before });
		}
		// Enforce the same aggregate byte limit before returning an intended write.
		result.encode()?;
		Ok(result)
	}

	/// Restore an exact retained copy and keep the displaced editor as an alternative.
	/// This is a local editor operation, never permission to replay a command.
	pub fn restore_recovered_copy(
		&self,
		copy: &DesktopRecoveredDraft,
	) -> Result<Self, &'static str> {
		self.validate()?;
		if !self.recovered.contains(copy) {
			return Err("Recovered draft is no longer available");
		}
		let mut result = self.clone();
		result.recovered.retain(|saved| saved != copy);
		if let Some(scope) = &copy.scope {
			let mut selected = copy.draft.clone();
			if let Some(current) = self.profiles.get(scope) {
				let mut remainder = copy.draft.clone();
				remainder.prompt_edits.clear();
				if !copy.draft.prompt_edits.is_empty()
					&& remainder == DesktopProfileDraft::default()
				{
					selected = current.clone();
					selected.prompt_edits.extend(copy.draft.prompt_edits.clone());
				}
				if displaces_pending_prompt(&selected, current) {
					result.retain_alternative(copy.clone());
				}
				if current != &copy.draft {
					result.retain_alternative(DesktopRecoveredDraft {
						scope: Some(scope.clone()),
						draft: current.clone(),
					});
				}
				retain_fences(&mut selected, current);
			}
			result.profiles.insert(scope.clone(), selected);
		} else {
			let current = self.unbound_profile();
			if current != copy.draft {
				result.retain_alternative(DesktopRecoveredDraft { scope: None, draft: current });
			}
			result.unbound = copy.draft.composer.clone();
			result.unbound_ordinary = copy.draft.ordinary.clone();
		}
		result.encode()?;
		Ok(result)
	}

	/// Remove an explicitly selected alternative after the user confirms removal.
	/// Unresolved delivery records must be reconciled before their copy is removed.
	pub fn remove_recovered_copy(
		&self,
		copy: &DesktopRecoveredDraft,
	) -> Result<Self, &'static str> {
		self.validate()?;
		if !self.recovered.contains(copy) {
			return Err("Recovered draft is no longer available");
		}
		if copy.draft.has_unconfirmed_delivery() {
			return Err("Confirm delivery before removing this copy");
		}
		let mut result = self.clone();
		result.recovered.retain(|saved| saved != copy);
		result.encode()?;
		Ok(result)
	}

	fn unbound_profile(&self) -> DesktopProfileDraft {
		DesktopProfileDraft {
			composer: self.unbound.clone(),
			ordinary: self.unbound_ordinary.clone(),
			..Default::default()
		}
	}

	/// Move service-free ordinary editors to the first selected profile.
	/// Preserve a displaced saved profile as a recoverable copy; never send input.
	pub fn adopt_unbound_ordinary(&self, scope: &str) -> Result<Self, &'static str> {
		self.validate()?;
		let mut result = self.clone();
		if !self.unbound_ordinary.is_empty() {
			if let Some(saved) = self.profiles.get(scope)
				&& self.unbound_ordinary.iter().any(|(directory, draft)| {
					saved.ordinary.get(directory).is_some_and(|old| old != draft)
				}) {
				result.retain_alternative(DesktopRecoveredDraft {
					scope: Some(scope.into()),
					draft: saved.clone(),
				});
			}
			let selected = result.profiles.entry(scope.into()).or_default();
			selected.ordinary.extend(std::mem::take(&mut result.unbound_ordinary));
			if let Some(saved) = self.profiles.get(scope) {
				retain_fences(selected, saved);
			}
		}
		result.encode()?;
		Ok(result)
	}

	fn retain_alternative(&mut self, saved: DesktopRecoveredDraft) {
		if !self.recovered.contains(&saved) {
			self.recovered.push(saved);
		}
	}
}

fn displaces_pending_prompt(selected: &DesktopProfileDraft, other: &DesktopProfileDraft) -> bool {
	other.prompt_edits.iter().any(|(review, draft)| {
		draft.pending_send.is_some()
			&& selected.prompt_edits.get(review).is_some_and(|local| local != draft)
	})
}

fn retain_fences(selected: &mut DesktopProfileDraft, other: &DesktopProfileDraft) {
	for (review, draft) in &other.prompt_edits {
		if draft.pending_send.is_some() {
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
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{DesktopComposerDraft, EntityId};

	fn profile(text: &str) -> DesktopProfileDraft {
		DesktopProfileDraft {
			composer: DesktopComposerDraft { text: text.into(), ..Default::default() },
			..Default::default()
		}
	}

	fn prompt(text: &str) -> crate::DesktopPromptEditDraft {
		let input =
			crate::PromptDraft::new(vec![serde_json::json!({"type":"text","text":text})]).unwrap();
		crate::DesktopPromptEditDraft {
			work_id: EntityId::new("work").unwrap(),
			thread_id: crate::WireText::new("thread").unwrap(),
			before_turn_id: crate::WireText::new("turn").unwrap(),
			item_id: crate::WireText::new("item").unwrap(),
			original_hash: input.fingerprint().unwrap(),
			review_token: crate::WireText::new("b".repeat(64)).unwrap(),
			receipt_id: None,
			confirmation_key: None,
			pending_send: None,
			handback_pending: false,
			input,
		}
	}

	#[test]
	fn conflicting_prompt_operations_keep_exact_bindings_and_both_inputs() {
		let scope = "a".repeat(64);
		let review = "b".repeat(64);
		let original = prompt("original input");
		let confirming = original
			.begin_confirmation(crate::IdempotencyKey::new("confirm-original").unwrap())
			.unwrap();
		let mut restored = original.clone();
		restored.receipt_id = Some(42);
		let sending = restored
			.begin_send(7, crate::IdempotencyKey::new("send-original").unwrap(), Default::default())
			.unwrap();
		let mut handback = restored.clone();
		handback.handback_pending = true;
		for remote in [confirming, handback, sending] {
			let mut base = DesktopDraftDocument::default();
			base.profiles.insert(scope.clone(), profile("occupied composer"));
			base.profiles
				.get_mut(&scope)
				.unwrap()
				.prompt_edits
				.insert(review.clone(), original.clone());
			let mut local = base.clone();
			let mut edited = original.clone();
			edited.input.replace_text(0, 0..0, "local edit: ").unwrap();
			if remote.receipt_id.is_some() && remote.pending_send.is_none() {
				edited = edited
					.begin_confirmation(crate::IdempotencyKey::new("confirm-local").unwrap())
					.unwrap();
			}
			local
				.profiles
				.get_mut(&scope)
				.unwrap()
				.prompt_edits
				.insert(review.clone(), edited.clone());
			let mut disk = base.clone();
			disk.profiles
				.get_mut(&scope)
				.unwrap()
				.prompt_edits
				.insert(review.clone(), remote.clone());
			let merged = local.reconcile_keep_both(&base, &disk).unwrap();
			let active = &merged.profiles[&scope].prompt_edits[&review];
			if remote.pending_send.is_none() {
				assert_eq!(active.input, edited.input);
				assert_eq!(active.receipt_id, remote.receipt_id);
				assert_eq!(active.confirmation_key, remote.confirmation_key);
				assert!(active.handback_pending);
				continue;
			}
			assert_eq!(active, &remote);
			assert!(
				merged
					.recovered
					.iter()
					.any(|copy| copy.draft.prompt_edits.get(&review) == Some(&edited))
			);
			let copy = merged
				.recovered
				.iter()
				.find(|copy| copy.draft.prompt_edits.get(&review) == Some(&edited))
				.unwrap();
			let recovered = merged.restore_recovered_copy(copy).unwrap();
			assert_eq!(recovered.profiles[&scope].prompt_edits[&review], remote);
			assert!(recovered.recovered.contains(copy));
			assert_eq!(recovered.profiles[&scope].composer.text, "occupied composer");
		}
	}

	#[test]
	fn removal_requires_exact_copy_and_preserves_unknown_delivery() {
		let mut doc = DesktopDraftDocument::default();
		let copy =
			DesktopRecoveredDraft { scope: Some("a".repeat(64)), draft: profile("removable") };
		doc.recovered.push(copy.clone());
		let mut pending = copy.clone();
		pending.draft.uncertain = true;
		pending
			.draft
			.unconfirmed_commands
			.push(crate::IdempotencyKey::new("unknown-command").unwrap());
		doc.recovered.push(pending.clone());
		assert!(doc.remove_recovered_copy(&pending).is_err());
		let removed = doc.remove_recovered_copy(&copy).unwrap();
		assert_eq!(removed.recovered.len(), 1);
		assert!(removed.recovered[0] == pending);
		assert!(removed.remove_recovered_copy(&copy).is_err());
	}

	#[test]
	fn restore_copy_preserves_displaced_input_and_delivery_fences() {
		let scope = "a".repeat(64);
		let mut doc = DesktopDraftDocument::default();
		let mut current = profile("current draft");
		current.uncertain = true;
		current.unconfirmed_commands.push(crate::IdempotencyKey::new("original-command").unwrap());
		doc.profiles.insert(scope.clone(), current.clone());
		let copy =
			DesktopRecoveredDraft { scope: Some(scope.clone()), draft: profile("older draft") };
		doc.recovered.push(copy.clone());
		let restored = doc.restore_recovered_copy(&copy).unwrap();
		assert_eq!(restored.profiles[&scope].composer.text, "older draft");
		assert!(restored.profiles[&scope].uncertain);
		assert_eq!(restored.profiles[&scope].unconfirmed_commands, current.unconfirmed_commands);
		assert!(restored.recovered.iter().any(|saved| saved.draft == current));
		assert!(!restored.recovered.contains(&copy));
		let reconciled = doc.reconcile_keep_both(&doc, &restored).unwrap();
		assert!(
			!reconciled.recovered.contains(&copy),
			"unchanged local history cannot undo explicit copy restoration"
		);
		assert!(restored.restore_recovered_copy(&copy).is_err());
	}

	#[test]
	fn keep_both_preserves_conflicting_sources_and_unrelated_remote_edits() {
		let scope = "a".repeat(64);
		let other = "b".repeat(64);
		let mut base = DesktopDraftDocument::default();
		base.profiles.insert(scope.clone(), profile("base"));
		let mut local = base.clone();
		local.profiles.insert(scope.clone(), profile("local"));
		let mut disk = base.clone();
		let mut remote = profile("remote");
		remote.uncertain = true;
		remote
			.unconfirmed_commands
			.push(crate::IdempotencyKey::new("unknown-remote-command").unwrap());
		disk.profiles.insert(scope.clone(), remote.clone());
		disk.profiles.insert(other.clone(), profile("unrelated remote"));
		let merged = local.reconcile_keep_both(&base, &disk).unwrap();
		assert_eq!(merged.profiles[&scope].composer.text, "local");
		assert_eq!(merged.profiles[&other].composer.text, "unrelated remote");
		assert!(merged.profiles[&scope].uncertain);
		assert_eq!(merged.profiles[&scope].unconfirmed_commands, remote.unconfirmed_commands);
		assert_eq!(merged.recovered.len(), 1);
		assert_eq!(merged.recovered[0].scope.as_ref(), Some(&scope));
		assert!(merged.recovered[0].draft == remote);
		let unchanged = base.reconcile_keep_both(&base, &disk).unwrap();
		assert!(unchanged == disk);
		let directory = tempfile::tempdir().unwrap();
		let store = crate::ClientDraftStore::open_at(
			&directory.path().canonicalize().unwrap().join("drafts"),
		)
		.unwrap();
		let revision = store.save(0, &disk.encode().unwrap()).unwrap();
		store.save(revision, &merged.encode().unwrap()).unwrap();
		let reopened = DesktopDraftDocument::decode(&store.load().unwrap().payload).unwrap();
		assert!(reopened == merged);
		assert!(matches!(
			store.save(revision, &merged.encode().unwrap()),
			Err(crate::ClientDraftError::Conflict)
		));
	}

	#[test]
	fn keep_both_retains_seeded_and_unbound_alternatives_without_source_invention() {
		let scope = "a".repeat(64);
		let mut base = DesktopDraftDocument::default();
		base.profiles.insert(scope.clone(), profile("saved profile"));
		base.unbound.text = "older unbound".into();
		let mut local = base.clone();
		local.profiles.insert(scope.clone(), profile("seeded input"));
		local.unbound.text = "new unbound".into();
		let merged = local.reconcile_keep_both(&base, &base).unwrap();
		assert_eq!(merged.profiles[&scope].composer.text, "seeded input");
		assert_eq!(merged.unbound.text, "new unbound");
		assert_eq!(merged.recovered.len(), 2);
		assert!(
			merged
				.recovered
				.iter()
				.any(|saved| saved.scope.is_none() && saved.draft.composer.text == "older unbound")
		);
		let again = local.reconcile_keep_both(&base, &merged).unwrap();
		assert_eq!(again.recovered.len(), 2);
		let mut invalid = merged;
		invalid.recovered[1].draft.composer.work_id = Some(EntityId::new("foreign-work").unwrap());
		assert!(invalid.encode().is_err());
	}

	#[test]
	fn keep_both_refuses_to_drop_old_copies_when_recovery_capacity_is_full() {
		let scope = "a".repeat(64);
		let mut base = DesktopDraftDocument::default();
		base.profiles.insert(scope.clone(), profile("base"));
		let mut local = base.clone();
		local.profiles.insert(scope, profile("changed"));
		for index in 0..32 {
			base.recovered.push(DesktopRecoveredDraft {
				scope: None,
				draft: profile(&format!("copy-{index}")),
			});
		}
		assert_eq!(
			local.reconcile_keep_both(&base, &base).err(),
			Some("Too many recovered draft copies")
		);
		assert_eq!(base.recovered.len(), 32);
	}
}
