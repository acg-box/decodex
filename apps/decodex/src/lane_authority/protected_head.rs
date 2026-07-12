//! Signed host-local anchor for the private authority event chain.

use ed25519_dalek::{Signature, Signer as _, SigningKey, Verifier as _, VerifyingKey};
use minicbor::{Decode, Encode};

use super::AuthorityEvent;
use crate::prelude::{Result, eyre};

const PROTECTED_HEAD_DOMAIN: &[u8] = b"decodex.authority-protected-head/1";

#[derive(Clone, Debug, Decode, Encode, Eq, PartialEq)]
#[cbor(map)]
struct ProtectedAuthorityHeadBody {
	#[n(0)]
	host_id: String,
	#[n(1)]
	key_id: String,
	#[n(2)]
	generation: u64,
	#[n(3)]
	sequence: u64,
	#[n(4)]
	event_hash: Vec<u8>,
	#[n(5)]
	database_digest: Vec<u8>,
}

#[derive(Clone, Debug, Decode, Encode, Eq, PartialEq)]
#[cbor(map)]
pub(crate) struct ProtectedAuthorityHead {
	#[n(0)]
	body: ProtectedAuthorityHeadBody,
	#[n(1)]
	public_key: Vec<u8>,
	#[n(2)]
	signature: Vec<u8>,
}
impl ProtectedAuthorityHead {
	pub(crate) fn generation(&self) -> u64 {
		self.body.generation
	}
	pub(crate) fn sequence(&self) -> u64 {
		self.body.sequence
	}
	pub(crate) fn event_hash(&self) -> &[u8] {
		&self.body.event_hash
	}

	fn signed_bytes(body: &ProtectedAuthorityHeadBody) -> Result<Vec<u8>> {
		let mut bytes = PROTECTED_HEAD_DOMAIN.to_vec();
		bytes.extend(minicbor::to_vec(body)?);
		Ok(bytes)
	}
}

#[derive(Clone)]
pub(crate) struct Ed25519HostAuthorityKey {
	key_id: String,
	signing_key: SigningKey,
}
impl Ed25519HostAuthorityKey {
	pub(crate) fn from_seed(key_id: impl Into<String>, seed: [u8; 32]) -> Self {
		Self { key_id: key_id.into(), signing_key: SigningKey::from_bytes(&seed) }
	}

	pub(crate) fn public_key(&self) -> [u8; 32] {
		self.signing_key.verifying_key().to_bytes()
	}

	pub(crate) fn sign(
		&self,
		host_id: &str,
		generation: u64,
		sequence: u64,
		event_hash: &[u8],
		database_digest: &[u8],
	) -> Result<ProtectedAuthorityHead> {
		if host_id.trim().is_empty() || event_hash.len() != 32 || database_digest.len() != 32 {
			eyre::bail!("protected_authority_head_invalid");
		}
		let body = ProtectedAuthorityHeadBody {
			host_id: host_id.to_owned(),
			key_id: self.key_id.clone(),
			generation,
			sequence,
			event_hash: event_hash.to_vec(),
			database_digest: database_digest.to_vec(),
		};
		let signature = self.signing_key.sign(&ProtectedAuthorityHead::signed_bytes(&body)?);
		Ok(ProtectedAuthorityHead {
			body,
			public_key: self.public_key().to_vec(),
			signature: signature.to_bytes().to_vec(),
		})
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProtectedHeadDisposition {
	Current,
	Advanced,
}

pub(crate) fn reconcile_protected_head(
	host_id: &str,
	key: &Ed25519HostAuthorityKey,
	protected: &ProtectedAuthorityHead,
	generation: u64,
	genesis_hash: &[u8],
	events: &[AuthorityEvent],
	database_digest: &[u8],
) -> Result<(ProtectedHeadDisposition, ProtectedAuthorityHead)> {
	verify_signature(host_id, key, protected)?;
	if protected.generation() != generation {
		eyre::bail!("protected_authority_head_generation_mismatch");
	}
	let database_sequence = u64::try_from(events.len())?;
	let database_hash = events.last().map_or(genesis_hash, |event| event.event_hash.as_slice());
	if protected.sequence() > database_sequence {
		eyre::bail!("protected_authority_head_ahead_of_database");
	}
	let anchored_hash = if protected.sequence() == 0 {
		genesis_hash
	} else {
		events
			.get(usize::try_from(protected.sequence() - 1)?)
			.map(|event| event.event_hash.as_slice())
			.ok_or_else(|| eyre::eyre!("protected_authority_head_sequence_missing"))?
	};
	if protected.event_hash() != anchored_hash {
		eyre::bail!("protected_authority_head_hash_mismatch");
	}
	if protected.sequence() == database_sequence {
		if protected.body.database_digest != database_digest {
			eyre::bail!("protected_authority_head_database_digest_mismatch");
		}
		return Ok((ProtectedHeadDisposition::Current, protected.clone()));
	}
	let advanced =
		key.sign(host_id, generation, database_sequence, database_hash, database_digest)?;
	Ok((ProtectedHeadDisposition::Advanced, advanced))
}

fn verify_signature(
	host_id: &str,
	key: &Ed25519HostAuthorityKey,
	protected: &ProtectedAuthorityHead,
) -> Result<()> {
	if protected.body.host_id != host_id
		|| protected.body.key_id != key.key_id
		|| protected.public_key != key.public_key()
	{
		eyre::bail!("protected_authority_head_identity_mismatch");
	}
	let public_key: [u8; 32] = protected
		.public_key
		.as_slice()
		.try_into()
		.map_err(|_| eyre::eyre!("protected_authority_head_public_key_invalid"))?;
	let signature = Signature::from_slice(&protected.signature)
		.map_err(|_| eyre::eyre!("protected_authority_head_signature_invalid"))?;
	VerifyingKey::from_bytes(&public_key)?
		.verify(&ProtectedAuthorityHead::signed_bytes(&protected.body)?, &signature)
		.map_err(|_| eyre::eyre!("protected_authority_head_signature_invalid"))
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::lane_authority::{
		AuthorityDecision, AuthorityEventDraft, AuthorityEventType, AuthorityReasonCode,
	};

	#[test]
	fn lane_authority_v2_c1_tel_09_signed_head_freezes_tamper_and_ahead_state() {
		let key = Ed25519HostAuthorityKey::from_seed("key-1", [3_u8; 32]);
		let genesis = [5_u8; 32];
		let first = AuthorityEvent::append(1, 1, &genesis, draft("event-1")).expect("event");
		let digest = [8_u8; 32];
		let genesis_head = key.sign("host-1", 1, 0, &genesis, &digest).expect("head");
		let (disposition, advanced) = reconcile_protected_head(
			"host-1",
			&key,
			&genesis_head,
			1,
			&genesis,
			std::slice::from_ref(&first),
			&digest,
		)
		.expect("advance");
		assert_eq!(disposition, ProtectedHeadDisposition::Advanced);
		let mut rewritten = advanced.clone();
		rewritten.body.event_hash[0] ^= 1;
		assert!(
			reconcile_protected_head(
				"host-1",
				&key,
				&rewritten,
				1,
				&genesis,
				std::slice::from_ref(&first),
				&digest,
			)
			.is_err()
		);
		assert!(
			reconcile_protected_head("host-1", &key, &advanced, 1, &genesis, &[], &digest).is_err()
		);
	}

	#[test]
	fn lane_authority_v2_c5_tel_10_recovers_only_database_ahead_suffix() {
		let key = Ed25519HostAuthorityKey::from_seed("key-1", [7_u8; 32]);
		let genesis = [2_u8; 32];
		let first = AuthorityEvent::append(4, 1, &genesis, draft("event-1")).expect("first");
		let second =
			AuthorityEvent::append(4, 2, &first.event_hash, draft("event-2")).expect("second");
		let old = key.sign("host-1", 4, 1, &first.event_hash, &[6_u8; 32]).expect("old");
		let (disposition, recovered) = reconcile_protected_head(
			"host-1",
			&key,
			&old,
			4,
			&genesis,
			&[first, second],
			&[9_u8; 32],
		)
		.expect("recover");
		assert_eq!(disposition, ProtectedHeadDisposition::Advanced);
		assert_eq!(recovered.sequence(), 2);
	}

	fn draft(event_id: &str) -> AuthorityEventDraft {
		AuthorityEventDraft {
			event_id: event_id.to_owned(),
			event_type: AuthorityEventType::TransitionCommitted,
			transition_id: String::from("transition-1"),
			correlation_id: String::from("correlation-1"),
			causation_id: String::from("cause-1"),
			project_key: Some(String::from("project-1")),
			tracker_issue_id: Some(String::from("issue-1")),
			project_binding_fingerprint: Some(String::from("binding-1")),
			invocation_identity_fingerprint: String::from("invocation-1"),
			observed_facts_fingerprint: String::from("facts-1"),
			decision: AuthorityDecision::Committed,
			reason_codes: vec![AuthorityReasonCode::BindingMatched],
			operation_id: None,
			effect_id: None,
			receipt_ref: None,
			runtime_version: String::from("0.2.0"),
			recorded_at_unix_micros: 1,
			boot_id_fingerprint: String::from("boot-1"),
			monotonic_nanos: 1,
		}
	}
}
