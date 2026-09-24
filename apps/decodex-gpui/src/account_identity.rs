//! Explicit, in-memory email reveal for the account list.
use super::*;
use decodex_protocol::{AccountClient, AccountProfileEmailDto, EntityRevision};

#[derive(Default)]
pub(super) struct Emails {
	pub(super) visible: bool,
	epoch: u64,
	values: std::collections::HashMap<EntityId, (EntityRevision, String)>,
	task: Option<gpui::Task<()>>,
}
impl Emails {
	pub(super) fn get(&self, account: &AccountDto) -> Option<String> {
		self.visible
			.then(|| self.values.get(&account.account_id))
			.flatten()
			.filter(|(revision, _)| *revision == account.account_revision)
			.map(|(_, email)| email.clone())
	}

	fn toggle(&mut self) {
		self.visible = !self.visible;
		self.epoch = self.epoch.wrapping_add(1);
		self.values.clear();
		self.task = None;
	}
}
impl Shell {
	pub(super) fn toggle_account_emails(&mut self, cx: &mut Context<Self>) {
		self.account_emails.toggle();
		cx.notify();
		if !self.account_emails.visible {
			return;
		}
		let Some(profile) = self.reset_cards.profile.clone() else {
			return;
		};
		let epoch = self.account_emails.epoch;
		let accounts: Vec<_> = self
			.accounts
			.accounts
			.iter()
			.map(|a| (a.account_id.clone(), a.account_revision))
			.collect();
		self.account_emails.task = Some(cx.spawn(async move |shell, cx| {
			for (id, revision) in accounts {
				let profile = profile.clone();
				let query_id = id.clone();
				let response = cx
					.background_executor()
					.spawn(async move {
						let runtime = tokio::runtime::Builder::new_current_thread()
							.enable_all()
							.build()
							.ok()?;
						runtime.block_on(AccountClient::new(profile).profile(query_id, true)).ok()
					})
					.await;
				if shell
					.update(cx, |s, cx| {
						if !s.account_emails.visible || s.account_emails.epoch != epoch {
							return false;
						}
						let email = match response {
							Some(
								AccountProfileResult::Current(p)
								| AccountProfileResult::Cached { profile: p, .. },
							) => Some(p.email),
							Some(AccountProfileResult::Unavailable { email, .. }) => Some(email),
							None => None,
						};
						if let Some(AccountProfileEmailDto::Visible(email)) = email {
							s.account_emails.values.insert(id, (revision, email.as_str().into()));
							cx.notify();
						}
						true
					})
					.ok() != Some(true)
				{
					break;
				}
			}
		}));
	}
}
#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn hide_discards_revealed_addresses_and_invalidates_pending_reads() {
		let mut state = Emails::default();
		state.toggle();
		let epoch = state.epoch;
		state.values.insert(
			EntityId::new("account").unwrap(),
			(EntityRevision(1), "fixture@example.invalid".into()),
		);
		state.toggle();
		assert!(!state.visible);
		assert!(state.values.is_empty());
		assert_ne!(state.epoch, epoch);
	}
}
