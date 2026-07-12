//! Canonical private authority events and hash-chain verification.

use minicbor::{Decode, Encode};
use sha2::{Digest, Sha256};

use super::{InvocationIdentity, LaneId};
use crate::prelude::{Result, eyre};

const AUTHORITY_EVENT_DOMAIN: &[u8] = b"decodex.authority-event/1";

#[derive(Clone, Copy, Debug, Decode, Encode, Eq, PartialEq)]
#[cbor(index_only)]
pub enum AuthorityEventType {
	#[n(0)]
	BindingRequested,
	#[n(1)]
	BindingAttested,
	#[n(2)]
	BindingRejected,
	#[n(3)]
	DispatchSelected,
	#[n(4)]
	TransitionPlanned,
	#[n(5)]
	PrerequisiteRevalidated,
	#[n(6)]
	PrerequisiteDrifted,
	#[n(7)]
	EffectStarted,
	#[n(8)]
	EffectSucceeded,
	#[n(9)]
	EffectFailed,
	#[n(10)]
	EffectReconciled,
	#[n(11)]
	TransitionCommitted,
	#[n(12)]
	LaneQuarantined,
	#[n(13)]
	LaneTransferred,
	#[n(14)]
	LaneReleased,
}
impl AuthorityEventType {
	pub const fn as_str(self) -> &'static str {
		match self {
			Self::BindingRequested => "binding_requested",
			Self::BindingAttested => "binding_attested",
			Self::BindingRejected => "binding_rejected",
			Self::DispatchSelected => "dispatch_selected",
			Self::TransitionPlanned => "transition_planned",
			Self::PrerequisiteRevalidated => "prerequisite_revalidated",
			Self::PrerequisiteDrifted => "prerequisite_drifted",
			Self::EffectStarted => "effect_started",
			Self::EffectSucceeded => "effect_succeeded",
			Self::EffectFailed => "effect_failed",
			Self::EffectReconciled => "effect_reconciled",
			Self::TransitionCommitted => "transition_committed",
			Self::LaneQuarantined => "lane_quarantined",
			Self::LaneTransferred => "lane_transferred",
			Self::LaneReleased => "lane_released",
		}
	}
}

#[derive(Clone, Copy, Debug, Decode, Encode, Eq, PartialEq)]
#[cbor(index_only)]
pub enum AuthorityDecision {
	#[n(0)]
	Accepted,
	#[n(1)]
	Rejected,
	#[n(2)]
	Committed,
	#[n(3)]
	Reconciled,
	#[n(4)]
	AttentionRequired,
}
impl AuthorityDecision {
	pub const fn as_str(self) -> &'static str {
		match self {
			Self::Accepted => "accepted",
			Self::Rejected => "rejected",
			Self::Committed => "committed",
			Self::Reconciled => "reconciled",
			Self::AttentionRequired => "attention_required",
		}
	}
}

#[derive(Clone, Copy, Debug, Decode, Encode, Eq, PartialEq, Ord, PartialOrd)]
#[cbor(index_only)]
pub enum AuthorityReasonCode {
	#[n(0)]
	BindingMatched,
	#[n(1)]
	BindingMismatch,
	#[n(2)]
	AmbiguousRouting,
	#[n(3)]
	StaleAuthorityEpoch,
	#[n(4)]
	PrerequisiteDrift,
	#[n(5)]
	EffectReceiptRecorded,
	#[n(6)]
	ConditionalMutationUnsupported,
	#[n(7)]
	SupersessionAccepted,
	#[n(8)]
	ConflictReleased,
	#[n(9)]
	QuarantinedAmbiguousLegacyState,
}
impl AuthorityReasonCode {
	pub const fn as_str(self) -> &'static str {
		match self {
			Self::BindingMatched => "binding_matched",
			Self::BindingMismatch => "binding_mismatch",
			Self::AmbiguousRouting => "ambiguous_routing",
			Self::StaleAuthorityEpoch => "stale_authority_epoch",
			Self::PrerequisiteDrift => "prerequisite_drift",
			Self::EffectReceiptRecorded => "effect_receipt_recorded",
			Self::ConditionalMutationUnsupported => "conditional_mutation_unsupported",
			Self::SupersessionAccepted => "supersession_accepted",
			Self::ConflictReleased => "conflict_released",
			Self::QuarantinedAmbiguousLegacyState => "quarantined_ambiguous_legacy_state",
		}
	}
}

#[derive(Clone, Debug)]
pub struct AuthorityTransitionContext {
	pub invocation: InvocationIdentity,
	pub event_id: String,
	pub event_type: AuthorityEventType,
	pub transition_id: String,
	pub correlation_id: String,
	pub causation_id: String,
	pub observed_facts_fingerprint: String,
	pub decision: AuthorityDecision,
	pub reason_codes: Vec<AuthorityReasonCode>,
	pub operation_id: Option<String>,
	pub runtime_version: String,
	pub recorded_at_unix_micros: i64,
	pub boot_id_fingerprint: String,
	pub monotonic_nanos: u64,
}
impl AuthorityTransitionContext {
	pub fn into_lane_event(
		self,
		lane_id: &LaneId,
		binding_fingerprint: &str,
	) -> Result<AuthorityEventDraft> {
		let invocation_identity_fingerprint =
			self.invocation.fingerprint()?.iter().map(|byte| format!("{byte:02x}")).collect();
		let draft = AuthorityEventDraft {
			event_id: self.event_id,
			event_type: self.event_type,
			transition_id: self.transition_id,
			correlation_id: self.correlation_id,
			causation_id: self.causation_id,
			project_key: Some(lane_id.project_key().to_owned()),
			tracker_issue_id: Some(lane_id.tracker_issue_id().to_owned()),
			project_binding_fingerprint: Some(binding_fingerprint.to_owned()),
			invocation_identity_fingerprint,
			observed_facts_fingerprint: self.observed_facts_fingerprint,
			decision: self.decision,
			reason_codes: self.reason_codes,
			operation_id: self.operation_id,
			effect_id: None,
			receipt_ref: None,
			runtime_version: self.runtime_version,
			recorded_at_unix_micros: self.recorded_at_unix_micros,
			boot_id_fingerprint: self.boot_id_fingerprint,
			monotonic_nanos: self.monotonic_nanos,
		};
		draft.validate()?;
		Ok(draft)
	}
}

#[derive(Clone, Debug, Decode, Encode, Eq, PartialEq)]
#[cbor(map)]
pub struct AuthorityEventDraft {
	#[n(0)]
	pub event_id: String,
	#[n(1)]
	pub event_type: AuthorityEventType,
	#[n(2)]
	pub transition_id: String,
	#[n(3)]
	pub correlation_id: String,
	#[n(4)]
	pub causation_id: String,
	#[n(5)]
	pub project_key: Option<String>,
	#[n(6)]
	pub tracker_issue_id: Option<String>,
	#[n(7)]
	pub project_binding_fingerprint: Option<String>,
	#[n(8)]
	pub invocation_identity_fingerprint: String,
	#[n(9)]
	pub observed_facts_fingerprint: String,
	#[n(10)]
	pub decision: AuthorityDecision,
	#[n(11)]
	pub reason_codes: Vec<AuthorityReasonCode>,
	#[n(12)]
	pub operation_id: Option<String>,
	#[n(13)]
	pub effect_id: Option<String>,
	#[n(14)]
	pub receipt_ref: Option<String>,
	#[n(15)]
	pub runtime_version: String,
	#[n(16)]
	pub recorded_at_unix_micros: i64,
	#[n(17)]
	pub boot_id_fingerprint: String,
	#[n(18)]
	pub monotonic_nanos: u64,
}
impl AuthorityEventDraft {
	pub fn validate(&self) -> Result<()> {
		for value in [
			self.event_id.as_str(),
			self.transition_id.as_str(),
			self.correlation_id.as_str(),
			self.causation_id.as_str(),
			self.invocation_identity_fingerprint.as_str(),
			self.observed_facts_fingerprint.as_str(),
			self.runtime_version.as_str(),
			self.boot_id_fingerprint.as_str(),
		] {
			if value.trim().is_empty() {
				eyre::bail!("Authority event identity cannot be empty.");
			}
		}
		if self.reason_codes.is_empty()
			|| self.reason_codes.windows(2).any(|pair| pair[0] >= pair[1])
			|| self.project_key.is_some() != self.project_binding_fingerprint.is_some()
			|| self.tracker_issue_id.is_some() && self.project_key.is_none()
		{
			eyre::bail!("Authority event scope or reason codes are invalid.");
		}
		Ok(())
	}
}

#[derive(Clone, Debug, Decode, Encode, Eq, PartialEq)]
#[cbor(map)]
pub struct AuthorityEvent {
	#[n(0)]
	pub generation: u64,
	#[n(1)]
	pub sequence: u64,
	#[n(2)]
	#[cbor(with = "minicbor::bytes")]
	pub previous_event_hash: Vec<u8>,
	#[n(3)]
	pub draft: AuthorityEventDraft,
	#[n(4)]
	#[cbor(with = "minicbor::bytes")]
	pub event_hash: Vec<u8>,
}
impl AuthorityEvent {
	pub fn append(
		generation: u64,
		sequence: u64,
		previous_event_hash: &[u8],
		draft: AuthorityEventDraft,
	) -> Result<Self> {
		draft.validate()?;
		if generation == 0 || sequence == 0 || previous_event_hash.len() != 32 {
			eyre::bail!("Authority event chain position is invalid.");
		}
		let event_hash = event_hash(generation, sequence, previous_event_hash, &draft)?;
		Ok(Self {
			generation,
			sequence,
			previous_event_hash: previous_event_hash.to_vec(),
			draft,
			event_hash: event_hash.to_vec(),
		})
	}

	pub fn canonical_bytes(&self) -> Result<Vec<u8>> {
		Ok(minicbor::to_vec(self)?)
	}
}

pub fn verify_authority_event_chain(
	events: &[AuthorityEvent],
	generation: u64,
	genesis_hash: &[u8],
) -> Result<()> {
	if generation == 0 || genesis_hash.len() != 32 {
		eyre::bail!("Authority generation genesis is invalid.");
	}
	let mut expected_previous = genesis_hash.to_vec();
	for (index, event) in events.iter().enumerate() {
		let expected_sequence = u64::try_from(index)?.saturating_add(1);
		if event.generation != generation
			|| event.sequence != expected_sequence
			|| event.previous_event_hash != expected_previous
			|| event.event_hash.len() != 32
			|| event.event_hash
				!= event_hash(
					event.generation,
					event.sequence,
					&event.previous_event_hash,
					&event.draft,
				)? {
			eyre::bail!("Authority event chain verification failed.");
		}
		expected_previous.clone_from(&event.event_hash);
	}
	Ok(())
}

fn event_hash(
	generation: u64,
	sequence: u64,
	previous_event_hash: &[u8],
	draft: &AuthorityEventDraft,
) -> Result<[u8; 32]> {
	let encoded = minicbor::to_vec(draft)?;
	let mut digest = Sha256::new();
	digest.update(AUTHORITY_EVENT_DOMAIN);
	digest.update(generation.to_be_bytes());
	digest.update(sequence.to_be_bytes());
	digest.update(previous_event_hash);
	digest.update(encoded);
	Ok(digest.finalize().into())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn lane_authority_v2_c5_hash_chain_rejects_rewrite_delete_reorder_and_fork() {
		let genesis = [7_u8; 32];
		let first = AuthorityEvent::append(1, 1, &genesis, draft("event-1", "transition-1"))
			.expect("first event");
		let second =
			AuthorityEvent::append(1, 2, &first.event_hash, draft("event-2", "transition-2"))
				.expect("second event");
		verify_authority_event_chain(&[first.clone(), second.clone()], 1, &genesis)
			.expect("valid chain");

		let mut rewritten = first.clone();
		rewritten.draft.decision = AuthorityDecision::Rejected;
		assert!(verify_authority_event_chain(&[rewritten, second.clone()], 1, &genesis).is_err());
		assert!(verify_authority_event_chain(&[second.clone()], 1, &genesis).is_err());
		assert!(
			verify_authority_event_chain(&[second.clone(), first.clone()], 1, &genesis).is_err()
		);
		let fork = AuthorityEvent::append(1, 2, &first.event_hash, draft("fork", "transition-3"))
			.expect("fork event");
		assert!(verify_authority_event_chain(&[first, second, fork], 1, &genesis).is_err());
	}

	fn draft(event_id: &str, transition_id: &str) -> AuthorityEventDraft {
		AuthorityEventDraft {
			event_id: event_id.to_owned(),
			event_type: AuthorityEventType::TransitionCommitted,
			transition_id: transition_id.to_owned(),
			correlation_id: String::from("correlation-1"),
			causation_id: String::from("cause-1"),
			project_key: Some(String::from("pubfi")),
			tracker_issue_id: Some(String::from("PUB-1711")),
			project_binding_fingerprint: Some(String::from("binding-1")),
			invocation_identity_fingerprint: String::from("invocation-1"),
			observed_facts_fingerprint: String::from("facts-1"),
			decision: AuthorityDecision::Committed,
			reason_codes: vec![AuthorityReasonCode::BindingMatched],
			operation_id: Some(String::from("operation-1")),
			effect_id: None,
			receipt_ref: None,
			runtime_version: String::from("0.2.0"),
			recorded_at_unix_micros: 1,
			boot_id_fingerprint: String::from("boot-1"),
			monotonic_nanos: 1,
		}
	}
}
