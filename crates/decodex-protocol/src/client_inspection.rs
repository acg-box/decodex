//! Explicit read-only inspection of current Agent evidence.
use std::time::Duration;

use crate::{
	AgentMemoryResult, UsageEvidenceResult, UsageEvidenceScope,
	client::{
		self, AgentClient, ClientFailure, EntityId, QueryPayload, QueryResultPayload,
		ResetCardClient, WireText,
	},
};

impl AgentClient {
	/// Read actual retained native counters and explicit product attribution.
	pub async fn usage_evidence(
		&self,
		scope: UsageEvidenceScope,
	) -> Result<UsageEvidenceResult, ClientFailure> {
		self.transport.require_local_profile()?;

		let completed = self
			.transport
			.query_inner("usage-evidence", QueryPayload::GetUsageEvidence { scope: scope.clone() })
			.await?;

		client::close_one_shot_socket(completed.socket).await;

		let QueryResultPayload::UsageEvidence(result) = completed.value else {
			return Err(ClientFailure::ProtocolMalformed);
		};

		if matches!(&result,UsageEvidenceResult::Available{scope:actual,..} if actual!=&scope) {
			return Err(ClientFailure::ProtocolMalformed);
		}

		Ok(result)
	}

	/// Read one memory page for the exact expected conversation without starting a turn.
	pub async fn memory_summary(
		&self,
		work_id: EntityId,
		thread_id: WireText,
		digest: Option<WireText>,
		offset: usize,
	) -> Result<AgentMemoryResult, ClientFailure> {
		self.transport.require_local_profile()?;

		let transport = ResetCardClient {
			profile: self.transport.profile.clone(),
			timeout: Duration::from_secs(35),
		};
		let completed = transport
			.query_inner(
				"agent-memory",
				QueryPayload::GetAgentMemory {
					work_id: work_id.clone(),
					thread_id: thread_id.clone(),
					digest,
					offset,
				},
			)
			.await?;

		client::close_one_shot_socket(completed.socket).await;

		let QueryResultPayload::AgentMemory(result) = completed.value else {
			return Err(ClientFailure::ProtocolMalformed);
		};

		match &result {
			AgentMemoryResult::Page {
				work_id: actual,
				thread_id: thread,
				offset: actual_offset,
				..
			} if actual != &work_id || thread != &thread_id || actual_offset != &offset =>
				Err(ClientFailure::ProtocolMalformed),
			AgentMemoryResult::Empty { work_id: actual, thread_id: thread }
				if actual != &work_id || thread != &thread_id || offset != 0 =>
				Err(ClientFailure::ProtocolMalformed),
			_ => Ok(result),
		}
	}
}
