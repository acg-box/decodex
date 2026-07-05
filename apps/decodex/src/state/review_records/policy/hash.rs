use serde::Serialize;
use sha2::{Digest as _, Sha256};

use crate::{
	prelude::{Result, eyre},
	state::review_records::policy::REVIEW_CHECKPOINT_PROMPT_VERSION,
};

pub(in crate::state::review_records::policy) fn review_checkpoint_evidence_key_json(
	phase: &str,
	review_level: &str,
	head_sha: &str,
) -> Result<String> {
	#[derive(Serialize)]
	struct ReviewCheckpointEvidenceKey<'a> {
		schema: &'static str,
		artifact_kind: &'static str,
		phase: &'a str,
		head_sha: &'a str,
		review_level: &'a str,
		review_prompt_version: &'static str,
	}

	serde_json::to_string(&ReviewCheckpointEvidenceKey {
		schema: "decodex.evidence_key/1",
		artifact_kind: "issue_review_checkpoint",
		phase,
		head_sha,
		review_level,
		review_prompt_version: REVIEW_CHECKPOINT_PROMPT_VERSION,
	})
	.map_err(|error| eyre::eyre!("failed to serialize review checkpoint evidence key: {error}"))
}

pub(in crate::state::review_records::policy) fn evidence_artifact_key_hash(
	artifact_kind: &str,
	key_json: &str,
) -> String {
	let payload = format!("{artifact_kind}\n{key_json}");
	let digest = Sha256::digest(payload.as_bytes());
	let mut hash = String::with_capacity(64);

	for byte in digest {
		hash.push(char::from(b"0123456789abcdef"[(byte >> 4) as usize]));
		hash.push(char::from(b"0123456789abcdef"[(byte & 0x0f) as usize]));
	}

	hash
}
