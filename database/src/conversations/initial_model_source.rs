//! Persisted account observation for initial model choices.

use decodex_core::{AccountId, ConversationId, ServiceTier};
use rusqlite::{Row, TransactionBehavior, params};

use super::{
	digest, read_runtime_receipt, validate_initial_execution, validate_key, write_runtime_receipt,
};
use crate::{SqliteStore, StoreError, account_lifecycle::sql_error, unix_micros};

/// Account whose catalog and native defaults supplied the original request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InitialModelSource {
	/// Stable local account identity.
	pub account_id: AccountId,
	/// Positive revision observed after discovery completed.
	pub account_revision: i64,
}

pub(super) fn from_row(
	row: &Row<'_>,
	offset: usize,
) -> rusqlite::Result<Option<InitialModelSource>> {
	let account_id: Option<String> = row.get(offset)?;
	let revision: Option<i64> = row.get(offset + 1)?;
	match (account_id, revision) {
		(None, None) => Ok(None),
		(Some(id), Some(revision)) if revision > 0 => {
			let account_id = AccountId::new(id).map_err(|error| {
				rusqlite::Error::FromSqlConversionFailure(
					offset,
					rusqlite::types::Type::Text,
					Box::new(error),
				)
			})?;
			Ok(Some(InitialModelSource { account_id, account_revision: revision }))
		},
		_ => Err(rusqlite::Error::InvalidQuery),
	}
}

/// Explicit replacement of choices for a blocked, unstarted conversation.
#[derive(Clone, Debug)]
pub struct ReviewInitialModelSettings {
	/// Conversation whose saved input and directory must remain unchanged.
	pub conversation_id: ConversationId,
	/// Revision displayed when the user confirmed the replacement settings.
	pub expected_revision: i64,
	/// Reviewed native model identifier.
	pub model: String,
	/// Reviewed native reasoning effort.
	pub reasoning_effort: Option<String>,
	/// Legacy compatibility setting; the explicit service tier remains authoritative.
	pub fast: bool,
	/// Reviewed service tier.
	pub service_tier: Option<ServiceTier>,
	/// Fresh account observation that supplied these choices.
	pub source: InitialModelSource,
}

/// Durable confirmation result without permission to spawn or dispatch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InitialModelReviewOutcome {
	/// The settings were committed once; replay returns the same revision.
	Applied { revision: i64, replayed: bool },
	/// The task changed or already has route, session, or turn authority.
	Rejected,
}

impl SqliteStore {
	/// Confirm initial choices without changing the saved prompt or working directory.
	pub async fn review_initial_model_settings(
		&self,
		idempotency_key: &str,
		request: &ReviewInitialModelSettings,
	) -> Result<InitialModelReviewOutcome, StoreError> {
		validate_key(idempotency_key)?;
		validate_initial_execution(&request.model, request.reasoning_effort.as_deref())?;
		if request.expected_revision <= 0 || request.source.account_revision <= 0 {
			return Err(StoreError::InvalidInput("model review revisions must be positive"));
		}
		let key = idempotency_key.to_owned();
		let request = request.clone();
		self.run(move |connection| {
            let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(sql_error)?;
            let identity = digest(&[
                request.conversation_id.as_str(), &request.expected_revision.to_string(),
                &request.model, request.reasoning_effort.as_deref().unwrap_or(""), &request.fast.to_string(),
                request.service_tier.as_ref().map_or("", ServiceTier::as_str), request.source.account_id.as_str(),
                &request.source.account_revision.to_string(),
            ]);
            if let Some(receipt) = read_runtime_receipt(&transaction, &key, &identity,
                "review_initial_model_settings", request.conversation_id.as_str())? {
                let revision = receipt.parse::<i64>().map_err(|_| StoreError::Incompatible("model review receipt".to_owned()))?;
                transaction.commit().map_err(sql_error)?;
                return Ok(InitialModelReviewOutcome::Applied { revision, replayed: true });
            }
            let eligible: bool = transaction.query_row(
                "SELECT EXISTS (
                   SELECT 1 FROM conversations c JOIN quick_task_requests q USING (conversation_id)
                   WHERE c.conversation_id = ?1 AND c.revision = ?2 AND c.state = 'active'
                     AND q.model_source_review_required = 1
                     AND NOT EXISTS (SELECT 1 FROM routing_decisions d WHERE d.conversation_id = c.conversation_id)
                     AND NOT EXISTS (SELECT 1 FROM runtime_sessions s WHERE s.conversation_id = c.conversation_id)
                     AND NOT EXISTS (SELECT 1 FROM turns t WHERE t.conversation_id = c.conversation_id)
                 )",
                params![request.conversation_id.as_str(), request.expected_revision], |row| row.get(0),
            ).map_err(sql_error)?;
            if !eligible { return Ok(InitialModelReviewOutcome::Rejected); }
            let revision = request.expected_revision.checked_add(1)
                .ok_or(StoreError::InvalidInput("model review revision overflow"))?;
            let now = unix_micros().map_err(StoreError::from)?;
            transaction.execute(
                "UPDATE quick_task_requests SET model = ?2, reasoning_effort = ?3, fast = ?4,
                   service_tier = ?5, model_source_account_id = ?6, model_source_account_revision = ?7,
                   model_source_review_required = 0 WHERE conversation_id = ?1",
                params![request.conversation_id.as_str(), request.model, request.reasoning_effort,
                    request.fast, request.service_tier.as_ref().map(ServiceTier::as_str), request.source.account_id.as_str(),
                    request.source.account_revision],
            ).map_err(sql_error)?;
            transaction.execute(
                "UPDATE conversations SET revision = ?2, updated_at_micros = MAX(updated_at_micros + 1, ?3) WHERE conversation_id = ?1",
                params![request.conversation_id.as_str(), revision, now],
            ).map_err(sql_error)?;
            write_runtime_receipt(&transaction, &key, &identity, "review_initial_model_settings",
                request.conversation_id.as_str(), &revision.to_string(), now)?;
            transaction.commit().map_err(sql_error)?;
            Ok(InitialModelReviewOutcome::Applied { revision, replayed: false })
        }).await
	}
}
