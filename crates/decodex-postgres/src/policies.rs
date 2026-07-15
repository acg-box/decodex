use std::{collections::BTreeMap, fmt::Display};

use serde_json::{Map, Value};
use tokio_postgres::{Error, Row, error::DbError};

use crate::{PostgresStore, StoreError};
use decodex_core::{
	AcceptedPolicyRevision, AgentId, Policy, PolicyId, PolicyProvenance, PolicyRepository,
	PolicyRevision, PolicyRevisionAcceptance, PolicyRevisionId, PolicySnapshot,
	PolicySnapshotValue, PolicyTimestamp, ProjectId,
};

impl PostgresStore {
	/// Idempotently create one Project-owned inert Policy identity.
	pub async fn create_policy(
		&self,
		id: PolicyId,
		project_id: ProjectId,
	) -> Result<Policy, StoreError> {
		let client = crate::checkout(self.pool(), &self.connector).await?;
		let row = client
			.query_one(
				"SELECT policy_id::text,project_id::text,\
				 (EXTRACT(EPOCH FROM created_at)*1000000)::bigint,current_revision \
				 FROM decodex.create_policy(\
				 $1::pg_catalog.text::decodex.canonical_uuid_v4_text,\
				 $2::pg_catalog.text::decodex.canonical_uuid_v4_text)",
				&[&id.as_str(), &project_id.as_str()],
			)
			.await
			.map_err(policy_database_error)?;

		decode_policy(&row)
	}

	/// Accept one exact immutable Policy revision under database-verified Lead authority.
	pub async fn accept_policy_revision(
		&self,
		acceptance: PolicyRevisionAcceptance,
	) -> Result<AcceptedPolicyRevision, StoreError> {
		acceptance.validate().map_err(|_| {
			StoreError::InvalidInput("Policy acceptance requires exact immediate supersession")
		})?;

		let revision = i64::try_from(acceptance.id.revision().get())
			.map_err(|_| StoreError::InvalidInput("Policy revision exceeds PostgreSQL bigint"))?;
		let supersedes_revision = acceptance
			.supersedes
			.as_ref()
			.map(|id| i64::try_from(id.revision().get()))
			.transpose()
			.map_err(|_| StoreError::InvalidInput("Policy revision exceeds PostgreSQL bigint"))?;
		let snapshot = encode_snapshot(&acceptance.snapshot);

		crate::ensure_credential_negative_text(acceptance.provenance.as_str())?;
		crate::ensure_credential_negative_json(&snapshot)?;

		let mut client = crate::checkout(self.pool(), &self.connector).await?;
		let transaction = client.transaction().await?;
		let row = transaction
			.query_one(
				"SELECT policy_id::text,project_id::text,revision,provenance,snapshot,\
				 accepted_by::text,(EXTRACT(EPOCH FROM policy_created_at)*1000000)::bigint,\
				 (EXTRACT(EPOCH FROM accepted_at)*1000000)::bigint,supersedes_revision,\
				 revision_accepted,actual_revision \
				 FROM decodex.accept_policy_revision(\
				 $1::pg_catalog.text::decodex.canonical_uuid_v4_text,\
				 $2::pg_catalog.text::decodex.canonical_uuid_v4_text,$3,$4,$5,\
				 $6::pg_catalog.text::decodex.canonical_uuid_v4_text,$7)",
				&[
					&acceptance.id.policy_id().as_str(),
					&acceptance.id.project_id().as_str(),
					&revision,
					&acceptance.provenance.as_str(),
					&snapshot,
					&acceptance.accepted_by.as_str(),
					&supersedes_revision,
				],
			)
			.await
			.map_err(policy_acceptance_database_error)?;
		let revision_accepted: bool = row.get(9);

		if !revision_accepted {
			let conflict = StoreError::RevisionConflict {
				entity: acceptance.id.policy_id().to_string(),
				expected: Some(revision),
				actual: row.get(10),
			};

			transaction.rollback().await?;

			return Err(conflict);
		}

		let accepted = decode_accepted_revision(&row)?;

		transaction.commit().await?;

		Ok(accepted)
	}

	/// Read one exact immutable Project-owned Policy revision.
	pub async fn policy_revision(
		&self,
		id: &PolicyRevisionId,
	) -> Result<Option<AcceptedPolicyRevision>, StoreError> {
		let revision = i64::try_from(id.revision().get())
			.map_err(|_| StoreError::InvalidInput("Policy revision exceeds PostgreSQL bigint"))?;
		let client = crate::checkout(self.pool(), &self.connector).await?;
		let row = client
			.query_opt(
				"SELECT revision.policy_id::text,revision.project_id::text,revision.revision,\
				 revision.provenance,revision.snapshot,revision.accepted_by::text,\
				 (EXTRACT(EPOCH FROM policy.created_at)*1000000)::bigint,\
				 (EXTRACT(EPOCH FROM revision.accepted_at)*1000000)::bigint,\
				 revision.supersedes_revision \
				 FROM decodex.policy_revisions AS revision \
				 JOIN decodex.policies AS policy ON policy.policy_id=revision.policy_id \
				 WHERE revision.project_id=$1::text::uuid AND revision.policy_id=$2::text::uuid \
				 AND revision.revision=$3",
				&[&id.project_id().as_str(), &id.policy_id().as_str(), &revision],
			)
			.await?;

		row.as_ref().map(decode_accepted_revision).transpose()
	}

	/// List Policy identities for one Project in stable identity order.
	pub async fn policies_for_project(
		&self,
		project_id: &ProjectId,
	) -> Result<Vec<Policy>, StoreError> {
		let client = crate::checkout(self.pool(), &self.connector).await?;
		let rows = client
			.query(
				"SELECT policy_id::text,project_id::text,\
				 (EXTRACT(EPOCH FROM created_at)*1000000)::bigint,current_revision \
				 FROM decodex.policies WHERE project_id=$1::text::uuid ORDER BY policy_id",
				&[&project_id.as_str()],
			)
			.await?;

		rows.iter().map(decode_policy).collect()
	}
}

impl PolicyRepository for PostgresStore {
	type Error = StoreError;

	async fn create_policy(
		&self,
		id: PolicyId,
		project_id: ProjectId,
	) -> Result<Policy, Self::Error> {
		Self::create_policy(self, id, project_id).await
	}

	async fn accept_policy_revision(
		&self,
		acceptance: PolicyRevisionAcceptance,
	) -> Result<AcceptedPolicyRevision, Self::Error> {
		Self::accept_policy_revision(self, acceptance).await
	}

	async fn policy_revision(
		&self,
		id: &PolicyRevisionId,
	) -> Result<Option<AcceptedPolicyRevision>, Self::Error> {
		Self::policy_revision(self, id).await
	}

	async fn policies_for_project(
		&self,
		project_id: &ProjectId,
	) -> Result<Vec<Policy>, Self::Error> {
		Self::policies_for_project(self, project_id).await
	}
}

fn decode_policy(row: &Row) -> Result<Policy, StoreError> {
	let current_revision = row
		.get::<_, Option<i64>>(3)
		.map(|value| {
			u64::try_from(value)
				.map_err(|_| incompatible())
				.and_then(|value| PolicyRevision::new(value).map_err(incompatible_core))
		})
		.transpose()?;

	Ok(Policy::from_stored(
		PolicyId::new(row.get::<_, String>(0)).map_err(incompatible_core)?,
		ProjectId::new(row.get::<_, String>(1)).map_err(incompatible_core)?,
		PolicyTimestamp::from_unix_microseconds(row.get(2)).map_err(incompatible_core)?,
		current_revision,
	))
}

fn decode_accepted_revision(row: &Row) -> Result<AcceptedPolicyRevision, StoreError> {
	let project_id = ProjectId::new(row.get::<_, String>(1)).map_err(incompatible_core)?;
	let policy_id = PolicyId::new(row.get::<_, String>(0)).map_err(incompatible_core)?;
	let revision =
		PolicyRevision::new(u64::try_from(row.get::<_, i64>(2)).map_err(|_| incompatible())?)
			.map_err(incompatible_core)?;
	let supersedes = row
		.get::<_, Option<i64>>(8)
		.map(|value| {
			let revision = PolicyRevision::new(u64::try_from(value).map_err(|_| incompatible())?)
				.map_err(incompatible_core)?;

			Ok::<_, StoreError>(PolicyRevisionId::new(
				project_id.clone(),
				policy_id.clone(),
				revision,
			))
		})
		.transpose()?;
	let acceptance = PolicyRevisionAcceptance {
		id: PolicyRevisionId::new(project_id, policy_id, revision),
		provenance: PolicyProvenance::new(row.get::<_, String>(3)).map_err(incompatible_core)?,
		snapshot: decode_snapshot(row.get(4))?,
		accepted_by: AgentId::new(row.get::<_, String>(5)).map_err(incompatible_core)?,
		supersedes,
	};

	AcceptedPolicyRevision::from_stored(
		acceptance,
		PolicyTimestamp::from_unix_microseconds(row.get(6)).map_err(incompatible_core)?,
		PolicyTimestamp::from_unix_microseconds(row.get(7)).map_err(incompatible_core)?,
	)
	.map_err(incompatible_core)
}

fn encode_snapshot(snapshot: &PolicySnapshot) -> Value {
	Value::Object(
		snapshot
			.as_map()
			.iter()
			.map(|(key, value)| {
				let value = match value {
					PolicySnapshotValue::Text(value) => Value::String(value.clone()),
					PolicySnapshotValue::Boolean(value) => Value::Bool(*value),
				};

				(key.clone(), value)
			})
			.collect::<Map<_, _>>(),
	)
}

fn decode_snapshot(value: Value) -> Result<PolicySnapshot, StoreError> {
	let Value::Object(value) = value else { return Err(incompatible()) };
	let values = value
		.into_iter()
		.map(|(key, value)| {
			let value = match value {
				Value::String(value) => PolicySnapshotValue::Text(value),
				Value::Bool(value) => PolicySnapshotValue::Boolean(value),
				_ => return Err(incompatible()),
			};

			Ok((key, value))
		})
		.collect::<Result<BTreeMap<_, _>, StoreError>>()?;

	PolicySnapshot::new(values).map_err(incompatible_core)
}

fn incompatible_core(error: impl Display) -> StoreError {
	StoreError::Incompatible(format!("invalid stored Project policy authority: {error}"))
}

fn incompatible() -> StoreError {
	StoreError::Incompatible("invalid stored Project policy authority".into())
}

fn policy_database_error(error: Error) -> StoreError {
	match error.as_db_error().and_then(DbError::constraint) {
		Some("policies_identity_project") =>
			StoreError::InvalidInput("Policy identity is already bound to another Project"),
		_ => StoreError::from(error),
	}
}

fn policy_acceptance_database_error(error: Error) -> StoreError {
	match error.as_db_error().and_then(DbError::constraint) {
		Some("policy_revisions_conflicting_replay") => StoreError::IdempotencyConflict,
		Some("policy_revisions_project_scope") =>
			StoreError::InvalidInput("Policy revision cannot attach across Projects"),
		Some("policy_revisions_accepting_authority") | Some("policy_revisions_active_project") =>
			StoreError::InvalidInput("Policy acceptance requires active Project Lead authority"),
		_ => StoreError::from(error),
	}
}
