use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
	lane_authority::{LaneId, ProjectBindingAttestation},
	prelude::{Result, eyre},
};

pub const INTAKE_AUTHORITY_SCHEMA: &str = "decodex/intake-authority/1";

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum IntakeAuthorityKind {
	DecisionContract {
		accepted_contract_id: String,
		contract_fingerprint: String,
	},
	IssueBatch {
		accepted_intake_id: String,
		batch_fingerprint: String,
	},
	RecoveryAdoption {
		recovery_request_id: String,
		evidence_fingerprint: String,
	},
	Transfer {
		transfer_authority_id: String,
		source_lane_id: LaneId,
		source_intake_authority_id: String,
		source_provenance_fingerprint: String,
		transfer_causation_event_id: String,
	},
}
impl IntakeAuthorityKind {
	pub const fn as_str(&self) -> &'static str {
		match self {
			Self::DecisionContract { .. } => "decision_contract",
			Self::IssueBatch { .. } => "issue_batch",
			Self::RecoveryAdoption { .. } => "recovery_adoption",
			Self::Transfer { .. } => "transfer",
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct IntakeAuthority {
	schema: String,
	authority_id: String,
	project_key: String,
	binding_attestation: ProjectBindingAttestation,
	plan_id: String,
	program_id: String,
	actor: String,
	source: String,
	correlation_id: String,
	accepted_at: String,
	accepted_at_unix: i64,
	authority: IntakeAuthorityKind,
	fingerprint: String,
}
impl IntakeAuthority {
	#[allow(clippy::too_many_arguments)]
	pub fn new(
		authority_id: &str,
		project_key: &str,
		binding_attestation: ProjectBindingAttestation,
		plan_id: &str,
		program_id: &str,
		actor: &str,
		source: &str,
		correlation_id: &str,
		accepted_at: &str,
		accepted_at_unix: i64,
		authority: IntakeAuthorityKind,
	) -> Result<Self> {
		for (field, value) in [
			("authority_id", authority_id),
			("project_key", project_key),
			("plan_id", plan_id),
			("program_id", program_id),
			("actor", actor),
			("source", source),
			("correlation_id", correlation_id),
			("accepted_at", accepted_at),
		] {
			if value.trim().is_empty() {
				eyre::bail!("Intake authority `{field}` cannot be empty.");
			}
		}
		if binding_attestation.project_key() != project_key {
			eyre::bail!("Intake authority project does not match its binding attestation.");
		}
		validate_kind(&authority)?;

		let mut record = Self {
			schema: String::from(INTAKE_AUTHORITY_SCHEMA),
			authority_id: authority_id.to_owned(),
			project_key: project_key.to_owned(),
			binding_attestation,
			plan_id: plan_id.to_owned(),
			program_id: program_id.to_owned(),
			actor: actor.to_owned(),
			source: source.to_owned(),
			correlation_id: correlation_id.to_owned(),
			accepted_at: accepted_at.to_owned(),
			accepted_at_unix,
			authority,
			fingerprint: String::new(),
		};
		record.fingerprint = record.calculate_fingerprint()?;
		Ok(record)
	}

	fn calculate_fingerprint(&self) -> Result<String> {
		let mut canonical = self.clone();
		canonical.fingerprint.clear();
		let digest = Sha256::digest(serde_json::to_vec(&canonical)?);
		Ok(format!(
			"sha256:{}",
			digest.iter().map(|byte| format!("{byte:02x}")).collect::<String>()
		))
	}

	pub fn validate(&self) -> Result<()> {
		if self.schema != INTAKE_AUTHORITY_SCHEMA
			|| self.fingerprint != self.calculate_fingerprint()?
		{
			eyre::bail!("Intake authority fingerprint or schema is invalid.");
		}
		validate_kind(&self.authority)
	}

	pub fn authority_id(&self) -> &str {
		&self.authority_id
	}

	pub fn project_key(&self) -> &str {
		&self.project_key
	}

	pub fn binding_attestation(&self) -> &ProjectBindingAttestation {
		&self.binding_attestation
	}

	pub fn plan_id(&self) -> &str {
		&self.plan_id
	}

	pub fn program_id(&self) -> &str {
		&self.program_id
	}

	pub fn actor(&self) -> &str {
		&self.actor
	}

	pub fn source(&self) -> &str {
		&self.source
	}

	pub fn correlation_id(&self) -> &str {
		&self.correlation_id
	}

	pub fn accepted_at(&self) -> &str {
		&self.accepted_at
	}

	pub const fn accepted_at_unix(&self) -> i64 {
		self.accepted_at_unix
	}

	pub fn authority(&self) -> &IntakeAuthorityKind {
		&self.authority
	}

	pub fn fingerprint(&self) -> &str {
		&self.fingerprint
	}
}

fn validate_kind(authority: &IntakeAuthorityKind) -> Result<()> {
	let values = match authority {
		IntakeAuthorityKind::DecisionContract { accepted_contract_id, contract_fingerprint } => {
			vec![
				("accepted_contract_id", accepted_contract_id),
				("contract_fingerprint", contract_fingerprint),
			]
		},
		IntakeAuthorityKind::IssueBatch { accepted_intake_id, batch_fingerprint } => {
			vec![
				("accepted_intake_id", accepted_intake_id),
				("batch_fingerprint", batch_fingerprint),
			]
		},
		IntakeAuthorityKind::RecoveryAdoption { recovery_request_id, evidence_fingerprint } => {
			vec![
				("recovery_request_id", recovery_request_id),
				("evidence_fingerprint", evidence_fingerprint),
			]
		},
		IntakeAuthorityKind::Transfer {
			transfer_authority_id,
			source_intake_authority_id,
			source_provenance_fingerprint,
			transfer_causation_event_id,
			..
		} => vec![
			("transfer_authority_id", transfer_authority_id),
			("source_intake_authority_id", source_intake_authority_id),
			("source_provenance_fingerprint", source_provenance_fingerprint),
			("transfer_causation_event_id", transfer_causation_event_id),
		],
	};
	for (field, value) in values {
		if value.trim().is_empty() {
			eyre::bail!("Intake authority `{field}` cannot be empty.");
		}
	}
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::lane_authority::{ProjectBinding, ProjectBindingAttestation};

	fn attestation() -> ProjectBindingAttestation {
		let binding = ProjectBinding::new(
			"pubfi",
			"helixbox",
			"pubfi-mono",
			"team",
			"decodex:queued:pubfi",
			"binding",
		)
		.expect("binding");
		ProjectBindingAttestation::new(&binding)
	}

	fn issue_batch_authority() -> IntakeAuthority {
		IntakeAuthority::new(
			"authority-1",
			"pubfi",
			attestation(),
			"plan-1",
			"program-1",
			"operator",
			"cli",
			"correlation-1",
			"2026-07-12T00:00:00Z",
			1,
			IntakeAuthorityKind::IssueBatch {
				accepted_intake_id: String::from("batch-1"),
				batch_fingerprint: String::from("batch-fingerprint"),
			},
		)
		.expect("authority")
	}

	#[test]
	fn issue_batch_is_typed_without_contract_id() {
		let authority = issue_batch_authority();
		authority.validate().expect("valid authority");
		assert!(matches!(authority.authority(), IntakeAuthorityKind::IssueBatch { .. }));
	}

	#[test]
	fn recovery_adoption_requires_explicit_evidence() {
		let authority = IntakeAuthority::new(
			"authority-recovery-1",
			"pubfi",
			attestation(),
			"recovery-plan-1",
			"recovery-program-1",
			"operator",
			"review_handoff_adopt",
			"correlation-recovery-1",
			"2026-07-12T00:00:00Z",
			1,
			IntakeAuthorityKind::RecoveryAdoption {
				recovery_request_id: String::from("adopt-run-1"),
				evidence_fingerprint: String::from("sha256:evidence"),
			},
		)
		.expect("recovery authority");
		authority.validate().expect("valid recovery authority");
		assert_eq!(authority.authority().as_str(), "recovery_adoption");
	}

	#[test]
	fn fingerprint_detects_tampering() {
		let mut authority = issue_batch_authority();
		authority.program_id = String::from("tampered");
		assert!(authority.validate().is_err());
	}
}
