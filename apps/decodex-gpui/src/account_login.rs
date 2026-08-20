//! Presentation-neutral GPUI seam for daemon-owned account login.

use decodex_protocol::{
	AccountLoginClient, AccountLoginStart, AccountLoginStatus, ClientFailure, ClientProfile, EntityId,
};

/// Short-lived protocol controller shared by any future account-login presentation.
pub(crate) struct AccountLoginController {
	client: AccountLoginClient,
}

impl AccountLoginController {
	/// Bind login exchanges to one already verified local client profile.
	pub(crate) const fn new(profile: ClientProfile) -> Self {
		Self { client: AccountLoginClient::new(profile) }
	}

	/// Start or idempotently read one exact daemon-owned login session.
	pub(crate) async fn start(
		&self,
		start: AccountLoginStart,
	) -> Result<AccountLoginStatus, ClientFailure> {
		self.client.start(start).await
	}

	/// Read one in-memory session without retaining its transient status.
	pub(crate) async fn status(
		&self,
		session_id: EntityId,
	) -> Result<AccountLoginStatus, ClientFailure> {
		self.client.status(session_id).await
	}

	/// Cancel one session and return only after daemon-owned cleanup is terminal.
	pub(crate) async fn cancel(
		&self,
		session_id: EntityId,
	) -> Result<AccountLoginStatus, ClientFailure> {
		self.client.cancel(session_id).await
	}
}
