//! Ordinary records share Chief's existing atomic desktop writer.
use super::{ChiefSurface, Context};
use decodex_protocol::{CommandEnvelope, DesktopOrdinaryDraft};

impl ChiefSurface {
	pub(in super::super) fn adopt_unbound_ordinary(&mut self, scope: &str) {
		let storage = &mut self.draft_profiles.storage;
		match storage.document.adopt_unbound_ordinary(scope) {
			Ok(document) => storage.document = document,
			Err(reason) => storage.error = Some(reason.into()),
		}
	}

	pub(crate) fn ordinary_draft_notice(&self) -> Option<&str> {
		self.draft_storage_notice()
	}

	pub(crate) fn show_ordinary_draft_recovery(&mut self, cx: &mut Context<Self>) {
		if !self.show_recovered_drafts() {
			self.toggle_recovered_drafts(cx);
		}
	}

	pub(crate) fn defer_ordinary_restore(&mut self) {
		let Some(profile) = self.draft_profiles.active.as_ref() else { return };
		let scope = profile.draft_scope_key();
		let storage = &mut self.draft_profiles.storage;
		let Some(draft) = storage.document.profiles.get(&scope).cloned() else { return };
		let copy = decodex_protocol::DesktopRecoveredDraft { scope: Some(scope), draft };
		if !storage.document.recovered.contains(&copy) {
			storage.document.recovered.push(copy);
		}
		storage.show_recovered = true;
	}

	pub(crate) fn ordinary_storage_record(
		&self,
		directory: &str,
		saved: bool,
	) -> Option<DesktopOrdinaryDraft> {
		let scope = self.draft_profiles.active.as_ref().map(|profile| profile.draft_scope_key());
		let storage = &self.draft_profiles.storage;
		let document = if saved { &storage.saved } else { &storage.document };
		match scope {
			Some(scope) => document.profiles.get(&scope)?.ordinary.get(directory).cloned(),
			None => document.unbound_ordinary.get(directory).cloned(),
		}
	}

	pub(crate) fn save_ordinary_storage(
		&mut self,
		draft: DesktopOrdinaryDraft,
		confirmed: &[CommandEnvelope],
		cx: &mut Context<Self>,
	) {
		let Some(profile) = self.draft_profiles.active.as_ref() else {
			self.draft_profiles
				.storage
				.document
				.unbound_ordinary
				.insert(draft.working_directory.as_str().into(), draft);
			self.save_draft_document(cx);
			return;
		};
		let scope = profile.draft_scope_key();
		let document = &mut self.draft_profiles.storage.document;
		document
			.profiles
			.entry(scope.clone())
			.or_default()
			.ordinary
			.insert(draft.working_directory.as_str().into(), draft);
		for profile in document.profiles.get_mut(&scope).into_iter().chain(
			document
				.recovered
				.iter_mut()
				.filter(|copy| copy.scope.as_ref() == Some(&scope))
				.map(|copy| &mut copy.draft),
		) {
			for ordinary in profile.ordinary.values_mut() {
				ordinary.unconfirmed.retain(|command| !confirmed.contains(command));
			}
		}
		self.save_draft_document(cx);
	}
}
