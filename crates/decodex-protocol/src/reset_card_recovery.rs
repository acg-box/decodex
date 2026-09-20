//! Public daemon-owned reset-card recovery. Provider credit IDs never cross this boundary.
use crate::{
	EntityId, EntityRevision, IdempotencyKey, ResetCardDescriptorDto, ResetCardError,
	ResetCardOperationResult,
};
use serde::{Deserialize, Serialize};

/// Latest reset-card operation for one account, including terminal results.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", content = "operation", rename_all = "snake_case")]
pub enum AccountResetCardOperationResult {
	/// This account has no durable reset-card intent.
	NotFound,
	/// The service retained the operation across client or daemon restarts.
	Found(ResetCardOperationView),
	/// Recovery could not be read; this does not permit a new consume attempt.
	Unavailable {
		/// Value-free service error.
		error: ResetCardError,
	},
}
/// Credential-free projection of one explicitly confirmed selection.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ResetCardOperationView {
	/// Selected account.
	pub account_id: EntityId,
	/// Revision confirmed by the user.
	pub account_revision: EntityRevision,
	/// Original logical request; never replaced for a retry.
	pub idempotency_key: IdempotencyKey,
	/// Selected public card dates.
	pub descriptor: ResetCardDescriptorDto,
	/// Current durable result.
	pub state: ResetCardOperationResult,
}
