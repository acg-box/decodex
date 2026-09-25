//! Finish a local input only with its exact positive non-submission receipt.
use crate::{StoreError, account_lifecycle::sql_error};
use decodex_core::{
	ProviderAttempt, ProviderAttemptConsumer, ProviderEvidenceSource, ProviderPositiveEvidence,
	ProviderTerminalOutcome,
};
use rusqlite::{Transaction, params};

pub(crate) fn finalize(
	connection: &Transaction<'_>,
	attempt: &ProviderAttempt,
	evidence: &ProviderPositiveEvidence,
	now: i64,
) -> Result<(), StoreError> {
	if evidence.outcome != ProviderTerminalOutcome::NotSubmitted
		|| evidence.source != ProviderEvidenceSource::PositiveNonSubmissionReceipt
	{
		return Ok(());
	}
	let ProviderAttemptConsumer::ConversationTurn { conversation_id, turn_id } = &attempt.consumer
	else {
		return Ok(());
	};
	let Some(thread) = &evidence.provider_thread_id else {
		return Ok(());
	};
	let changed = connection
		.execute(
			"UPDATE turns SET status='failed', revision=revision+1,
		 updated_at_micros=max(updated_at_micros,?5), completed_at_micros=?5
		 WHERE turn_id=?1 AND conversation_id=?2 AND runtime_session_id=?3
		   AND role='user' AND status='active'
		   AND EXISTS(SELECT 1 FROM runtime_sessions s WHERE s.runtime_session_id=?3
		              AND s.conversation_id=?2 AND s.codex_thread_id=?4 AND s.state='active')
		   AND NOT EXISTS(SELECT 1 FROM history_items h WHERE h.turn_id=?1 AND h.status='streaming')
		   AND NOT EXISTS(SELECT 1 FROM provider_attempts p WHERE p.turn_id=?1
		                  AND p.attempt_id<>?6 AND p.state NOT IN ('canceled','not_submitted'))",
			params![
				turn_id.as_str(),
				conversation_id.as_str(),
				attempt.runtime_session_id.as_str(),
				thread,
				now,
				attempt.attempt_id.as_str()
			],
		)
		.map_err(sql_error)?;
	if changed == 0 {
		return Err(super::incompatible("non-submission does not match the active local input"));
	}
	let metadata = serde_json::json!({
		"type": "native_turn_not_submitted",
		"evidenceId": evidence.evidence_id.as_str(),
		"response_sha256": evidence.witness_digest,
	})
	.to_string();
	connection.execute(
		"INSERT INTO history_items(history_item_id,conversation_id,turn_id,sequence,kind,
		 role,status,media_type,inline_text,metadata_json,revision,created_at_micros,updated_at_micros)
		 VALUES(?1,?2,?3,(SELECT coalesce(max(sequence),0)+1 FROM history_items WHERE conversation_id=?2),
		 'status','user','failed','text/plain',?4,?5,1,?6,?6)",
		params![evidence.evidence_id.as_str(),conversation_id.as_str(),turn_id.as_str(),
		        "Codex confirmed that this input was not submitted. Reconnect and send again. Your input remains in history.",
		        metadata,now],
	).map_err(sql_error)?;
	super::touch_conversation(connection, conversation_id, now)
}
