//! Preserve an exact native pre-dispatch refusal through the existing provider evidence owner.
use super::{
	ConversationAmbiguity, ConversationLocalState, ConversationManualRecovery, ConversationOutcome,
	ConversationRuntime, LocalSession, ProviderAttemptReconciliation, derived_uuid,
	session_readback,
};
use decodex_core::{
	HistoryItemId, ProviderAttemptId, ProviderAttemptState, ProviderEvidenceId,
	ProviderEvidenceSource, ProviderPositiveEvidence, ProviderRequestId, ProviderRequestKey,
	ProviderTerminalOutcome, TurnId,
};

impl ConversationRuntime {
	pub(super) async fn finish_native_non_submission(
		&self,
		mut session: LocalSession,
		turn_id: TurnId,
		attempt_id: ProviderAttemptId,
		request_id: ProviderRequestId,
		provider_key: ProviderRequestKey,
		witness_digest: String,
	) -> ConversationOutcome {
		let evidence = ProviderPositiveEvidence::new(
			ProviderEvidenceId::new(derived_uuid(
				"provider-non-submission",
				&[attempt_id.as_str()],
			))
			.expect("derived UUID is valid"),
			attempt_id.clone(),
			request_id,
			ProviderEvidenceSource::PositiveNonSubmissionReceipt,
			ProviderTerminalOutcome::NotSubmitted,
			provider_key,
			Some(format!("app-server-refusal:{witness_digest}")),
			Some(session.codex_thread_id.clone()),
			None,
			witness_digest,
		);
		let result = match evidence {
			Ok(evidence) =>
				self.inner.provider_attempts.record_positive_evidence(&evidence).await.ok(),
			Err(_) => None,
		};
		if !matches!(
			result,
			Some(
				ProviderAttemptReconciliation::PositiveEvidenceRecorded {
					state: ProviderAttemptState::NotSubmitted
				} | ProviderAttemptReconciliation::AlreadyTerminal {
					state: ProviderAttemptState::NotSubmitted
				}
			)
		) {
			return self
				.ambiguous_session(session, turn_id, ConversationAmbiguity::TurnFinalization)
				.await;
		}
		// Read the revision changed by the evidence transaction before publishing recovery.
		match self
			.inner
			.store
			.read_ordinary_runtime_session_for_resume(&session.conversation_id)
			.await
		{
			Ok(Some(saved))
				if saved.runtime_session_id == session.runtime_session_id
					&& saved.codex_thread_id == session.codex_thread_id =>
			{
				session.conversation_revision = saved.conversation_revision;
				session.runtime_session_revision = saved.runtime_session_revision;
			},
			_ =>
				return self
					.ambiguous_session(session, turn_id, ConversationAmbiguity::TurnFinalization)
					.await,
		}
		let readback = session_readback(&session, ConversationLocalState::ManualRecovery, None);
		self.emit(ConversationOutcome::HistoryChanged {
			readback: readback.clone(),
			history_item_id: HistoryItemId::new(derived_uuid(
				"provider-non-submission",
				&[attempt_id.as_str()],
			))
			.expect("derived UUID is valid"),
		})
		.await;
		// Preserve the admitted process until the user's explicit recovery action.
		self.recover(readback, ConversationManualRecovery::ProcessUnavailable).await
	}
}
