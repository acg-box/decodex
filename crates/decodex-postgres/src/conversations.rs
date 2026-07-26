//! Logical Conversation, RuntimeSession, bounded history, blob, and Context Pack persistence.
//!
//! This owner intentionally exposes no account selection, process start, Codex dispatch,
//! rollover executor, fallback executor, or wake scheduler.

use std::time::Duration;
#[cfg(debug_assertions)] use std::{env, fs, path::PathBuf};

use deadpool_postgres::{Client, ClientWrapper};
use serde_json::{self, Value};
#[cfg(debug_assertions)] use tokio::time;
use tokio_postgres::{Row, Transaction, error::SqlState};

use crate::{
	CommandIdentity, PostgresStore, StoreError,
	accounts::{self, CommandClaim, CommandDescriptor},
};
use decodex_core::{
	self, ArtifactId, ArtifactStatus, BlobHash, BlobInventoryCursor, BlobStore, ContextPack,
	ContextPackPolicy, ContextSourceDisposition, ContextSourceKind, ConversationId, HistoryItemId,
	HistoryItemKind, HistoryMediaType, HistoryMetadata, ItemStatus, MAX_BLOB_BYTES,
	MAX_INLINE_HISTORY_BYTES, PossibleSideEffects, ProposedTransitionKind, RuntimeSessionId,
	TurnId, TurnRole,
};

const MAX_PAGE_SIZE: u16 = 100;
const HIERARCHY_COORDINATION_LOCK: i64 = 1_271;
const CURSOR_COORDINATION_LOCK: i64 = 1_272;
const BLOB_LOCK_NAMESPACE: i32 = 1_273;
const BLOB_SHARD_LOCK_NAMESPACE: i32 = 1_274;

/// Create a logical Conversation without any account or Codex-thread identity.
#[derive(Clone, Debug)]
pub struct CreateConversation {
	/// Caller-selected logical identity.
	pub conversation_id: ConversationId,
	/// Bounded display title.
	pub title: String,
}

/// Committed logical Conversation readback.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredConversation {
	/// Stable logical identity.
	pub conversation_id: ConversationId,
	/// Persisted title.
	pub title: String,
	/// Persisted optimistic revision.
	pub revision: i64,
}

/// Create one immutable-content Artifact scoped to a logical Conversation.
#[derive(Clone, Debug)]
pub struct CreateArtifact {
	/// Caller-selected stable Artifact identity.
	pub artifact_id: ArtifactId,
	/// Owning logical Conversation.
	pub conversation_id: ConversationId,
	/// Immutable bounded content bytes.
	pub bytes: Vec<u8>,
	/// Canonical bounded media type.
	pub media_type: String,
	/// Optional bounded display name.
	pub display_name: Option<String>,
}

/// Complete verified Artifact revision read model.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredArtifact {
	/// Stable Artifact identity.
	pub artifact_id: ArtifactId,
	/// Owning logical Conversation.
	pub conversation_id: ConversationId,
	/// Exact immutable revision.
	pub revision: i64,
	/// Lifecycle state recorded by this revision.
	pub status: ArtifactStatus,
	/// Verified content address.
	pub blob_hash: BlobHash,
	/// Verified exact content bytes.
	pub bytes: Vec<u8>,
	/// Persisted media type.
	pub media_type: String,
	/// Persisted optional display name.
	pub display_name: Option<String>,
}

/// One normalized streamed or completed history-item mutation.
#[derive(Clone, Debug)]
pub struct RecordHistoryItem {
	/// Parent logical Conversation.
	pub conversation_id: ConversationId,
	/// Producing runtime segment.
	pub runtime_session_id: RuntimeSessionId,
	/// Parent turn identity.
	pub turn_id: TurnId,
	/// Logical turn ordering key.
	pub turn_sequence: i64,
	/// Normalized turn role.
	pub turn_role: TurnRole,
	/// Explicit side-effect uncertainty.
	pub possible_side_effects: PossibleSideEffects,
	/// Stable normalized item identity.
	pub history_item_id: HistoryItemId,
	/// Stable position within the turn.
	pub ordinal: i32,
	/// Normalized item class.
	pub kind: HistoryItemKind,
	/// Stream lifecycle.
	pub status: ItemStatus,
	/// Exact normalized UTF-8 payload, offloaded when large.
	pub text: String,
	/// Bounded media type.
	pub media_type: HistoryMediaType,
	/// Bounded credential-negative structured metadata.
	pub metadata: HistoryMetadata,
	/// `None` creates revision 1; `Some` updates only that exact item revision.
	pub expected_revision: Option<i64>,
	/// Exact typed Artifact revision for artifact history items.
	pub artifact: Option<(ArtifactId, i64)>,
}

/// Opaque versioned handle to one PostgreSQL-issued Conversation snapshot boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoryCursor {
	/// Random persisted issuance identity; no boundary metadata is client-editable.
	token: String,
}
impl HistoryCursor {
	/// Encode the opaque cursor for the typed protocol.
	pub fn encode(&self) -> String {
		format!("v1:{}", self.token)
	}

	/// Parse only the versioned opaque shape; PostgreSQL validates issuance and binding.
	pub fn parse(value: &str) -> Result<Self, StoreError> {
		let Some(token) = value.strip_prefix("v1:") else {
			return Err(StoreError::InvalidInput("history cursor is malformed"));
		};

		if !is_canonical_uuid(token) {
			return Err(StoreError::InvalidInput("history cursor is malformed"));
		}

		Ok(Self { token: token.to_owned() })
	}

	fn issued(token: String) -> Result<Self, StoreError> {
		if is_canonical_uuid(&token) {
			Ok(Self { token })
		} else {
			Err(StoreError::Incompatible("issued history cursor identity is invalid".into()))
		}
	}
}

/// Verified persisted normalized history read model.
#[derive(Clone, Debug, PartialEq)]
pub struct HistoryEntry {
	/// Stable item identity.
	pub history_item_id: String,
	/// Parent turn identity.
	pub turn_id: String,
	/// Producing runtime segment identity.
	pub runtime_session_id: String,
	/// Normalized author role.
	pub turn_role: TurnRole,
	/// Explicit side-effect uncertainty.
	pub possible_side_effects: PossibleSideEffects,
	/// Normalized item class.
	pub kind: HistoryItemKind,
	/// Stream lifecycle.
	pub status: ItemStatus,
	/// Inline payload when small.
	pub inline_text: Option<String>,
	/// Content address when offloaded.
	pub blob_hash: Option<BlobHash>,
	/// Verified offloaded length.
	pub blob_byte_length: Option<u64>,
	/// Persisted media type.
	pub media_type: HistoryMediaType,
	/// Persisted bounded metadata.
	pub metadata: HistoryMetadata,
	/// Exact typed Artifact revision for Artifact history items.
	pub artifact: Option<(ArtifactId, u64)>,
	/// Persisted optimistic revision.
	pub revision: i64,
}

/// One strictly bounded deterministic history page.
#[derive(Clone, Debug, PartialEq)]
pub struct HistoryPage {
	/// Ordered verified entries.
	pub entries: Vec<HistoryEntry>,
	/// Cursor for the next page only when more rows exist.
	pub next_cursor: Option<HistoryCursor>,
}

/// Result of one bounded deterministic blob-inventory reclamation pass.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlobReclaimPage {
	/// Files removed in this pass.
	pub removed: u16,
	/// Continuation required to cover the complete SHA-256 namespace.
	pub next_cursor: Option<BlobInventoryCursor>,
}

/// Persist one already compiled immutable Context Pack and its exact source revisions.
#[derive(Clone, Debug)]
pub struct PersistContextPack {
	/// Caller-selected pack identity.
	pub context_pack_id: String,
	/// Immutable revision within the Conversation.
	pub pack_revision: i64,
}

/// Verified committed Context Pack metadata.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextPackRecord {
	/// Stable pack identity.
	pub context_pack_id: String,
	/// Parent logical Conversation.
	pub conversation_id: ConversationId,
	/// Immutable pack revision.
	pub pack_revision: i64,
	/// Digest of exact compiled bytes.
	pub compiled_digest: BlobHash,
	/// Verified compiled length.
	pub byte_length: u64,
	/// Whether deterministic policy shortened any source.
	pub truncated: bool,
	/// Count of source inputs omitted entirely.
	pub omitted_source_count: usize,
	/// Completely reconstructed, byte- and manifest-verified immutable pack.
	pub pack: ContextPack,
}

/// Persist an inert rollover/fallback proposal. `dispatch_enabled` is fixed false in schema.
#[derive(Clone, Debug)]
pub struct ProposeTransition {
	/// Caller-selected proposal identity.
	pub transition_id: String,
	/// Logical Conversation preserved by the proposal.
	pub conversation_id: ConversationId,
	/// Runtime segment the proposal would replace.
	pub from_runtime_session_id: RuntimeSessionId,
	/// Exact persisted Context Pack identity.
	pub context_pack_id: String,
	/// Proposed transition class.
	pub kind: ProposedTransitionKind,
	/// Bounded operator-inspectable rationale.
	pub reason: String,
}

pub(crate) struct BlobSession {
	client: ClientWrapper,
}

impl PostgresStore {
	async fn dedicated_session(&self) -> Result<BlobSession, StoreError> {
		let client = self.pool().get().await?;

		Ok(BlobSession { client: Client::take(client) })
	}

	pub(crate) async fn lock_blob_session(
		&self,
		hashes: &[BlobHash],
		capacity_hashes: &[BlobHash],
	) -> Result<BlobSession, StoreError> {
		let session = self.dedicated_session().await?;
		let mut encoded_hashes = hashes.iter().map(|hash| hash.to_hex()).collect::<Vec<_>>();

		encoded_hashes.sort_unstable();
		encoded_hashes.dedup();

		for encoded in &encoded_hashes {
			session
				.client
				.query_one(
					"SELECT pg_catalog.pg_advisory_lock($1, pg_catalog.hashtext($2))",
					&[&BLOB_LOCK_NAMESPACE, encoded],
				)
				.await?;
		}

		let mut shards = capacity_hashes
			.iter()
			.map(|hash| {
				let encoded = hash.to_hex();

				i32::from_str_radix(&encoded[..2], 16)
					.map_err(|_| StoreError::Incompatible("blob shard identity is invalid".into()))
			})
			.collect::<Result<Vec<_>, _>>()?;

		shards.sort_unstable();
		shards.dedup();

		for shard in shards {
			session
				.client
				.query_one(
					"SELECT pg_catalog.pg_advisory_lock($1, $2)",
					&[&BLOB_SHARD_LOCK_NAMESPACE, &shard],
				)
				.await?;
		}

		Ok(session)
	}

	async fn history_artifact_hashes(
		&self,
		mutation: &RecordHistoryItem,
	) -> Result<Vec<BlobHash>, StoreError> {
		let Some((artifact_id, revision)) = &mutation.artifact else {
			return Ok(Vec::new());
		};
		let row = self
			.pool()
			.get()
			.await?
			.query_opt(
				"SELECT blob_hash FROM decodex.artifact_revisions \
				 WHERE artifact_id=$1::text::uuid AND conversation_id=$2::text::uuid AND revision=$3",
				&[&artifact_id.as_str(), &mutation.conversation_id.as_str(), revision],
			)
			.await?
			.ok_or(StoreError::InvalidInput("Artifact history reference does not exist"))?;

		Ok(vec![BlobHash::parse(row.get(0))?])
	}

	/// Atomically create one logical Conversation with activity, outbox, and receipt evidence.
	pub async fn create_conversation(
		&self,
		command: &CommandIdentity,
		create: &CreateConversation,
	) -> Result<StoredConversation, StoreError> {
		if create.title.is_empty() || create.title.len() > 512 {
			return Err(StoreError::InvalidInput("conversation title must contain 1..=512 bytes"));
		}

		crate::ensure_credential_negative_text(&create.title)?;

		let mut client = self.pool().get().await?;
		let reservation = match reserve_conversation_command(
			&mut client,
			command,
			"create_conversation",
			("global", "conversations", create.conversation_id.as_str()),
			None,
			None,
		)
		.await?
		{
			accounts::CommandClaim::Completed(response) => {
				return conversation_from_response(&response);
			},
			accounts::CommandClaim::Owned(reservation) => reservation,
		};
		let transaction = client.transaction().await?;
		let inserted = transaction
			.query_opt(
				"INSERT INTO decodex.conversations (conversation_id, title) \
				 VALUES ($1::text::uuid, $2) ON CONFLICT DO NOTHING RETURNING revision",
				&[&create.conversation_id.as_str(), &create.title],
			)
			.await?;

		if inserted.is_none() {
			return Err(StoreError::RevisionConflict {
				entity: format!("conversation/{}", create.conversation_id),
				expected: None,
				actual: conversation_revision(&transaction, &create.conversation_id).await?,
			});
		}

		let payload = serde_json::json!({
			"conversation_id": create.conversation_id.as_str(),
			"revision": 1,
		});

		accounts::append_activity_and_outbox(
			&transaction,
			"conversation",
			create.conversation_id.as_str(),
			1,
			"conversation_created",
			&command.key,
			&payload,
		)
		.await?;

		let response = serde_json::json!({
			"kind": "conversation",
			"conversation_id": create.conversation_id.as_str(),
			"title": create.title,
			"revision": 1,
		});

		accounts::finish_command(&transaction, &reservation, &response).await?;

		transaction.commit().await?;

		conversation_from_response(&response)
	}

	/// Complete or fail one active normalized Turn after its items are terminal.
	pub async fn transition_turn(
		&self,
		command: &CommandIdentity,
		turn_id: &TurnId,
		expected_revision: i64,
		status: decodex_core::TurnStatus,
	) -> Result<i64, StoreError> {
		if expected_revision < 1 || status == decodex_core::TurnStatus::Active {
			return Err(StoreError::InvalidInput("Turn transition is invalid"));
		}

		let mut client = self.pool().get().await?;
		let reservation = match reserve_conversation_command(
			&mut client,
			command,
			"transition_turn",
			("turn", turn_id.as_str(), turn_id.as_str()),
			Some(expected_revision),
			None,
		)
		.await?
		{
			accounts::CommandClaim::Completed(response) => {
				return response_revision(&response, "turn");
			},
			accounts::CommandClaim::Owned(reservation) => reservation,
		};
		let transaction = client.transaction().await?;
		let state = match status {
			decodex_core::TurnStatus::Completed => "completed",
			decodex_core::TurnStatus::Failed => "failed",
			decodex_core::TurnStatus::Active => unreachable!(),
		};
		let row=transaction.query_opt("UPDATE decodex.turns SET status=$3::text::decodex.turn_status, revision=revision+1, updated_at=clock_timestamp(), completed_at=clock_timestamp() WHERE turn_id=$1::text::uuid AND revision=$2 AND NOT EXISTS (SELECT 1 FROM decodex.history_items WHERE turn_id=$1::text::uuid AND status='streaming') RETURNING revision, conversation_id::text",&[&turn_id.as_str(),&expected_revision,&state]).await?.ok_or(StoreError::RevisionConflict{entity:format!("turn/{turn_id}"),expected:Some(expected_revision),actual:None})?;
		let revision: i64 = row.get(0);
		let conversation_id: String = row.get(1);

		accounts::append_activity_and_outbox(
			&transaction,
			"turn",
			turn_id.as_str(),
			revision,
			"turn_transitioned",
			&command.key,
			&serde_json::json!({"conversation_id":conversation_id,"status":state,"revision":revision}),
		)
		.await?;
		accounts::finish_command(
			&transaction,
			&reservation,
			&serde_json::json!({"kind":"turn","turn_id":turn_id.as_str(),"revision":revision}),
		)
		.await?;

		transaction.commit().await?;

		Ok(revision)
	}

	/// Transactionally publish and reference one immutable Artifact revision.
	pub async fn create_artifact(
		&self,
		blob_store: &BlobStore,
		command: &CommandIdentity,
		create: &CreateArtifact,
	) -> Result<StoredArtifact, StoreError> {
		validate_artifact(create)?;

		let hash = BlobHash::digest(&create.bytes);
		let byte_length = i64::try_from(create.bytes.len())
			.map_err(|_| StoreError::InvalidInput("Artifact blob length is invalid"))?;
		let mut client = self.pool().get().await?;
		let reservation = match reserve_conversation_command(
			&mut client,
			command,
			"create_artifact",
			("conversation", create.conversation_id.as_str(), create.artifact_id.as_str()),
			None,
			Some((hash, byte_length)),
		)
		.await?
		{
			accounts::CommandClaim::Completed(response) => {
				let revision = response_revision(&response, "artifact")?;

				return self.artifact(blob_store, &create.artifact_id, Some(revision)).await;
			},
			accounts::CommandClaim::Owned(reservation) => reservation,
		};

		drop(client);

		let mut publication = self.lock_blob_session(&[hash], &[hash]).await?;

		publish_verified_blob(blob_store, hash, &create.bytes)?;
		blob_publish_test_barrier().await?;

		let transaction = publication.client.transaction().await?;

		insert_verified_blob(&transaction, hash, byte_length).await?;

		transaction.execute(
			"INSERT INTO decodex.artifacts (artifact_id, conversation_id) VALUES ($1::text::uuid, $2::text::uuid)",
			&[&create.artifact_id.as_str(), &create.conversation_id.as_str()],
		).await?;
		transaction.execute(
			"INSERT INTO decodex.artifact_revisions (artifact_id, conversation_id, revision, blob_hash, media_type, display_name, status) VALUES ($1::text::uuid, $2::text::uuid, 1, $3, $4, $5, 'active')",
			&[&create.artifact_id.as_str(), &create.conversation_id.as_str(), &hash.to_hex(), &create.media_type, &create.display_name],
		).await?;

		accounts::append_activity_and_outbox(&transaction, "artifact", create.artifact_id.as_str(), 1, "artifact_created", &command.key,
			&serde_json::json!({"conversation_id": create.conversation_id.as_str(), "blob_hash": hash.to_hex(), "revision": 1})).await?;
		accounts::finish_command(
			&transaction,
			&reservation,
			&serde_json::json!({"kind":"artifact","artifact_id":create.artifact_id.as_str(),"revision":1}),
		)
		.await?;

		transaction.commit().await?;

		self.artifact(blob_store, &create.artifact_id, Some(1)).await
	}

	/// Apply one legal Artifact lifecycle transition and retain an immutable revision row.
	pub async fn transition_artifact(
		&self,
		blob_store: &BlobStore,
		command: &CommandIdentity,
		artifact_id: &ArtifactId,
		expected_revision: i64,
		status: ArtifactStatus,
	) -> Result<StoredArtifact, StoreError> {
		if expected_revision < 1 || status == ArtifactStatus::Active {
			return Err(StoreError::InvalidInput("Artifact transition is invalid"));
		}

		let mut client = self.pool().get().await?;
		let reservation = match reserve_conversation_command(
			&mut client,
			command,
			"transition_artifact",
			("artifact", artifact_id.as_str(), artifact_id.as_str()),
			Some(expected_revision),
			None,
		)
		.await?
		{
			accounts::CommandClaim::Completed(response) => {
				let revision = response_revision(&response, "artifact")?;

				return self.artifact(blob_store, artifact_id, Some(revision)).await;
			},
			accounts::CommandClaim::Owned(reservation) => reservation,
		};
		let transaction = client.transaction().await?;
		let row = transaction.query_opt(
			"UPDATE decodex.artifacts SET status=$3::text::decodex.artifact_status, revision=revision+1, updated_at=clock_timestamp() WHERE artifact_id=$1::text::uuid AND revision=$2 RETURNING conversation_id::text, revision",
			&[&artifact_id.as_str(), &expected_revision, &artifact_status_sql(status)],
		).await?.ok_or(StoreError::RevisionConflict { entity: format!("artifact/{artifact_id}"), expected: Some(expected_revision), actual: None })?;
		let conversation_id: String = row.get(0);
		let revision: i64 = row.get(1);

		transaction.execute(
			"INSERT INTO decodex.artifact_revisions (artifact_id, conversation_id, revision, blob_hash, media_type, display_name, status) SELECT artifact_id, conversation_id, $2, blob_hash, media_type, display_name, $3::text::decodex.artifact_status FROM decodex.artifact_revisions WHERE artifact_id=$1::text::uuid AND revision=$4",
			&[&artifact_id.as_str(), &revision, &artifact_status_sql(status), &expected_revision],
		).await?;

		accounts::append_activity_and_outbox(&transaction, "artifact", artifact_id.as_str(), revision, "artifact_transitioned", &command.key,
			&serde_json::json!({"conversation_id":conversation_id,"status":artifact_status_sql(status),"revision":revision})).await?;
		accounts::finish_command(
			&transaction,
			&reservation,
			&serde_json::json!({"kind":"artifact","artifact_id":artifact_id.as_str(),"revision":revision}),
		)
		.await?;

		transaction.commit().await?;

		self.artifact(blob_store, artifact_id, Some(revision)).await
	}

	/// Read and verify one exact or current Artifact revision.
	pub async fn artifact(
		&self,
		blob_store: &BlobStore,
		artifact_id: &ArtifactId,
		revision: Option<i64>,
	) -> Result<StoredArtifact, StoreError> {
		let row = self
			.pool()
			.get()
			.await?
			.query_opt(
				"SELECT ar.artifact_id::text, ar.conversation_id::text, ar.revision, ar.status::text, \
			 ar.blob_hash, ar.media_type, ar.display_name, bo.byte_length \
			 FROM decodex.artifact_revisions ar JOIN decodex.artifacts a ON a.artifact_id=ar.artifact_id \
			 JOIN decodex.blob_objects bo ON bo.blob_hash=ar.blob_hash \
			 WHERE ar.artifact_id=$1::text::uuid AND ar.revision=COALESCE($2,a.revision)",
				&[&artifact_id.as_str(), &revision],
			)
			.await?
			.ok_or(StoreError::InvalidInput("Artifact revision does not exist"))?;
		let hash = BlobHash::parse(row.get(4))?;
		let bytes = blob_store.read(hash)?;

		if i64::try_from(bytes.len()).ok() != Some(row.get(7)) {
			return Err(StoreError::Incompatible(
				"Artifact blob length differs from committed metadata".into(),
			));
		}

		let media_type: String = row.get(5);

		if !decodex_core::is_canonical_media_type(&media_type) {
			return Err(StoreError::Incompatible("stored Artifact media type is invalid".into()));
		}

		Ok(StoredArtifact {
			artifact_id: ArtifactId::new(row.get::<_, String>(0)).map_err(|_| {
				StoreError::Incompatible("stored Artifact identity is invalid".into())
			})?,
			conversation_id: ConversationId::new(row.get::<_, String>(1)).map_err(|_| {
				StoreError::Incompatible("stored Conversation identity is invalid".into())
			})?,
			revision: row.get(2),
			status: artifact_status_from_sql(row.get(3))?,
			blob_hash: hash,
			bytes,
			media_type,
			display_name: row.get(6),
		})
	}

	/// Persist a normalized history item. Blob bytes are atomically published and fully verified
	/// before PostgreSQL metadata/reference commit. A crash can therefore leave only an
	/// unreferenced content-addressed blob; it cannot commit a reference before publication.
	pub async fn record_history_item(
		&self,
		blob_store: &BlobStore,
		command: &CommandIdentity,
		mutation: &RecordHistoryItem,
	) -> Result<HistoryEntry, StoreError> {
		validate_history_item(mutation)?;

		let blob = prepare_payload(&mutation.text)?;
		let mut client = self.pool().get().await?;
		let reservation = match reserve_conversation_command(
			&mut client,
			command,
			"record_history_item",
			("conversation", mutation.conversation_id.as_str(), mutation.history_item_id.as_str()),
			mutation.expected_revision,
			blob,
		)
		.await?
		{
			accounts::CommandClaim::Completed(response) => {
				let entry = history_entry_from_response(&response)?;

				self.verify_history_entry(blob_store, &entry).await?;

				return Ok(entry);
			},
			accounts::CommandClaim::Owned(reservation) => reservation,
		};

		drop(client);

		let capacity_hashes = blob.map(|(hash, _)| vec![hash]).unwrap_or_default();
		let mut referenced_hashes = self.history_artifact_hashes(mutation).await?;

		if let Some((hash, _)) = blob {
			referenced_hashes.push(hash);
		}

		let mut publication = if !referenced_hashes.is_empty() {
			let publication = self.lock_blob_session(&referenced_hashes, &capacity_hashes).await?;

			if let Some((hash, _)) = blob {
				publish_verified_blob(blob_store, hash, mutation.text.as_bytes())?;
			}

			publication
		} else {
			self.dedicated_session().await?
		};
		let transaction = publication.client.transaction().await?;

		if let Some((hash, byte_length)) = blob {
			insert_verified_blob(&transaction, hash, byte_length).await?;
		}

		ensure_turn(&transaction, mutation).await?;

		let revision = match mutation.expected_revision {
			None => insert_history_item(&transaction, mutation, blob).await?,
			Some(expected) => update_history_item(&transaction, mutation, blob, expected).await?,
		};
		let payload = serde_json::json!({
			"conversation_id": mutation.conversation_id.as_str(),
			"runtime_session_id": mutation.runtime_session_id.as_str(),
			"turn_id": mutation.turn_id.as_str(),
			"history_item_id": mutation.history_item_id.as_str(),
			"status": item_status_sql(mutation.status),
			"blob_hash": blob.map(|(hash, _)| hash.to_hex()),
			"revision": revision,
		});

		accounts::append_activity_and_outbox(
			&transaction,
			"history_item",
			mutation.history_item_id.as_str(),
			revision,
			if revision == 1 { "history_item_recorded" } else { "history_item_updated" },
			&command.key,
			&payload,
		)
		.await?;

		let response = history_entry_response(mutation, blob, revision)?;

		accounts::finish_command(&transaction, &reservation, &response).await?;

		transaction.commit().await?;

		post_commit_test_barrier("history_item").await?;

		let entry = history_entry_from_response(&response)?;

		self.verify_history_entry(blob_store, &entry).await?;

		Ok(entry)
	}

	/// Remove a bounded inventory of grace-aged filesystem blobs that PostgreSQL proves have
	/// no committed reference. Metadata deletion commits before byte removal; collection then
	/// reacquires the writer lock and rechecks references, so every crash residue remains an
	/// inventory-visible content-addressed orphan rather than metadata whose bytes disappeared.
	pub async fn reclaim_orphan_blobs(
		&self,
		blob_store: &BlobStore,
		grace: Duration,
		limit: u16,
		after: Option<BlobInventoryCursor>,
	) -> Result<BlobReclaimPage, StoreError> {
		if limit == 0 || limit > 256 || grace.is_zero() {
			return Err(StoreError::InvalidInput("blob reclamation bounds are invalid"));
		}

		self.pool().get().await?.query_one("SELECT decodex.prune_history_snapshots()", &[]).await?;

		let page = blob_store.old_inventory(grace, usize::from(limit), after)?;
		let mut removed = 0_u16;

		for candidate in page.entries {
			let mut session = self.lock_blob_session(&[candidate.hash], &[candidate.hash]).await?;
			let transaction = session.client.transaction().await?;
			let referenced: bool = transaction
				.query_one(
					"SELECT EXISTS (SELECT 1 FROM decodex.history_items WHERE blob_hash=$1) \
					 OR EXISTS (SELECT 1 FROM decodex.history_item_versions WHERE blob_hash=$1) \
					 OR EXISTS (SELECT 1 FROM decodex.artifact_revisions WHERE blob_hash=$1) \
					 OR EXISTS (SELECT 1 FROM decodex.context_packs WHERE blob_hash=$1)",
					&[&candidate.hash.to_hex()],
				)
				.await?
				.get(0);
			let eligible_for_unlink = !referenced;

			if !referenced {
				let deletion = transaction
					.execute(
						"DELETE FROM decodex.blob_objects WHERE blob_hash=$1",
						&[&candidate.hash.to_hex()],
					)
					.await;

				match deletion {
					Ok(_) => {},
					Err(error)
						if error.code().is_some_and(|code| {
							code == &SqlState::FOREIGN_KEY_VIOLATION
								|| code == &SqlState::RESTRICT_VIOLATION
						}) =>
					{
						transaction.rollback().await?;

						continue;
					},
					Err(error) => return Err(error.into()),
				}
			}

			transaction.commit().await?;

			if eligible_for_unlink && blob_store.remove_orphan_if_old(candidate.hash, grace)? {
				removed += 1;
			}
		}

		Ok(BlobReclaimPage { removed, next_cursor: page.next_cursor })
	}

	/// Read one strictly bounded keyset page and verify every referenced blob before return.
	pub async fn conversation_history(
		&self,
		blob_store: &BlobStore,
		conversation_id: &ConversationId,
		after: Option<&HistoryCursor>,
		page_size: u16,
	) -> Result<HistoryPage, StoreError> {
		if page_size == 0 || page_size > MAX_PAGE_SIZE {
			return Err(StoreError::InvalidInput("history page size must be within 1..=100"));
		}

		let mut client = self.pool().get().await?;
		let transaction = client.transaction().await?;

		transaction
			.query_one(
				"SELECT pg_catalog.pg_advisory_xact_lock($1)",
				&[&HIERARCHY_COORDINATION_LOCK],
			)
			.await?;
		transaction
			.query_one("SELECT pg_catalog.pg_advisory_xact_lock($1)", &[&CURSOR_COORDINATION_LOCK])
			.await?;

		if transaction
			.query_opt(
				"SELECT 1 FROM decodex.conversations \
				 WHERE conversation_id = $1::text::uuid FOR UPDATE",
				&[&conversation_id.as_str()],
			)
			.await?
			.is_none()
		{
			return Err(StoreError::InvalidInput("Conversation does not exist"));
		}

		let current_high_water: i64 = transaction
			.query_one(
				"SELECT COALESCE(max(history_position), 0) FROM decodex.history_items \
				 WHERE conversation_id = $1::text::uuid",
				&[&conversation_id.as_str()],
			)
			.await?
			.get(0);
		let current_snapshot_version: i64 = transaction
			.query_one(
				"SELECT COALESCE(max(version_sequence), 0) FROM decodex.history_item_versions \
				 WHERE conversation_id = $1::text::uuid",
				&[&conversation_id.as_str()],
			)
			.await?
			.get(0);
		let (snapshot_high_water, snapshot_version, last_position): (i64, i64, i64) =
			if let Some(cursor) = after {
				let row = transaction
					.query_opt(
						"SELECT snapshot_high_water, snapshot_version_sequence, last_position, page_size \
					 FROM decodex.history_cursors \
					 WHERE cursor_id=$1::text::uuid AND conversation_id=$2::text::uuid \
					 AND expires_at > clock_timestamp()",
						&[&cursor.token, &conversation_id.as_str()],
					)
					.await?
					.ok_or(StoreError::InvalidInput(
						"history cursor was not issued or has expired for this Conversation",
					))?;

				if row.get::<_, i32>(3) != i32::from(page_size) {
					return Err(StoreError::InvalidInput(
						"history page size must match the issued cursor",
					));
				}

				(row.get(0), row.get(1), row.get(2))
			} else {
				(current_high_water, current_snapshot_version, 0)
			};
		let fetch_limit = i64::from(page_size) + 1;
		let rows = transaction
			.query(
				"SELECT hi.history_position, hi.history_item_id::text, t.turn_id::text, \
				 t.runtime_session_id::text, t.role::text, t.possible_side_effects::text, \
				 hi.kind::text, hi.status::text, hi.inline_text, hi.blob_hash, bo.byte_length, \
				 hi.media_type, hi.metadata, hi.artifact_id::text, hi.artifact_revision, hi.revision \
				 FROM (SELECT DISTINCT ON (history_position) * \
				       FROM decodex.history_item_versions \
				       WHERE conversation_id=$1::text::uuid AND version_sequence <= $4 \
				       ORDER BY history_position, version_sequence DESC) AS hi \
				 JOIN decodex.turns AS t ON t.turn_id = hi.turn_id \
				 LEFT JOIN decodex.blob_objects AS bo ON bo.blob_hash = hi.blob_hash \
				 WHERE hi.history_position > $2 AND hi.history_position <= $3 \
				 ORDER BY hi.history_position LIMIT $5",
				&[
					&conversation_id.as_str(),
					&last_position,
					&snapshot_high_water,
					&snapshot_version,
					&fetch_limit,
				],
			)
			.await?;
		let has_more = rows.len() > usize::from(page_size);
		let mut entries = Vec::with_capacity(rows.len().min(usize::from(page_size)));

		for row in rows.into_iter().take(usize::from(page_size)) {
			entries.push(history_entry_from_row(row)?);
		}

		let next_cursor = if has_more {
			Some(
				issue_history_cursor(&transaction, conversation_id, after, i32::from(page_size))
					.await?,
			)
		} else {
			None
		};

		transaction.commit().await?;

		for entry in &entries {
			self.verify_history_entry(blob_store, entry).await?;
		}

		Ok(HistoryPage { entries, next_cursor })
	}

	/// Persist an immutable compiled Context Pack and its exact provenance revisions.
	pub async fn persist_context_pack(
		&self,
		blob_store: &BlobStore,
		command: &CommandIdentity,
		request: &PersistContextPack,
		pack: &ContextPack,
	) -> Result<ContextPackRecord, StoreError> {
		validate_context_pack(request, pack)?;

		let blob = if pack.bytes().len() > MAX_INLINE_HISTORY_BYTES {
			Some((
				pack.digest(),
				i64::try_from(pack.bytes().len()).map_err(|_| {
					StoreError::InvalidInput("Context Pack byte length is out of range")
				})?,
			))
		} else {
			None
		};
		let inline_bytes = blob.is_none().then(|| pack.bytes().to_vec());
		let mut client = self.pool().get().await?;
		let reservation = match reserve_conversation_command(
			&mut client,
			command,
			"persist_context_pack",
			("conversation", pack.conversation_id().as_str(), &request.context_pack_id),
			Some(request.pack_revision),
			blob,
		)
		.await?
		{
			accounts::CommandClaim::Completed(_) => {
				return self.required_context_pack(blob_store, &request.context_pack_id).await;
			},
			accounts::CommandClaim::Owned(reservation) => reservation,
		};

		drop(client);

		let capacity_hashes = blob.map(|(hash, _)| vec![hash]).unwrap_or_default();
		let referenced_hashes = context_pack_referenced_hashes(pack, blob);
		let mut publication = if !referenced_hashes.is_empty() {
			let publication = self.lock_blob_session(&referenced_hashes, &capacity_hashes).await?;

			if let Some((hash, _)) = blob {
				publish_verified_blob(blob_store, hash, pack.bytes())?;
			}

			publication
		} else {
			self.dedicated_session().await?
		};
		let transaction = publication.client.transaction().await?;

		if let Some((hash, byte_length)) = blob {
			insert_verified_blob(&transaction, hash, byte_length).await?;
		}

		insert_context_pack_sources(&transaction, request, pack).await?;

		transaction
			.execute(
				"INSERT INTO decodex.context_packs \
				 (context_pack_id, conversation_id, pack_revision, compiled_digest, manifest_digest, inline_bytes, \
				  blob_hash, byte_length, max_bytes, recent_item_limit, possible_side_effects, \
				  truncated, omitted_source_count, source_count) \
				 VALUES ($1::text::uuid, $2::text::uuid, $3, $4, $5, $6, $7, $8, $9, $10, \
				 $11::text::decodex.side_effect_state, $12, $13, $14)",
				&[
					&request.context_pack_id,
					&pack.conversation_id().as_str(),
					&request.pack_revision,
					&pack.digest().to_hex(),
					&pack.manifest_digest().to_hex(),
					&inline_bytes,
					&blob.map(|(hash, _)| hash.to_hex()),
					&i64::try_from(pack.bytes().len()).unwrap_or(i64::MAX),
					&i32::try_from(pack.policy().max_bytes()).unwrap_or(i32::MAX),
					&i32::try_from(pack.policy().recent_item_limit()).unwrap_or(i32::MAX),
					&side_effect_sql(pack.possible_side_effects()),
					&pack.truncated(),
					&i32::try_from(pack.omitted_source_count()).unwrap_or(i32::MAX),
					&i32::try_from(pack.source_manifest().len()).unwrap_or(i32::MAX),
				],
			)
			.await?;

		let payload = serde_json::json!({
			"context_pack_id": request.context_pack_id,
			"conversation_id": pack.conversation_id().as_str(),
			"pack_revision": request.pack_revision,
			"compiled_digest": pack.digest().to_hex(),
			"dispatch_enabled": false,
		});

		accounts::append_activity_and_outbox(
			&transaction,
			"context_pack",
			&request.context_pack_id,
			request.pack_revision,
			"context_pack_compiled",
			&command.key,
			&payload,
		)
		.await?;
		accounts::finish_command(
			&transaction,
			&reservation,
			&serde_json::json!({
				"kind": "context_pack",
				"context_pack_id": request.context_pack_id,
			}),
		)
		.await?;

		transaction.commit().await?;

		self.required_context_pack(blob_store, &request.context_pack_id).await
	}

	/// Persist an inert transition proposal whose dispatch flag is schema-forced false.
	pub async fn propose_transition(
		&self,
		command: &CommandIdentity,
		proposal: &ProposeTransition,
	) -> Result<(), StoreError> {
		if !is_canonical_uuid(&proposal.transition_id)
			|| !is_canonical_uuid(&proposal.context_pack_id)
			|| proposal.reason.is_empty()
			|| proposal.reason.len() > 512
		{
			return Err(StoreError::InvalidInput("transition proposal is malformed"));
		}

		crate::ensure_credential_negative_text(&proposal.reason)?;

		let mut client = self.pool().get().await?;
		let reservation = match reserve_conversation_command(
			&mut client,
			command,
			"propose_transition",
			("conversation", proposal.conversation_id.as_str(), &proposal.transition_id),
			None,
			None,
		)
		.await?
		{
			accounts::CommandClaim::Completed(_) => return Ok(()),
			accounts::CommandClaim::Owned(reservation) => reservation,
		};
		let transaction = client.transaction().await?;

		transaction
			.execute(
				"INSERT INTO decodex.transition_proposals \
				 (transition_id, conversation_id, from_runtime_session_id, context_pack_id, kind, reason) \
				 VALUES ($1::text::uuid, $2::text::uuid, $3::text::uuid, $4::text::uuid, \
				 $5::text::decodex.transition_kind, $6)",
				&[
					&proposal.transition_id,
					&proposal.conversation_id.as_str(),
					&proposal.from_runtime_session_id.as_str(),
					&proposal.context_pack_id,
					&transition_sql(proposal.kind),
					&proposal.reason,
				],
			)
			.await?;

		accounts::append_activity_and_outbox(
			&transaction,
			"transition_proposal",
			&proposal.transition_id,
			1,
			"transition_proposed",
			&command.key,
			&serde_json::json!({
				"conversation_id": proposal.conversation_id.as_str(),
				"kind": transition_sql(proposal.kind),
				"dispatch_enabled": false,
			}),
		)
		.await?;
		accounts::finish_command(
			&transaction,
			&reservation,
			&serde_json::json!({
				"kind": "transition_proposal",
				"transition_id": proposal.transition_id,
				"dispatch_enabled": false,
			}),
		)
		.await?;

		transaction.commit().await?;

		Ok(())
	}

	async fn verify_history_entry(
		&self,
		blob_store: &BlobStore,
		entry: &HistoryEntry,
	) -> Result<(), StoreError> {
		verify_entry_blob(blob_store, entry)?;

		if let Some((artifact_id, revision)) = &entry.artifact {
			let revision = i64::try_from(*revision).map_err(|_| {
				StoreError::Incompatible("history Artifact revision is invalid".into())
			})?;
			let row = self
				.pool()
				.get()
				.await?
				.query_opt(
					"SELECT ar.blob_hash, bo.byte_length FROM decodex.artifact_revisions ar \
					 JOIN decodex.blob_objects bo ON bo.blob_hash=ar.blob_hash \
					 WHERE ar.artifact_id=$1::text::uuid AND ar.revision=$2",
					&[&artifact_id.as_str(), &revision],
				)
				.await?
				.ok_or_else(|| {
					StoreError::Incompatible("history Artifact revision metadata is absent".into())
				})?;
			let hash = BlobHash::parse(row.get(0))?;
			let bytes = blob_store.read(hash)?;

			if i64::try_from(bytes.len()).ok() != Some(row.get(1)) {
				return Err(StoreError::Incompatible(
					"history Artifact blob length differs from metadata".into(),
				));
			}
		}

		Ok(())
	}

	/// Inspect Context Pack metadata only after re-verifying its inline or blob bytes.
	pub async fn context_pack(
		&self,
		blob_store: &BlobStore,
		context_pack_id: &str,
	) -> Result<ContextPackRecord, StoreError> {
		if !is_canonical_uuid(context_pack_id) {
			return Err(StoreError::InvalidInput("Context Pack identity is malformed"));
		}

		self.required_context_pack(blob_store, context_pack_id).await
	}

	async fn verify_context_pack_artifacts(
		&self,
		blob_store: &BlobStore,
		sources: &[decodex_core::ContextSourceManifest],
	) -> Result<(), StoreError> {
		for source in sources {
			let Some((artifact_id, revision)) = source.artifact_reference() else {
				continue;
			};
			let revision = i64::try_from(revision).map_err(|_| {
				StoreError::Incompatible("Context Pack Artifact revision is invalid".into())
			})?;
			let row = self
				.pool()
				.get()
				.await?
				.query_opt(
					"SELECT ar.blob_hash, bo.byte_length FROM decodex.artifact_revisions ar \
					 JOIN decodex.blob_objects bo ON bo.blob_hash=ar.blob_hash \
					 WHERE ar.artifact_id=$1::text::uuid AND ar.revision=$2",
					&[&artifact_id.as_str(), &revision],
				)
				.await?
				.ok_or_else(|| {
					StoreError::Incompatible(
						"Context Pack Artifact revision metadata is absent".into(),
					)
				})?;
			let hash = BlobHash::parse(row.get(0))?;
			let bytes = blob_store.read(hash)?;

			if hash != source.content_digest()
				|| u64::try_from(bytes.len()).ok() != Some(source.original_byte_length())
				|| i64::try_from(bytes.len()).ok() != Some(row.get(1))
			{
				return Err(StoreError::Incompatible(
					"Context Pack Artifact bytes differ from provenance".into(),
				));
			}
		}

		Ok(())
	}

	pub(crate) async fn required_context_pack(
		&self,
		blob_store: &BlobStore,
		context_pack_id: &str,
	) -> Result<ContextPackRecord, StoreError> {
		let row = self
			.pool()
			.get()
			.await?
			.query_opt(
				"SELECT context_pack_id::text, conversation_id::text, pack_revision, compiled_digest, \
				 manifest_digest, byte_length, max_bytes, recent_item_limit, possible_side_effects::text, \
				 truncated, omitted_source_count, inline_bytes, blob_hash, source_count \
			 FROM decodex.context_packs \
			 WHERE context_pack_id = $1::text::uuid",
				&[&context_pack_id],
			)
			.await?
			.ok_or_else(|| {
				StoreError::Incompatible("Context Pack command receipt lost entity".into())
			})?;
		let digest = BlobHash::parse(row.get::<_, &str>(3))?;
		let stored_manifest_digest = BlobHash::parse(row.get::<_, &str>(4))?;
		let byte_length = u64::try_from(row.get::<_, i64>(5)).map_err(|_| {
			StoreError::Incompatible("stored Context Pack length is invalid".into())
		})?;
		let inline: Option<Vec<u8>> = row.get(11);
		let blob: Option<String> = row.get(12);
		let bytes = match (inline, blob) {
			(Some(inline), None) => inline,
			(None, Some(blob)) => blob_store.read(BlobHash::parse(&blob)?)?,
			_ => {
				return Err(StoreError::Incompatible(
					"stored Context Pack payload is invalid".into(),
				));
			},
		};

		if BlobHash::digest(&bytes) != digest
			|| u64::try_from(bytes.len()).ok() != Some(byte_length)
		{
			return Err(StoreError::Incompatible(
				"stored Context Pack bytes failed verification".into(),
			));
		}

		let conversation_id = ConversationId::new(row.get::<_, String>(1)).map_err(|_| {
			StoreError::Incompatible("stored conversation identity is invalid".into())
		})?;
		let source_rows = self
			.pool()
			.get()
			.await?
			.query(
				"SELECT kind::text, source_id, source_revision, content_digest, original_byte_length, \
			 included_byte_length, included_digest, disposition::text, artifact_id::text, artifact_revision \
			 FROM decodex.context_pack_sources WHERE context_pack_id = $1::text::uuid ORDER BY position",
				&[&context_pack_id],
			)
			.await?;
		let sources =
			source_rows.into_iter().map(context_source_from_row).collect::<Result<Vec<_>, _>>()?;

		self.verify_context_pack_artifacts(blob_store, &sources).await?;

		if i32::try_from(sources.len()).ok() != Some(row.get(13)) {
			return Err(StoreError::Incompatible(
				"stored Context Pack source count is inconsistent".into(),
			));
		}

		let policy = ContextPackPolicy::new(
			usize::try_from(row.get::<_, i32>(6))
				.map_err(|_| StoreError::Incompatible("stored policy is invalid".into()))?,
			usize::try_from(row.get::<_, i32>(7))
				.map_err(|_| StoreError::Incompatible("stored policy is invalid".into()))?,
		)
		.map_err(|_| StoreError::Incompatible("stored policy is invalid".into()))?;
		let pack = ContextPack::from_persisted(
			conversation_id.clone(),
			side_effect_from_sql(row.get::<_, &str>(8))?,
			policy,
			sources,
			bytes,
			digest,
		)
		.map_err(|_| {
			StoreError::Incompatible(
				"stored Context Pack record failed canonical verification".into(),
			)
		})?;

		if pack.manifest_digest() != stored_manifest_digest
			|| pack.truncated() != row.get::<_, bool>(9)
			|| pack.omitted_source_count()
				!= usize::try_from(row.get::<_, i32>(10)).unwrap_or(usize::MAX)
		{
			return Err(StoreError::Incompatible(
				"stored Context Pack metadata is inconsistent".into(),
			));
		}

		Ok(ContextPackRecord {
			context_pack_id: row.get(0),
			conversation_id,
			pack_revision: row.get(2),
			compiled_digest: digest,
			byte_length,
			truncated: row.get(9),
			omitted_source_count: usize::try_from(row.get::<_, i32>(10))
				.map_err(|_| StoreError::Incompatible("stored source count is invalid".into()))?,
			pack,
		})
	}
}

pub(crate) fn context_pack_referenced_hashes(
	pack: &ContextPack,
	blob: Option<(BlobHash, i64)>,
) -> Vec<BlobHash> {
	let mut hashes = pack
		.source_manifest()
		.iter()
		.filter(|source| source.artifact_reference().is_some())
		.map(decodex_core::ContextSourceManifest::content_digest)
		.collect::<Vec<_>>();

	if let Some((hash, _)) = blob {
		hashes.push(hash);
	}

	hashes
}

fn validate_artifact(create: &CreateArtifact) -> Result<(), StoreError> {
	if create.bytes.is_empty()
		|| create.bytes.len() > MAX_BLOB_BYTES
		|| !decodex_core::is_canonical_media_type(&create.media_type)
		|| create.display_name.as_ref().is_some_and(|name| name.is_empty() || name.len() > 256)
	{
		return Err(StoreError::InvalidInput("Artifact violates a field or payload bound"));
	}

	crate::ensure_credential_negative_text(&create.media_type)?;

	if let Some(name) = &create.display_name {
		crate::ensure_credential_negative_text(name)?;
	}

	Ok(())
}

fn validate_history_item(mutation: &RecordHistoryItem) -> Result<(), StoreError> {
	if mutation.turn_sequence <= 0
		|| !(0..=1_000_000).contains(&mutation.ordinal)
		|| mutation.text.len() > MAX_BLOB_BYTES
		|| mutation.expected_revision.is_some_and(|revision| revision < 1)
		|| (mutation.kind == HistoryItemKind::Artifact) != mutation.artifact.is_some()
	{
		return Err(StoreError::InvalidInput("history item violates a field or payload bound"));
	}

	crate::ensure_credential_negative_text(&mutation.text)?;
	crate::ensure_credential_negative_text(mutation.media_type.as_str())?;

	Ok(())
}

fn history_metadata_json(metadata: &HistoryMetadata) -> Result<Value, StoreError> {
	serde_json::to_value(metadata)
		.map_err(|_| StoreError::Incompatible("history metadata could not be encoded".into()))
}

fn history_metadata_from_json(metadata: Value) -> Result<HistoryMetadata, StoreError> {
	serde_json::from_value(metadata)
		.map_err(|_| StoreError::Incompatible("history metadata is invalid".into()))
}

fn prepare_payload(text: &str) -> Result<Option<(BlobHash, i64)>, StoreError> {
	if text.len() <= MAX_INLINE_HISTORY_BYTES {
		return Ok(None);
	}

	Ok(Some((
		BlobHash::digest(text.as_bytes()),
		i64::try_from(text.len())
			.map_err(|_| StoreError::InvalidInput("blob length is out of range"))?,
	)))
}

fn history_entry_from_row(row: Row) -> Result<HistoryEntry, StoreError> {
	let history_item_id: String = row.get(1);
	let blob_hash =
		row.get::<_, Option<String>>(9).map(|value| BlobHash::parse(&value)).transpose()?;
	let blob_byte_length = row
		.get::<_, Option<i64>>(10)
		.map(|value| {
			u64::try_from(value)
				.map_err(|_| StoreError::Incompatible("blob length is invalid".into()))
		})
		.transpose()?;
	let artifact = match (row.get::<_, Option<String>>(13), row.get::<_, Option<i64>>(14)) {
		(Some(id), Some(revision)) => Some((
			ArtifactId::new(id).map_err(|_| {
				StoreError::Incompatible("history Artifact identity is invalid".into())
			})?,
			u64::try_from(revision).map_err(|_| {
				StoreError::Incompatible("history Artifact revision is invalid".into())
			})?,
		)),
		(None, None) => None,
		_ => {
			return Err(StoreError::Incompatible(
				"history Artifact reference is incomplete".into(),
			));
		},
	};
	let kind = item_kind_from_sql(row.get::<_, &str>(6))?;
	let media_type = HistoryMediaType::new(row.get::<_, String>(11))
		.map_err(|_| StoreError::Incompatible("history media type is invalid".into()))?;
	let metadata = history_metadata_from_json(row.get(12))?;

	if (kind == HistoryItemKind::Artifact) != artifact.is_some() {
		return Err(StoreError::Incompatible(
			"history Artifact kind/reference is inconsistent".into(),
		));
	}

	Ok(HistoryEntry {
		history_item_id,
		turn_id: row.get(2),
		runtime_session_id: row.get(3),
		turn_role: turn_role_from_sql(row.get::<_, &str>(4))?,
		possible_side_effects: side_effect_from_sql(row.get::<_, &str>(5))?,
		kind,
		status: item_status_from_sql(row.get::<_, &str>(7))?,
		inline_text: row.get(8),
		blob_hash,
		blob_byte_length,
		media_type,
		metadata,
		artifact,
		revision: row.get(15),
	})
}

fn verify_entry_blob(blob_store: &BlobStore, entry: &HistoryEntry) -> Result<(), StoreError> {
	match (entry.blob_hash, entry.blob_byte_length) {
		(Some(hash), Some(expected)) => {
			let bytes = blob_store.read(hash)?;

			if u64::try_from(bytes.len()).ok() != Some(expected) {
				return Err(StoreError::Incompatible(
					"verified blob length differs from metadata".into(),
				));
			}

			Ok(())
		},
		(None, None) if entry.inline_text.is_some() => Ok(()),
		_ => Err(StoreError::Incompatible("history payload metadata is incomplete".into())),
	}
}

pub(crate) fn validate_context_pack(
	request: &PersistContextPack,
	pack: &ContextPack,
) -> Result<(), StoreError> {
	if !is_canonical_uuid(&request.context_pack_id)
		|| request.pack_revision < 1
		|| pack.verify().is_err()
	{
		return Err(StoreError::InvalidInput("Context Pack violates its immutable policy"));
	}

	for source in pack.source_manifest() {
		crate::ensure_credential_negative_text(source.source_id())?;
	}

	Ok(())
}

fn context_source_from_row(row: Row) -> Result<decodex_core::ContextSourceManifest, StoreError> {
	let artifact = match (row.get::<_, Option<String>>(8), row.get::<_, Option<i64>>(9)) {
		(Some(id), Some(revision)) => Some((
			ArtifactId::new(id).map_err(|_| {
				StoreError::Incompatible("stored Artifact identity is invalid".into())
			})?,
			u64::try_from(revision).map_err(|_| {
				StoreError::Incompatible("stored Artifact revision is invalid".into())
			})?,
		)),
		(None, None) => None,
		_ => return Err(StoreError::Incompatible("stored Artifact source is incomplete".into())),
	};

	decodex_core::ContextSourceManifest::from_persisted(
		context_source_from_sql(row.get(0))?,
		row.get::<_, String>(1),
		u64::try_from(row.get::<_, i64>(2))
			.map_err(|_| StoreError::Incompatible("stored source revision is invalid".into()))?,
		BlobHash::parse(row.get(3))?,
		u64::try_from(row.get::<_, i64>(4))
			.map_err(|_| StoreError::Incompatible("stored source length is invalid".into()))?,
		u64::try_from(row.get::<_, i64>(5))
			.map_err(|_| StoreError::Incompatible("stored included length is invalid".into()))?,
		BlobHash::parse(row.get(6))?,
		context_disposition_from_sql(row.get(7))?,
		artifact,
	)
	.map_err(|_| StoreError::Incompatible("stored source manifest is invalid".into()))
}

const fn artifact_status_sql(value: ArtifactStatus) -> &'static str {
	match value {
		ArtifactStatus::Active => "active",
		ArtifactStatus::Expired => "expired",
		ArtifactStatus::Deleted => "deleted",
	}
}

fn artifact_status_from_sql(value: &str) -> Result<ArtifactStatus, StoreError> {
	match value {
		"active" => Ok(ArtifactStatus::Active),
		"expired" => Ok(ArtifactStatus::Expired),
		"deleted" => Ok(ArtifactStatus::Deleted),
		_ => Err(StoreError::Incompatible("unknown Artifact status".into())),
	}
}

const fn turn_role_sql(value: TurnRole) -> &'static str {
	match value {
		TurnRole::User => "user",
		TurnRole::Assistant => "assistant",
		TurnRole::System => "system",
		TurnRole::Tool => "tool",
	}
}

fn turn_role_from_sql(value: &str) -> Result<TurnRole, StoreError> {
	match value {
		"user" => Ok(TurnRole::User),
		"assistant" => Ok(TurnRole::Assistant),
		"system" => Ok(TurnRole::System),
		"tool" => Ok(TurnRole::Tool),
		_ => Err(StoreError::Incompatible("unknown turn role".into())),
	}
}

pub(crate) const fn side_effect_sql(value: PossibleSideEffects) -> &'static str {
	match value {
		PossibleSideEffects::None => "none",
		PossibleSideEffects::Possible => "possible",
		PossibleSideEffects::Unknown => "unknown",
	}
}

fn side_effect_from_sql(value: &str) -> Result<PossibleSideEffects, StoreError> {
	match value {
		"none" => Ok(PossibleSideEffects::None),
		"possible" => Ok(PossibleSideEffects::Possible),
		"unknown" => Ok(PossibleSideEffects::Unknown),
		_ => Err(StoreError::Incompatible("unknown side-effect state".into())),
	}
}

const fn item_kind_sql(value: HistoryItemKind) -> &'static str {
	match value {
		HistoryItemKind::Message => "message",
		HistoryItemKind::Reasoning => "reasoning",
		HistoryItemKind::ToolCall => "tool_call",
		HistoryItemKind::ToolResult => "tool_result",
		HistoryItemKind::Artifact => "artifact",
		HistoryItemKind::Status => "status",
	}
}

fn item_kind_from_sql(value: &str) -> Result<HistoryItemKind, StoreError> {
	match value {
		"message" => Ok(HistoryItemKind::Message),
		"reasoning" => Ok(HistoryItemKind::Reasoning),
		"tool_call" => Ok(HistoryItemKind::ToolCall),
		"tool_result" => Ok(HistoryItemKind::ToolResult),
		"artifact" => Ok(HistoryItemKind::Artifact),
		"status" => Ok(HistoryItemKind::Status),
		_ => Err(StoreError::Incompatible("unknown history-item kind".into())),
	}
}

const fn item_status_sql(value: ItemStatus) -> &'static str {
	match value {
		ItemStatus::Streaming => "streaming",
		ItemStatus::Completed => "completed",
		ItemStatus::Failed => "failed",
	}
}

fn item_status_from_sql(value: &str) -> Result<ItemStatus, StoreError> {
	match value {
		"streaming" => Ok(ItemStatus::Streaming),
		"completed" => Ok(ItemStatus::Completed),
		"failed" => Ok(ItemStatus::Failed),
		_ => Err(StoreError::Incompatible("unknown history-item status".into())),
	}
}

pub(crate) const fn context_source_sql(value: ContextSourceKind) -> &'static str {
	match value {
		ContextSourceKind::PinnedRevision => "pinned_revision",
		ContextSourceKind::RepositoryInstructions => "repository_instructions",
		ContextSourceKind::OpenWiki => "openwiki",
		ContextSourceKind::Decision => "decision",
		ContextSourceKind::Fact => "fact",
		ContextSourceKind::Artifact => "artifact",
		ContextSourceKind::RecentRaw => "recent_raw",
	}
}

fn context_source_from_sql(value: &str) -> Result<ContextSourceKind, StoreError> {
	match value {
		"pinned_revision" => Ok(ContextSourceKind::PinnedRevision),
		"repository_instructions" => Ok(ContextSourceKind::RepositoryInstructions),
		"openwiki" => Ok(ContextSourceKind::OpenWiki),
		"decision" => Ok(ContextSourceKind::Decision),
		"fact" => Ok(ContextSourceKind::Fact),
		"artifact" => Ok(ContextSourceKind::Artifact),
		"recent_raw" => Ok(ContextSourceKind::RecentRaw),
		_ => Err(StoreError::Incompatible("unknown Context Pack source kind".into())),
	}
}

pub(crate) const fn context_disposition_sql(value: ContextSourceDisposition) -> &'static str {
	match value {
		ContextSourceDisposition::Complete => "complete",
		ContextSourceDisposition::Truncated => "truncated",
		ContextSourceDisposition::Omitted => "omitted",
	}
}

fn context_disposition_from_sql(value: &str) -> Result<ContextSourceDisposition, StoreError> {
	match value {
		"complete" => Ok(ContextSourceDisposition::Complete),
		"truncated" => Ok(ContextSourceDisposition::Truncated),
		"omitted" => Ok(ContextSourceDisposition::Omitted),
		_ => Err(StoreError::Incompatible("unknown Context Pack source disposition".into())),
	}
}

const fn transition_sql(value: ProposedTransitionKind) -> &'static str {
	match value {
		ProposedTransitionKind::Rollover => "rollover",
		ProposedTransitionKind::Fallback => "fallback",
	}
}

fn is_canonical_uuid(value: &str) -> bool {
	value.len() == 36
		&& value.bytes().enumerate().all(|(index, byte)| match index {
			8 | 13 | 18 | 23 => byte == b'-',
			_ => byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'),
		})
}

pub(crate) fn publish_verified_blob(
	blob_store: &BlobStore,
	hash: BlobHash,
	bytes: &[u8],
) -> Result<(), StoreError> {
	let published = blob_store.put(bytes)?;

	if published != hash || blob_store.read(hash)? != bytes {
		return Err(StoreError::InvalidInput("blob publication verification failed"));
	}

	Ok(())
}

fn response_revision(response: &Value, kind: &str) -> Result<i64, StoreError> {
	if response.get("kind").and_then(Value::as_str) != Some(kind) {
		return Err(StoreError::Incompatible("stored command response kind is invalid".into()));
	}

	response.get("revision").and_then(Value::as_i64).filter(|revision| *revision > 0).ok_or_else(
		|| StoreError::Incompatible("stored command response revision is invalid".into()),
	)
}

fn history_entry_response(
	mutation: &RecordHistoryItem,
	blob: Option<(BlobHash, i64)>,
	revision: i64,
) -> Result<Value, StoreError> {
	let blob_byte_length = blob
		.map(|(_, length)| u64::try_from(length))
		.transpose()
		.map_err(|_| StoreError::Incompatible("history response length is invalid".into()))?;
	let artifact = mutation.artifact.as_ref().map(|(id, artifact_revision)| {
		serde_json::json!({
			"artifact_id": id.as_str(),
			"revision": artifact_revision,
		})
	});

	Ok(serde_json::json!({
		"kind": "history_item",
		"history_item_id": mutation.history_item_id.as_str(),
		"turn_id": mutation.turn_id.as_str(),
		"runtime_session_id": mutation.runtime_session_id.as_str(),
		"turn_role": turn_role_sql(mutation.turn_role),
		"possible_side_effects": side_effect_sql(mutation.possible_side_effects),
		"item_kind": item_kind_sql(mutation.kind),
		"status": item_status_sql(mutation.status),
		"inline_text": blob.is_none().then_some(mutation.text.as_str()),
		"blob_hash": blob.map(|(hash, _)| hash.to_hex()),
		"blob_byte_length": blob_byte_length,
		"media_type": mutation.media_type.as_str(),
		"metadata": mutation.metadata,
		"artifact": artifact,
		"revision": revision,
	}))
}

fn history_entry_from_response(response: &Value) -> Result<HistoryEntry, StoreError> {
	if response.get("kind").and_then(Value::as_str) != Some("history_item") {
		return Err(StoreError::Incompatible("stored history response kind is invalid".into()));
	}

	let required = |field: &'static str| {
		response
			.get(field)
			.and_then(Value::as_str)
			.ok_or_else(|| StoreError::Incompatible("stored history response is incomplete".into()))
	};
	let inline_text = match response.get("inline_text") {
		Some(Value::String(text)) => Some(text.clone()),
		Some(Value::Null) => None,
		_ => return Err(StoreError::Incompatible("stored history payload is invalid".into())),
	};
	let blob_hash = match response.get("blob_hash") {
		Some(Value::String(hash)) => Some(BlobHash::parse(hash)?),
		Some(Value::Null) => None,
		_ => return Err(StoreError::Incompatible("stored history payload is invalid".into())),
	};
	let blob_byte_length = match response.get("blob_byte_length") {
		Some(Value::Number(length)) => length.as_u64(),
		Some(Value::Null) => None,
		_ => return Err(StoreError::Incompatible("stored history payload is invalid".into())),
	};
	let artifact = match response.get("artifact") {
		Some(Value::Object(reference)) => {
			let id = reference.get("artifact_id").and_then(Value::as_str).ok_or_else(|| {
				StoreError::Incompatible("stored history Artifact response is invalid".into())
			})?;
			let revision = reference
				.get("revision")
				.and_then(Value::as_u64)
				.filter(|v| *v > 0)
				.ok_or_else(|| {
					StoreError::Incompatible("stored history Artifact response is invalid".into())
				})?;

			Some((
				ArtifactId::new(id.to_owned()).map_err(|_| {
					StoreError::Incompatible("stored history Artifact response is invalid".into())
				})?,
				revision,
			))
		},
		Some(Value::Null) => None,
		_ => {
			return Err(StoreError::Incompatible(
				"stored history Artifact response is invalid".into(),
			));
		},
	};
	let metadata = response.get("metadata").cloned().ok_or_else(|| {
		StoreError::Incompatible("stored history metadata response is absent".into())
	})?;
	let media_type = HistoryMediaType::new(required("media_type")?.to_owned())
		.map_err(|_| StoreError::Incompatible("stored history media type is invalid".into()))?;
	let metadata = history_metadata_from_json(metadata)?;
	let kind = item_kind_from_sql(required("item_kind")?)?;

	if (inline_text.is_some() == blob_hash.is_some())
		|| blob_hash.is_some() != blob_byte_length.is_some()
		|| (kind == HistoryItemKind::Artifact) != artifact.is_some()
	{
		return Err(StoreError::Incompatible("stored history response is inconsistent".into()));
	}

	Ok(HistoryEntry {
		history_item_id: required("history_item_id")?.to_owned(),
		turn_id: required("turn_id")?.to_owned(),
		runtime_session_id: required("runtime_session_id")?.to_owned(),
		turn_role: turn_role_from_sql(required("turn_role")?)?,
		possible_side_effects: side_effect_from_sql(required("possible_side_effects")?)?,
		kind,
		status: item_status_from_sql(required("status")?)?,
		inline_text,
		blob_hash,
		blob_byte_length,
		media_type,
		metadata,
		artifact,
		revision: response_revision(response, "history_item")?,
	})
}

fn conversation_from_response(response: &Value) -> Result<StoredConversation, StoreError> {
	if response.get("kind").and_then(Value::as_str) != Some("conversation") {
		return Err(StoreError::Incompatible(
			"stored Conversation response kind is invalid".into(),
		));
	}

	let conversation_id =
		response.get("conversation_id").and_then(Value::as_str).ok_or_else(|| {
			StoreError::Incompatible("stored Conversation response is incomplete".into())
		})?;
	let title = response.get("title").and_then(Value::as_str).ok_or_else(|| {
		StoreError::Incompatible("stored Conversation response is incomplete".into())
	})?;

	Ok(StoredConversation {
		conversation_id: ConversationId::new(conversation_id.to_owned()).map_err(|_| {
			StoreError::Incompatible("stored Conversation response identity is invalid".into())
		})?,
		title: title.to_owned(),
		revision: response_revision(response, "conversation")?,
	})
}

#[cfg(debug_assertions)]
async fn post_commit_test_barrier(operation: &str) -> Result<(), StoreError> {
	let Ok(root) = env::var("DECODEX_TEST_POST_COMMIT_SYNC") else {
		return Ok(());
	};
	let root = PathBuf::from(root);
	let committed = root.join(format!("{operation}.committed"));
	let release = root.join(format!("{operation}.continue"));

	fs::write(&committed, b"committed")
		.map_err(|_| StoreError::Incompatible("test post-commit barrier failed".into()))?;

	for _ in 0..3_000 {
		if release.exists() {
			return Ok(());
		}

		time::sleep(Duration::from_millis(10)).await;
	}

	Err(StoreError::Incompatible("test post-commit barrier timed out".into()))
}

#[cfg(not(debug_assertions))]
async fn post_commit_test_barrier(_operation: &str) -> Result<(), StoreError> {
	Ok(())
}

#[cfg(debug_assertions)]
async fn blob_publish_test_barrier() -> Result<(), StoreError> {
	let Ok(root) = env::var("DECODEX_TEST_BLOB_RESTART_SYNC") else {
		return Ok(());
	};
	let root = PathBuf::from(root);

	fs::write(root.join("published"), b"published")
		.map_err(|_| StoreError::Incompatible("test blob publication barrier failed".into()))?;

	for _ in 0..3_000 {
		if root.join("continue").exists() {
			return Ok(());
		}

		time::sleep(Duration::from_millis(10)).await;
	}

	Err(StoreError::Incompatible("test blob publication barrier timed out".into()))
}

#[cfg(not(debug_assertions))]
async fn blob_publish_test_barrier() -> Result<(), StoreError> {
	Ok(())
}

async fn issue_history_cursor(
	transaction: &Transaction<'_>,
	conversation_id: &ConversationId,
	parent: Option<&HistoryCursor>,
	page_size: i32,
) -> Result<HistoryCursor, StoreError> {
	let parent_token = parent.map(|cursor| cursor.token.as_str());
	let row = transaction
		.query_one(
			"SELECT decodex.issue_history_cursor( \
			 $1::text::uuid,$2::text::uuid,$3)::text",
			&[&conversation_id.as_str(), &parent_token, &page_size],
		)
		.await?;

	HistoryCursor::issued(row.get(0))
}

async fn insert_verified_blob(
	transaction: &Transaction<'_>,
	hash: BlobHash,
	byte_length: i64,
) -> Result<(), StoreError> {
	let row = transaction
		.query_one(
			"WITH write_time AS (SELECT clock_timestamp() AS value), inserted AS ( \
		 INSERT INTO decodex.blob_objects (blob_hash, byte_length, verified_at, created_at) \
		 SELECT $1, $2, value, value FROM write_time ON CONFLICT (blob_hash) DO NOTHING \
		 RETURNING byte_length) \
		 SELECT byte_length FROM inserted UNION ALL \
		 SELECT byte_length FROM decodex.blob_objects WHERE blob_hash = $1 LIMIT 1",
			&[&hash.to_hex(), &byte_length],
		)
		.await?;
	let stored_length: i64 = row.get(0);

	if stored_length != byte_length {
		return Err(StoreError::Incompatible(
			"blob metadata conflicts with content address".into(),
		));
	}

	Ok(())
}

async fn reserve_conversation_command(
	client: &mut Client,
	command: &CommandIdentity,
	operation: &'static str,
	identity: (&'static str, &str, &str),
	expected_revision: Option<i64>,
	payload: Option<(BlobHash, i64)>,
) -> Result<CommandClaim, StoreError> {
	let (project_scope, scope_id, entity_id) = identity;

	accounts::reserve_command(
		client,
		command,
		&CommandDescriptor {
			protocol_version: "decodex/store-command/1",
			operation,
			project_scope,
			scope_id: scope_id.into(),
			entity_id: entity_id.into(),
			expected_revision,
			payload_hash: payload.map(|(hash, _)| hash.to_hex()),
			payload_length: payload.map(|(_, length)| length),
		},
	)
	.await
}

async fn insert_context_pack_sources(
	transaction: &Transaction<'_>,
	request: &PersistContextPack,
	pack: &ContextPack,
) -> Result<(), StoreError> {
	for (position, source) in pack.source_manifest().iter().enumerate() {
		let (artifact_id, artifact_revision) =
			source.artifact_reference().map_or((None, None), |(id, revision)| {
				(Some(id.as_str()), i64::try_from(revision).ok())
			});

		if let (Some(artifact_id), Some(artifact_revision)) = (artifact_id, artifact_revision) {
			let valid: bool = transaction
				.query_one(
					"SELECT EXISTS (SELECT 1 FROM decodex.artifact_revisions ar \
				 JOIN decodex.blob_objects bo ON bo.blob_hash=ar.blob_hash \
				 WHERE ar.artifact_id=$1::text::uuid AND ar.conversation_id=$2::text::uuid \
				 AND ar.revision=$3 AND ar.blob_hash=$4 AND bo.byte_length=$5)",
					&[
						&artifact_id,
						&pack.conversation_id().as_str(),
						&artifact_revision,
						&source.content_digest().to_hex(),
						&i64::try_from(source.original_byte_length()).unwrap_or(i64::MAX),
					],
				)
				.await?
				.get(0);

			if !valid {
				return Err(StoreError::InvalidInput(
					"Context Pack Artifact provenance is invalid",
				));
			}
		}

		transaction
			.execute(
				"INSERT INTO decodex.context_pack_sources \
			 (context_pack_id, conversation_id, position, kind, source_id, source_revision, \
			 content_digest, original_byte_length, included_byte_length, included_digest, \
			 disposition, artifact_id, artifact_revision) \
			 VALUES ($1::text::uuid, $2::text::uuid, $3, $4::text::decodex.context_source_kind, \
			 $5, $6, $7, $8, $9, $10, $11::text::decodex.context_source_disposition, $12::text::uuid, $13)",
				&[
					&request.context_pack_id,
					&pack.conversation_id().as_str(),
					&i32::try_from(position).unwrap_or(i32::MAX),
					&context_source_sql(source.kind()),
					&source.source_id(),
					&i64::try_from(source.revision()).unwrap_or(i64::MAX),
					&source.content_digest().to_hex(),
					&i64::try_from(source.original_byte_length()).unwrap_or(i64::MAX),
					&i64::try_from(source.included_byte_length()).unwrap_or(i64::MAX),
					&source.included_digest().to_hex(),
					&context_disposition_sql(source.disposition()),
					&artifact_id,
					&artifact_revision,
				],
			)
			.await?;
	}

	Ok(())
}

async fn ensure_turn(
	transaction: &Transaction<'_>,
	mutation: &RecordHistoryItem,
) -> Result<(), StoreError> {
	let inserted = transaction
		.execute(
			"INSERT INTO decodex.turns \
		 (turn_id, conversation_id, runtime_session_id, sequence, role, possible_side_effects) \
		 VALUES ($1::text::uuid, $2::text::uuid, $3::text::uuid, $4, \
		 $5::text::decodex.turn_role, $6::text::decodex.side_effect_state) \
		 ON CONFLICT (turn_id) DO NOTHING",
			&[
				&mutation.turn_id.as_str(),
				&mutation.conversation_id.as_str(),
				&mutation.runtime_session_id.as_str(),
				&mutation.turn_sequence,
				&turn_role_sql(mutation.turn_role),
				&side_effect_sql(mutation.possible_side_effects),
			],
		)
		.await?;

	if inserted == 1 {
		return Ok(());
	}

	let matches: bool = transaction
		.query_one(
			"SELECT conversation_id = $2::text::uuid AND runtime_session_id = $3::text::uuid \
		 AND sequence = $4 AND role = $5::text::decodex.turn_role \
		 AND possible_side_effects = $6::text::decodex.side_effect_state \
		 FROM decodex.turns WHERE turn_id = $1::text::uuid FOR UPDATE",
			&[
				&mutation.turn_id.as_str(),
				&mutation.conversation_id.as_str(),
				&mutation.runtime_session_id.as_str(),
				&mutation.turn_sequence,
				&turn_role_sql(mutation.turn_role),
				&side_effect_sql(mutation.possible_side_effects),
			],
		)
		.await?
		.get(0);

	if matches { Ok(()) } else { Err(StoreError::IdempotencyConflict) }
}

async fn insert_history_item(
	transaction: &Transaction<'_>,
	mutation: &RecordHistoryItem,
	blob: Option<(BlobHash, i64)>,
) -> Result<i64, StoreError> {
	let inline = blob.is_none().then_some(mutation.text.as_str());
	let blob_hash = blob.map(|(hash, _)| hash.to_hex());
	let metadata = history_metadata_json(&mutation.metadata)?;
	let (artifact_id, artifact_revision) = mutation
		.artifact
		.as_ref()
		.map_or((None, None), |(id, revision)| (Some(id.as_str()), Some(*revision)));
	let row = transaction
		.query_opt(
			"INSERT INTO decodex.history_items \
			 (history_item_id, conversation_id, history_position, turn_id, ordinal, kind, status, \
			 inline_text, blob_hash, media_type, metadata, artifact_id, artifact_revision) \
			 VALUES ($1::text::uuid, $2::text::uuid, 1, $3::text::uuid, $4, \
			 $5::text::decodex.history_item_kind, $6::text::decodex.history_item_status, \
			 $7, $8, $9, $10, $11::text::uuid, $12) \
			 ON CONFLICT DO NOTHING RETURNING revision",
			&[
				&mutation.history_item_id.as_str(),
				&mutation.conversation_id.as_str(),
				&mutation.turn_id.as_str(),
				&mutation.ordinal,
				&item_kind_sql(mutation.kind),
				&item_status_sql(mutation.status),
				&inline,
				&blob_hash,
				&mutation.media_type.as_str(),
				&metadata,
				&artifact_id,
				&artifact_revision,
			],
		)
		.await?;

	if let Some(row) = row {
		return Ok(row.get(0));
	}

	Err(history_revision_error(transaction, mutation, None).await?)
}

async fn update_history_item(
	transaction: &Transaction<'_>,
	mutation: &RecordHistoryItem,
	blob: Option<(BlobHash, i64)>,
	expected: i64,
) -> Result<i64, StoreError> {
	let inline = blob.is_none().then_some(mutation.text.as_str());
	let blob_hash = blob.map(|(hash, _)| hash.to_hex());
	let metadata = history_metadata_json(&mutation.metadata)?;
	let (artifact_id, artifact_revision) = mutation
		.artifact
		.as_ref()
		.map_or((None, None), |(id, revision)| (Some(id.as_str()), Some(*revision)));
	let row = transaction
		.query_opt(
			"UPDATE decodex.history_items SET status = $5::text::decodex.history_item_status, \
		 inline_text = $6, blob_hash = $7, media_type = $8, metadata = $9, \
		 revision = revision + 1, updated_at = clock_timestamp() \
			 WHERE history_item_id = $1::text::uuid AND turn_id = $2::text::uuid AND ordinal = $3 \
			 AND kind = $4::text::decodex.history_item_kind AND revision = $10 \
			 AND artifact_id IS NOT DISTINCT FROM $11::text::uuid \
			 AND artifact_revision IS NOT DISTINCT FROM $12 RETURNING revision",
			&[
				&mutation.history_item_id.as_str(),
				&mutation.turn_id.as_str(),
				&mutation.ordinal,
				&item_kind_sql(mutation.kind),
				&item_status_sql(mutation.status),
				&inline,
				&blob_hash,
				&mutation.media_type.as_str(),
				&metadata,
				&expected,
				&artifact_id,
				&artifact_revision,
			],
		)
		.await?;

	if let Some(row) = row {
		return Ok(row.get(0));
	}

	Err(history_revision_error(transaction, mutation, Some(expected)).await?)
}

async fn history_revision_error(
	transaction: &Transaction<'_>,
	mutation: &RecordHistoryItem,
	expected: Option<i64>,
) -> Result<StoreError, StoreError> {
	let actual = transaction
		.query_opt(
			"SELECT revision FROM decodex.history_items WHERE history_item_id = $1::text::uuid",
			&[&mutation.history_item_id.as_str()],
		)
		.await?
		.map(|row| row.get(0));

	Ok(StoreError::RevisionConflict {
		entity: format!("history_item/{}", mutation.history_item_id),
		expected,
		actual,
	})
}

async fn conversation_revision(
	transaction: &Transaction<'_>,
	id: &ConversationId,
) -> Result<Option<i64>, StoreError> {
	Ok(transaction
		.query_opt(
			"SELECT revision FROM decodex.conversations WHERE conversation_id = $1::text::uuid",
			&[&id.as_str()],
		)
		.await?
		.map(|row| row.get(0)))
}

#[cfg(test)]
mod tests {
	use crate::{StoreError, conversations};

	#[test]
	fn history_cursor_round_trips_and_rejects_ambiguous_forms() {
		let cursor =
			conversations::HistoryCursor::issued("00000000-0000-4000-8000-000000000009".into())
				.unwrap();

		assert_eq!(conversations::HistoryCursor::parse(&cursor.encode()).unwrap(), cursor);
		assert!(conversations::HistoryCursor::parse("v1:bad").is_err());
		assert!(
			conversations::HistoryCursor::parse("v1:00000000-0000-4000-8000-000000000009:extra")
				.is_err()
		);
		assert!(
			conversations::HistoryCursor::parse("v2:00000000-0000-4000-8000-000000000009").is_err()
		);
	}

	#[test]
	fn stored_history_response_revalidates_the_core_projection() {
		let valid = serde_json::json!({
			"kind":"history_item",
			"history_item_id":"44000000-0000-4000-8000-000000000009",
			"turn_id":"45000000-0000-4000-8000-000000000009",
			"runtime_session_id":"42000000-0000-4000-8000-000000000009",
			"turn_role":"assistant",
			"possible_side_effects":"none",
			"item_kind":"message",
			"status":"completed",
			"inline_text":"ok",
			"blob_hash":null,
			"blob_byte_length":null,
			"media_type":"application/json",
			"metadata":{"note":"secret sauce","summary":"token budget","visible":true},
			"artifact":null,
			"revision":1
		});

		assert!(conversations::history_entry_from_response(&valid).is_ok());

		for metadata in [
			serde_json::json!({"token":"ordinary"}),
			serde_json::json!({"note":"secret=abcd"}),
			serde_json::json!({"nested":{"unsafe":true}}),
		] {
			let mut malformed = valid.clone();

			malformed["metadata"] = metadata;

			assert!(matches!(
				conversations::history_entry_from_response(&malformed),
				Err(StoreError::Incompatible(_))
			));
		}
	}
}
