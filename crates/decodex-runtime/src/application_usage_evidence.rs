//! Aggregate existing native receipts without assigning work by selection or latest output.
use super::{ProductStore, ServiceApplication};
use decodex_protocol::{
	QueryResultPayload, UsageEvidenceResult, UsageEvidenceScope, UsageEvidenceSource,
	UsageEvidenceTotals,
};

impl ServiceApplication {
	pub(super) async fn query_usage_evidence(
		&self,
		scope: &UsageEvidenceScope,
	) -> QueryResultPayload {
		let unavailable = || QueryResultPayload::UsageEvidence(UsageEvidenceResult::Unavailable);
		let (Some(agent), ProductStore::Available(store)) = (&self.agent, &self.store) else {
			return unavailable();
		};
		let Some(source) = agent.runtime_source().await else { return unavailable() };
		let Ok(Some(saved)) = store
			.read_usage_evidence(
				matches!(scope, UsageEvidenceScope::Agent { .. }),
				scope.id().as_str().into(),
			)
			.await
		else {
			return unavailable();
		};
		if agent.runtime_source().await.as_ref() != Some(&source) {
			return unavailable();
		}
		let totals = UsageEvidenceTotals {
			turns: saved.turns,
			token_turns: saved.token_turns,
			duration_turns: saved.duration_turns,
			shared_turns: saved.shared_turns,
			unattributed_turns: saved.unattributed_turns,
			input_tokens: saved.input_tokens,
			output_tokens: saved.output_tokens,
			duration_ms: saved.duration_ms,
		};
		let mut sources = Vec::new();
		let mut bytes = 0usize;
		for row in saved.sources {
			let row = UsageEvidenceSource {
				event_id: row.event_id,
				agent_id: row.agent_id,
				thread_id: row.thread_id,
				turn_id: row.turn_id,
				work_ids: row.work_ids,
				work_count: row.work_count,
				input_tokens: row.input_tokens,
				output_tokens: row.output_tokens,
				duration_ms: row.duration_ms,
			};
			let size = serde_json::to_vec(&row).map_or(usize::MAX, |v| v.len() + 1);
			if bytes.saturating_add(size) > 40 * 1024 {
				break;
			}
			bytes += size;
			sources.push(row);
		}
		QueryResultPayload::UsageEvidence(UsageEvidenceResult::Available {
			source,
			scope: scope.clone(),
			sources_truncated: (sources.len() as u64) < totals.turns,
			totals,
			sources,
		})
	}
}
