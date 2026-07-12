use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::LaneId;
use crate::prelude::{Result, eyre};

pub const NO_EFFECTIVE_DELTA_RECOVERY_SCHEMA: &str = "decodex/no-effective-delta-recovery/1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NoEffectiveDeltaRecoveryState {
	RetryScheduled,
	AttentionRequired,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct NoEffectiveDeltaFacts {
	base_oid: String,
	head_oid: String,
	merge_base_oid: String,
	patch_set_digest: String,
	name_only_digest: String,
	worktree_status_digest: String,
	expected_surface_digest: String,
	acceptance_criteria_digest: String,
	checkpoint_facts_digest: String,
	validation_results_digest: String,
	explicit_blocker: bool,
}
impl NoEffectiveDeltaFacts {
	#[allow(clippy::too_many_arguments)]
	pub fn new(
		base_oid: &str,
		head_oid: &str,
		merge_base_oid: &str,
		patch_set_digest: &str,
		name_only_digest: &str,
		worktree_status_digest: &str,
		expected_surface_digest: &str,
		acceptance_criteria_digest: &str,
		checkpoint_facts_digest: &str,
		validation_results_digest: &str,
		explicit_blocker: bool,
	) -> Result<Self> {
		let record = Self {
			base_oid: base_oid.to_owned(),
			head_oid: head_oid.to_owned(),
			merge_base_oid: merge_base_oid.to_owned(),
			patch_set_digest: patch_set_digest.to_owned(),
			name_only_digest: name_only_digest.to_owned(),
			worktree_status_digest: worktree_status_digest.to_owned(),
			expected_surface_digest: expected_surface_digest.to_owned(),
			acceptance_criteria_digest: acceptance_criteria_digest.to_owned(),
			checkpoint_facts_digest: checkpoint_facts_digest.to_owned(),
			validation_results_digest: validation_results_digest.to_owned(),
			explicit_blocker,
		};
		record.validate()?;
		Ok(record)
	}

	fn validate(&self) -> Result<()> {
		for (field, value) in [
			("base_oid", self.base_oid.as_str()),
			("head_oid", self.head_oid.as_str()),
			("merge_base_oid", self.merge_base_oid.as_str()),
			("patch_set_digest", self.patch_set_digest.as_str()),
			("name_only_digest", self.name_only_digest.as_str()),
			("worktree_status_digest", self.worktree_status_digest.as_str()),
			("expected_surface_digest", self.expected_surface_digest.as_str()),
			("acceptance_criteria_digest", self.acceptance_criteria_digest.as_str()),
			("checkpoint_facts_digest", self.checkpoint_facts_digest.as_str()),
			("validation_results_digest", self.validation_results_digest.as_str()),
		] {
			if value.trim().is_empty() {
				eyre::bail!("No-effective-delta fact `{field}` cannot be empty.");
			}
		}
		Ok(())
	}

	fn digest(&self) -> Result<String> {
		self.validate()?;
		let digest = Sha256::digest(serde_json::to_vec(self)?);
		Ok(sha256_text(&digest))
	}
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct NoEffectiveDeltaRecovery {
	schema: String,
	operation_id: String,
	lane_id: LaneId,
	ordinal: u8,
	state: NoEffectiveDeltaRecoveryState,
	facts: NoEffectiveDeltaFacts,
	fact_digest: String,
	idempotency_key: String,
}
impl NoEffectiveDeltaRecovery {
	pub fn operation_id(&self) -> &str {
		&self.operation_id
	}

	pub fn lane_id(&self) -> &LaneId {
		&self.lane_id
	}

	pub const fn ordinal(&self) -> u8 {
		self.ordinal
	}

	pub const fn facts(&self) -> &NoEffectiveDeltaFacts {
		&self.facts
	}

	pub const fn state(&self) -> NoEffectiveDeltaRecoveryState {
		self.state
	}

	pub fn idempotency_key(&self) -> &str {
		&self.idempotency_key
	}

	pub fn validate(&self) -> Result<()> {
		if self.schema != NO_EFFECTIVE_DELTA_RECOVERY_SCHEMA
			|| self.operation_id.trim().is_empty()
			|| self.ordinal != 1
			|| self.fact_digest.trim().is_empty()
			|| self.idempotency_key.trim().is_empty()
		{
			eyre::bail!("No-effective-delta recovery record is invalid.");
		}
		if self.facts.digest()? != self.fact_digest {
			eyre::bail!("No-effective-delta recovery fact digest does not match its diagnostics.");
		}
		Ok(())
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NoEffectiveDeltaCommand {
	Observe {
		operation_id: String,
		lane_id: LaneId,
		facts: NoEffectiveDeltaFacts,
	},
	ObserveRetryResult {
		operation_id: String,
		lane_id: LaneId,
		facts: NoEffectiveDeltaFacts,
	},
	ProveAlreadySatisfied {
		independent_validator_receipt: String,
		acceptance_criteria_digest: String,
	},
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NoEffectiveDeltaDecision {
	Blocked,
	AlreadySatisfied { validator_receipt: String },
	Retry(NoEffectiveDeltaRecovery),
	AttentionRequired { reason_code: &'static str, recovery: NoEffectiveDeltaRecovery },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NoEffectiveDeltaRejection {
	InvalidEvidence,
	OperationDrift,
}

pub fn decide_no_effective_delta(
	current: Option<&NoEffectiveDeltaRecovery>,
	command: NoEffectiveDeltaCommand,
) -> Result<NoEffectiveDeltaDecision, NoEffectiveDeltaRejection> {
	match command {
		NoEffectiveDeltaCommand::ProveAlreadySatisfied {
			independent_validator_receipt,
			acceptance_criteria_digest,
		} => {
			if current.is_some()
				|| independent_validator_receipt.trim().is_empty()
				|| acceptance_criteria_digest.trim().is_empty()
			{
				return Err(NoEffectiveDeltaRejection::InvalidEvidence);
			}
			Ok(NoEffectiveDeltaDecision::AlreadySatisfied {
				validator_receipt: independent_validator_receipt,
			})
		},
		NoEffectiveDeltaCommand::Observe { operation_id, lane_id, facts } => {
			if operation_id.trim().is_empty() || facts.validate().is_err() {
				return Err(NoEffectiveDeltaRejection::InvalidEvidence);
			}
			if facts.explicit_blocker {
				return Ok(NoEffectiveDeltaDecision::Blocked);
			}
			let fact_digest =
				facts.digest().map_err(|_| NoEffectiveDeltaRejection::InvalidEvidence)?;
			if let Some(recovery) = current {
				if recovery.operation_id != operation_id
					|| recovery.lane_id != lane_id
					|| recovery.fact_digest != fact_digest
				{
					return Err(NoEffectiveDeltaRejection::OperationDrift);
				}
				return Ok(match recovery.state {
					NoEffectiveDeltaRecoveryState::RetryScheduled =>
						NoEffectiveDeltaDecision::Retry(recovery.clone()),
					NoEffectiveDeltaRecoveryState::AttentionRequired =>
						NoEffectiveDeltaDecision::AttentionRequired {
							reason_code: "no_effective_delta_unresolved",
							recovery: recovery.clone(),
						},
				});
			}

			let idempotency_key = recovery_idempotency_key(&operation_id, &lane_id, &facts)
				.map_err(|_| NoEffectiveDeltaRejection::InvalidEvidence)?;
			Ok(NoEffectiveDeltaDecision::Retry(NoEffectiveDeltaRecovery {
				schema: String::from(NO_EFFECTIVE_DELTA_RECOVERY_SCHEMA),
				operation_id,
				lane_id,
				ordinal: 1,
				state: NoEffectiveDeltaRecoveryState::RetryScheduled,
				facts,
				fact_digest,
				idempotency_key,
			}))
		},
		NoEffectiveDeltaCommand::ObserveRetryResult { operation_id, lane_id, facts } => {
			let Some(recovery) = current else {
				return Err(NoEffectiveDeltaRejection::InvalidEvidence);
			};
			let fact_digest =
				facts.digest().map_err(|_| NoEffectiveDeltaRejection::InvalidEvidence)?;
			if recovery.operation_id != operation_id
				|| recovery.lane_id != lane_id
				|| recovery.fact_digest != fact_digest
			{
				return Err(NoEffectiveDeltaRejection::OperationDrift);
			}
			let mut recovery = recovery.clone();
			recovery.state = NoEffectiveDeltaRecoveryState::AttentionRequired;
			Ok(NoEffectiveDeltaDecision::AttentionRequired {
				reason_code: "no_effective_delta_unresolved",
				recovery,
			})
		},
	}
}

fn recovery_idempotency_key(
	operation_id: &str,
	lane_id: &LaneId,
	facts: &NoEffectiveDeltaFacts,
) -> Result<String> {
	let material = serde_json::json!({
		"lane_id": lane_id,
		"operation_id": operation_id,
		"validation_phase": "implementation",
		"base_oid": facts.base_oid,
		"head_oid": facts.head_oid,
		"expected_surface_digest": facts.expected_surface_digest,
		"ordinal": 1,
	});
	let digest = Sha256::digest(serde_json::to_vec(&material)?);
	Ok(sha256_text(&digest))
}

fn sha256_text(digest: &[u8]) -> String {
	format!("sha256:{}", digest.iter().map(|byte| format!("{byte:02x}")).collect::<String>())
}

#[cfg(test)]
mod tests {
	use super::*;

	fn facts(blocked: bool) -> NoEffectiveDeltaFacts {
		NoEffectiveDeltaFacts::new(
			"base",
			"head",
			"merge-base",
			"patch",
			"names",
			"status",
			"surface",
			"acceptance",
			"checkpoints",
			"validation",
			blocked,
		)
		.expect("facts")
	}

	fn observe(facts: NoEffectiveDeltaFacts) -> NoEffectiveDeltaCommand {
		NoEffectiveDeltaCommand::Observe {
			operation_id: String::from("operation-1"),
			lane_id: LaneId::new("project", "issue").expect("lane"),
			facts,
		}
	}

	#[test]
	fn first_observation_plans_one_deterministic_retry_and_replay_returns_it() {
		let NoEffectiveDeltaDecision::Retry(recovery) =
			decide_no_effective_delta(None, observe(facts(false))).expect("retry")
		else {
			panic!("expected retry");
		};
		let replay =
			decide_no_effective_delta(Some(&recovery), observe(facts(false))).expect("replay");
		assert_eq!(recovery.facts(), &facts(false));
		assert!(recovery.idempotency_key().starts_with("sha256:"));
		assert_eq!(replay, NoEffectiveDeltaDecision::Retry(recovery));
	}

	#[test]
	fn persisted_recovery_rejects_diagnostics_that_do_not_match_the_fact_digest() {
		let NoEffectiveDeltaDecision::Retry(recovery) =
			decide_no_effective_delta(None, observe(facts(false))).expect("retry")
		else {
			panic!("expected retry");
		};
		let mut payload = serde_json::to_value(recovery).expect("serialize recovery");
		payload["facts"]["head_oid"] = serde_json::Value::String(String::from("tampered"));
		let tampered =
			serde_json::from_value::<NoEffectiveDeltaRecovery>(payload).expect("decode payload");
		assert!(tampered.validate().is_err());
	}

	#[test]
	fn retry_result_converges_to_reason_coded_attention() {
		let NoEffectiveDeltaDecision::Retry(recovery) =
			decide_no_effective_delta(None, observe(facts(false))).expect("retry")
		else {
			panic!("expected retry");
		};
		let decision = decide_no_effective_delta(
			Some(&recovery),
			NoEffectiveDeltaCommand::ObserveRetryResult {
				operation_id: String::from("operation-1"),
				lane_id: LaneId::new("project", "issue").expect("lane"),
				facts: facts(false),
			},
		)
		.expect("attention");
		let NoEffectiveDeltaDecision::AttentionRequired { reason_code, recovery } = decision else {
			panic!("expected attention");
		};
		assert_eq!(reason_code, "no_effective_delta_unresolved");
		assert_eq!(recovery.state(), NoEffectiveDeltaRecoveryState::AttentionRequired);
		assert_eq!(
			decide_no_effective_delta(Some(&recovery), observe(facts(false))).expect("replay"),
			NoEffectiveDeltaDecision::AttentionRequired {
				reason_code: "no_effective_delta_unresolved",
				recovery,
			}
		);
	}

	#[test]
	fn blocker_does_not_enter_retry_and_drift_fails_closed() {
		assert_eq!(
			decide_no_effective_delta(None, observe(facts(true))).expect("blocked"),
			NoEffectiveDeltaDecision::Blocked
		);
		let NoEffectiveDeltaDecision::Retry(recovery) =
			decide_no_effective_delta(None, observe(facts(false))).expect("retry")
		else {
			panic!("expected retry");
		};
		let mut changed = facts(false);
		changed.head_oid = String::from("changed-head");
		assert_eq!(
			decide_no_effective_delta(Some(&recovery), observe(changed)),
			Err(NoEffectiveDeltaRejection::OperationDrift)
		);
	}

	#[test]
	fn already_satisfied_requires_independent_evidence_before_recovery() {
		assert_eq!(
			decide_no_effective_delta(
				None,
				NoEffectiveDeltaCommand::ProveAlreadySatisfied {
					independent_validator_receipt: String::from("validator-receipt"),
					acceptance_criteria_digest: String::from("acceptance"),
				},
			),
			Ok(NoEffectiveDeltaDecision::AlreadySatisfied {
				validator_receipt: String::from("validator-receipt")
			})
		);
	}
}
