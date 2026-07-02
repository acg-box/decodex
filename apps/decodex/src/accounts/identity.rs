use crate::accounts::AccountIdentitySummary;

#[derive(Clone)]
pub(in crate::accounts) struct AccountIdentity {
	pub(in crate::accounts) account_id: Option<String>,
	pub(in crate::accounts) email: Option<String>,
}
impl AccountIdentity {
	pub(in crate::accounts) fn summary(&self) -> AccountIdentitySummary {
		let account_fingerprint = self
			.account_id
			.as_deref()
			.map(redact_account_id)
			.or_else(|| self.email.clone())
			.unwrap_or_else(|| String::from("unknown"));
		let selector = self.email.clone().unwrap_or_else(|| account_fingerprint.clone());

		AccountIdentitySummary { account_fingerprint, email: self.email.clone(), selector }
	}
}

pub(in crate::accounts) fn redact_account_id(account_id: &str) -> String {
	let tail =
		account_id.chars().rev().take(6).collect::<Vec<_>>().into_iter().rev().collect::<String>();

	if tail.is_empty() { String::from("unknown") } else { format!("...{tail}") }
}
