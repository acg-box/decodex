//! Executable real-PostgreSQL feasibility proof for XY-1264.

mod embedded {
	refinery::embed_migrations!("migrations");
}

use std::{
	collections::{HashSet, VecDeque},
	env,
	error::Error,
	fs::{self, File},
	io::{Read as _, Seek as _, SeekFrom},
	path::{Path, PathBuf},
	str::FromStr,
	time::{Duration, Instant},
};

use deadpool_postgres::{Client, Manager, ManagerConfig, Pool, RecyclingMethod};
use serde_json::{self, Value};
use sha2::{Digest, Sha256};
use tokio::{task::JoinSet, time};
use tokio_postgres::{Config, NoTls};

use embedded::migrations;

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

const CACHE_MAX_BYTES: usize = 2_048;
const CACHE_MAX_ENTRIES: usize = 8;

struct CacheEntry {
	path: PathBuf,
	bytes: usize,
}

struct BoundedCache {
	entries: VecDeque<CacheEntry>,
	root: PathBuf,
	total_bytes: usize,
}
impl BoundedCache {
	fn new(root: PathBuf) -> Result<Self> {
		fs::create_dir_all(&root)?;

		Ok(Self { entries: VecDeque::new(), root, total_bytes: 0 })
	}

	fn insert(&mut self, key: &str, value: &Value) -> Result<()> {
		let path = self.root.join(format!("{}.json", hex_sha256(key.as_bytes())));
		let body = serde_json::to_vec(value)?;

		if let Some(position) = self.entries.iter().position(|entry| entry.path == path) {
			let replaced = self.entries.remove(position).ok_or("cache entry disappeared")?;

			self.total_bytes -= replaced.bytes;
		}

		fs::write(&path, &body)?;

		self.total_bytes += body.len();

		self.entries.push_back(CacheEntry { path, bytes: body.len() });

		while self.entries.len() > CACHE_MAX_ENTRIES || self.total_bytes > CACHE_MAX_BYTES {
			let Some(expired) = self.entries.pop_front() else { break };

			fs::remove_file(expired.path)?;

			self.total_bytes -= expired.bytes;
		}

		Ok(())
	}
}

fn hex_sha256(bytes: &[u8]) -> String {
	hex_bytes(&Sha256::digest(bytes))
}

fn hex_bytes(bytes: &[u8]) -> String {
	const HEX: &[u8; 16] = b"0123456789abcdef";

	let mut encoded = String::with_capacity(64);

	for &byte in bytes {
		encoded.push(char::from(HEX[usize::from(byte >> 4)]));
		encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
	}

	encoded
}

fn authenticated_range(
	path: &Path,
	expected_hash: &str,
	expected_size: usize,
	presented_bearer: &str,
	expected_bearer: &str,
	start: usize,
	end_inclusive: usize,
) -> Result<Vec<u8>> {
	if presented_bearer != expected_bearer {
		return Err("unauthorized blob range read".into());
	}

	let mut file = File::open(path)?;
	let actual_size = usize::try_from(file.metadata()?.len())?;

	if actual_size != expected_size {
		return Err("blob integrity verification failed".into());
	}

	let mut hasher = Sha256::new();
	let mut buffer = [0_u8; 64 * 1_024];

	loop {
		let read = file.read(&mut buffer)?;

		if read == 0 {
			break;
		}

		hasher.update(&buffer[..read]);
	}

	if hex_bytes(&hasher.finalize()) != expected_hash {
		return Err("blob integrity verification failed".into());
	}
	if start > end_inclusive || end_inclusive >= actual_size {
		return Err("unsatisfiable blob byte range".into());
	}

	file.seek(SeekFrom::Start(u64::try_from(start)?))?;

	let mut range = vec![0_u8; end_inclusive - start + 1];

	file.read_exact(&mut range)?;

	Ok(range)
}

async fn concurrent_leases(pool: &Pool) -> Result<Value> {
	let started = Instant::now();
	let mut tasks = JoinSet::new();

	for _ in 0..32 {
		let pool = pool.clone();

		tasks.spawn(async move {
			let client = pool.get().await?;
			let row = client
				.query_one(
					"SELECT acquired FROM decodex.try_acquire_lease( \
					 'managed-run/1', gen_random_uuid(), interval '30 seconds')",
					&[],
				)
				.await?;

			Ok::<bool, Box<dyn Error + Send + Sync>>(row.get(0))
		});
	}

	let mut winners = 0;

	while let Some(result) = tasks.join_next().await {
		if result?? {
			winners += 1;
		}
	}

	if winners != 1 {
		return Err(format!("expected one lease winner, found {winners}").into());
	}

	let client = pool.get().await?;
	let rows: i64 = client
		.query_one("SELECT count(*) FROM decodex.leases WHERE resource_key = 'managed-run/1'", &[])
		.await?
		.get(0);

	if rows != 1 {
		return Err(format!("expected one lease row, found {rows}").into());
	}

	client
		.execute(
			"UPDATE decodex.leases SET expires_at = clock_timestamp() - interval '1 second'",
			&[],
		)
		.await?;

	let reclaimed: bool = client
		.query_one(
			"SELECT acquired FROM decodex.try_acquire_lease( \
			 'managed-run/1', gen_random_uuid(), interval '30 seconds')",
			&[],
		)
		.await?
		.get(0);

	if !reclaimed {
		return Err("expired lease was not reclaimable".into());
	}

	Ok(serde_json::json!({
		"contenders": 32,
		"winners": winners,
		"expired_lease_reclaimed": true,
		"elapsed_ms": started.elapsed().as_secs_f64() * 1_000.0,
	}))
}

async fn optimistic_revisions(pool: &Pool) -> Result<Value> {
	pool.get()
		.await?
		.execute(
			"INSERT INTO decodex.probe_entities (entity_key, value) \
			 VALUES ('revision-target', '{\"writer\": -1}')",
			&[],
		)
		.await?;

	let started = Instant::now();
	let mut tasks = JoinSet::new();

	for writer in 0..16_i64 {
		let pool = pool.clone();

		tasks.spawn(async move {
			let client = pool.get().await?;
			let updated = client
				.query_one(
					"SELECT decodex.update_probe_entity( \
					 'revision-target', 1, jsonb_build_object('writer', $1::bigint))",
					&[&writer],
				)
				.await
				.is_ok();

			Ok::<bool, Box<dyn Error + Send + Sync>>(updated)
		});
	}

	let mut winners = 0;

	while let Some(result) = tasks.join_next().await {
		if result?? {
			winners += 1;
		}
	}

	if winners != 1 {
		return Err(format!("expected one optimistic writer, found {winners}").into());
	}

	let revision: i64 = pool
		.get()
		.await?
		.query_one(
			"SELECT revision FROM decodex.probe_entities WHERE entity_key = 'revision-target'",
			&[],
		)
		.await?
		.get(0);

	if revision != 2 {
		return Err(format!("expected revision 2, found {revision}").into());
	}

	Ok(serde_json::json!({
		"contenders": 16,
		"winners": winners,
		"conflicts": 15,
		"elapsed_ms": started.elapsed().as_secs_f64() * 1_000.0,
	}))
}

async fn idempotent_commands(pool: &Pool) -> Result<Value> {
	pool.get()
		.await?
		.execute(
			"INSERT INTO decodex.probe_entities (entity_key, value) \
			 VALUES ('command-target', '{\"state\": \"before\"}')",
			&[],
		)
		.await?;

	let request_hash = hex_sha256(b"command-target:after");
	let started = Instant::now();
	let mut tasks = JoinSet::new();

	for _ in 0..16 {
		let pool = pool.clone();
		let request_hash = request_hash.clone();

		tasks.spawn(async move {
			let client = pool.get().await?;
			let row = client
				.query_one(
					"SELECT decodex.apply_probe_command( \
					 'command-1', $1, 'command-target', 1, '{\"state\": \"after\"}')",
					&[&request_hash],
				)
				.await?;

			Ok::<Value, Box<dyn Error + Send + Sync>>(row.get(0))
		});
	}

	let mut responses = HashSet::new();

	while let Some(result) = tasks.join_next().await {
		responses.insert(result??.to_string());
	}

	if responses.len() != 1 {
		return Err("duplicate submissions returned different responses".into());
	}

	let client = pool.get().await?;
	let revision: i64 = client
		.query_one(
			"SELECT revision FROM decodex.probe_entities WHERE entity_key = 'command-target'",
			&[],
		)
		.await?
		.get(0);
	let events: i64 = client
		.query_one(
			"SELECT count(*) FROM decodex.outbox WHERE aggregate_key = 'command-target'",
			&[],
		)
		.await?
		.get(0);

	if revision != 2 || events != 1 {
		return Err(
			format!("idempotent command produced revision {revision} and {events} events").into()
		);
	}

	let conflict_hash = hex_sha256(b"different-request");
	let conflict = client
		.query_one(
			"SELECT decodex.apply_probe_command( \
			 'command-1', $1, 'command-target', 2, '{}')",
			&[&conflict_hash],
		)
		.await;

	if conflict.is_ok() {
		return Err("idempotency key reuse with a different request hash succeeded".into());
	}

	Ok(serde_json::json!({
		"duplicate_submissions": 16,
		"mutations": 1,
		"outbox_rows": 1,
		"conflicting_payload_rejected": true,
		"elapsed_ms": started.elapsed().as_secs_f64() * 1_000.0,
	}))
}

async fn outbox_concurrency(pool: &Pool) -> Result<Value> {
	pool.get()
		.await?
		.execute(
			"INSERT INTO decodex.outbox (aggregate_key, payload) \
			 SELECT 'bulk/' || value, jsonb_build_object('kind', 'bulk', 'value', value) \
			 FROM generate_series(1, 1000) AS value",
			&[],
		)
		.await?;

	let started = Instant::now();
	let mut tasks = JoinSet::new();

	for _ in 0..8 {
		let pool = pool.clone();

		tasks.spawn(async move {
			let client = pool.get().await?;
			let rows = client
				.query(
					"SELECT id FROM decodex.claim_outbox( \
					 gen_random_uuid(), 200, interval '30 seconds')",
					&[],
				)
				.await?;

			Ok::<Vec<i64>, Box<dyn Error + Send + Sync>>(
				rows.into_iter().map(|row| row.get(0)).collect(),
			)
		});
	}

	let mut claims = Vec::new();

	while let Some(result) = tasks.join_next().await {
		claims.extend(result??);
	}

	let unique = claims.iter().copied().collect::<HashSet<_>>();

	if claims.len() != 1_001 || unique.len() != claims.len() {
		return Err(format!(
			"expected 1001 unique claims, found {} / {} unique",
			claims.len(),
			unique.len()
		)
		.into());
	}

	let elapsed = started.elapsed().as_secs_f64();

	Ok(serde_json::json!({
		"workers": 8,
		"claimed": claims.len(),
		"duplicate_claims": 0,
		"elapsed_ms": elapsed * 1_000.0,
		"claims_per_second": claims.len() as f64 / elapsed,
	}))
}

async fn outbox_retry(pool: &Pool) -> Result<Value> {
	let client = pool.get().await?;
	let retry_id: i64 = client
		.query_one(
			"INSERT INTO decodex.outbox (aggregate_key, payload) \
			 VALUES ('ordinary-retry', '{\"kind\": \"retry\"}') RETURNING id",
			&[],
		)
		.await?
		.get(0);
	let claimed: i64 = client
		.query_one(
			"SELECT id FROM decodex.claim_outbox( \
			 '12345678-1234-1234-1234-123456789abc', 1, interval '30 seconds') \
			 WHERE id = $1",
			&[&retry_id],
		)
		.await?
		.get(0);

	if claimed != retry_id {
		return Err("ordinary retry fixture was not claimed".into());
	}

	let retried: bool = client
		.query_one(
			"SELECT decodex.retry_outbox($1, '12345678-1234-1234-1234-123456789abc', \
			 'temporary_failure', interval '50 milliseconds')",
			&[&retry_id],
		)
		.await?
		.get(0);

	if !retried {
		return Err("worker-owned outbox retry transition failed".into());
	}

	let before_delay = client
		.query(
			"SELECT id FROM decodex.claim_outbox( \
			 '12345678-1234-1234-1234-123456789abc', 1, interval '30 seconds') WHERE id = $1",
			&[&retry_id],
		)
		.await?;

	if !before_delay.is_empty() {
		return Err("outbox retry was claimable before its delay".into());
	}

	let recorded_error: String = client
		.query_one("SELECT last_error FROM decodex.outbox WHERE id = $1", &[&retry_id])
		.await?
		.get(0);

	time::sleep(Duration::from_millis(70)).await;

	let row = client
		.query_one(
			"SELECT id, attempt_count FROM decodex.claim_outbox( \
			 '12345678-1234-1234-1234-123456789abc', 1, interval '30 seconds') \
			 WHERE id = $1",
			&[&retry_id],
		)
		.await?;
	let attempt_count: i32 = row.get(1);

	if row.get::<_, i64>(0) != retry_id
		|| attempt_count != 2
		|| recorded_error != "temporary_failure"
	{
		return Err("delayed outbox retry did not preserve error and increment attempt".into());
	}

	client
		.query_one(
			"SELECT decodex.complete_outbox( \
			 $1, '12345678-1234-1234-1234-123456789abc')",
			&[&retry_id],
		)
		.await?;

	Ok(serde_json::json!({
		"recorded_error": recorded_error,
		"not_claimable_before_delay": true,
		"reclaimed_attempt": attempt_count,
		"completed": true,
	}))
}

async fn prove_retention_deletion(
	client: &mut Client,
	path: &Path,
	content_hash: &str,
) -> Result<()> {
	client
		.execute(
			"UPDATE decodex.artifacts SET state = 'expired', \
			 delete_after = clock_timestamp() + interval '1 hour' WHERE content_hash = $1",
			&[&content_hash],
		)
		.await?;

	let deletion = client.transaction().await?;
	let not_due = deletion
		.query_opt(
			"SELECT relative_path FROM decodex.artifacts WHERE content_hash = $1 \
			 AND state = 'expired' AND delete_after <= clock_timestamp() FOR UPDATE",
			&[&content_hash],
		)
		.await?;

	deletion.rollback().await?;

	if not_due.is_some() || !path.exists() {
		return Err("not-yet-due blob became eligible for byte deletion".into());
	}

	client
		.execute(
			"UPDATE decodex.artifacts SET delete_after = clock_timestamp() - interval '1 second' \
			 WHERE content_hash = $1 AND state = 'expired'",
			&[&content_hash],
		)
		.await?;

	let deletion = client.transaction().await?;
	let due = deletion
		.query_opt(
			"SELECT relative_path FROM decodex.artifacts WHERE content_hash = $1 \
			 AND state = 'expired' AND delete_after <= clock_timestamp() FOR UPDATE",
			&[&content_hash],
		)
		.await?;

	if due.is_none() {
		return Err("due blob did not become eligible for deletion".into());
	}

	fs::remove_file(path)?;

	deletion
		.execute(
			"UPDATE decodex.artifacts SET state = 'deleted', deleted_at = clock_timestamp() \
			 WHERE content_hash = $1 AND state = 'expired'",
			&[&content_hash],
		)
		.await?;
	deletion.commit().await?;

	let state: String = client
		.query_one(
			"SELECT state::text FROM decodex.artifacts WHERE content_hash = $1",
			&[&content_hash],
		)
		.await?
		.get(0);

	if state != "deleted" || path.exists() {
		return Err("blob retention deletion did not retain only a tombstone".into());
	}

	Ok(())
}

async fn prove_credential_constraints(client: &Client) -> Result<()> {
	let forbidden = serde_json::json!({"nested": {"refresh_token": "forbidden"}});

	for statement in [
		"INSERT INTO decodex.probe_entities (entity_key, value) VALUES (gen_random_uuid()::text, $1)",
		"INSERT INTO decodex.outbox (aggregate_key, payload) VALUES ('credential-negative', $1)",
		"INSERT INTO decodex.command_receipts (idempotency_key, request_hash, response) \
		 VALUES (gen_random_uuid()::text, repeat('a', 64), $1)",
	] {
		let result = client.execute(statement, &[&forbidden]).await;

		if result.is_ok() {
			return Err("credential-shaped JSON entered an ordinary row".into());
		}
	}

	Ok(())
}

async fn blob_boundary(pool: &Pool, root: &Path) -> Result<Value> {
	let content = b"decodex-vnext-content-addressed-proof\n";
	let content_hash = hex_sha256(content);
	let relative = format!("blobs/sha256/{}/{}", &content_hash[..2], content_hash);
	let path = root.join(&relative);

	fs::create_dir_all(path.parent().ok_or("blob path has no parent")?)?;
	fs::write(&path, content)?;

	let byte_size = i64::try_from(content.len())?;

	pool.get()
		.await?
		.execute(
			"INSERT INTO decodex.artifacts \
			 (content_hash, byte_size, relative_path, integrity_verified_at) \
			 VALUES ($1, $2, $3, clock_timestamp())",
			&[&content_hash, &byte_size, &relative],
		)
		.await?;

	if authenticated_range(&path, &content_hash, content.len(), "wrong", "proof-token", 0, 6)
		.is_ok()
	{
		return Err("unauthenticated range read succeeded".into());
	}

	let range = authenticated_range(
		&path,
		&content_hash,
		content.len(),
		"proof-token",
		"proof-token",
		8,
		12,
	)?;

	if range != b"vnext" {
		return Err("authenticated byte range returned wrong content".into());
	}

	fs::write(&path, [content.as_slice(), b"tamper"].concat())?;

	if authenticated_range(&path, &content_hash, content.len(), "proof-token", "proof-token", 0, 1)
		.is_ok()
	{
		return Err("tampered blob passed integrity verification".into());
	}

	fs::remove_file(&path)?;

	if authenticated_range(&path, &content_hash, content.len(), "proof-token", "proof-token", 0, 1)
		.is_ok()
	{
		return Err("missing blob passed availability verification".into());
	}

	fs::write(&path, content)?;

	let mut client = pool.get().await?;

	prove_retention_deletion(&mut client, &path, &content_hash).await?;
	prove_credential_constraints(&client).await?;

	Ok(serde_json::json!({
		"algorithm": "sha256",
		"bytes": content.len(),
		"authenticated_range_read": true,
		"missing_detected": true,
		"tamper_detected": true,
		"retention_deleted_bytes": true,
		"not_due_bytes_preserved": true,
		"metadata_tombstone_retained": true,
		"credential_payloads_rejected": 3,
	}))
}

async fn bounded_cache(pool: &Pool, root: &Path) -> Result<Value> {
	let client = pool.get().await?;

	client
		.execute(
			"INSERT INTO decodex.probe_entities (entity_key, value) \
			 SELECT 'cache/' || value, \
			 jsonb_build_object('source', 'postgres', 'sequence', value, \
			 'padding', repeat('x', 96)) FROM generate_series(1, 1000) AS value",
			&[],
		)
		.await?;

	let rows = client
		.query(
			"SELECT entity_key, revision, value FROM decodex.probe_entities \
			 WHERE entity_key LIKE 'cache/%' ORDER BY entity_key",
			&[],
		)
		.await?;
	let cache_root = root.join("cache");
	let mut cache = BoundedCache::new(cache_root.clone())?;

	for row in rows {
		let key: String = row.get(0);
		let revision: i64 = row.get(1);
		let value: Value = row.get(2);

		cache.insert(
			&key,
			&serde_json::json!({
				"server": "local-proof",
				"schema": "V1__bootstrap",
				"key": key,
				"revision": revision,
				"value": value,
			}),
		)?;
	}

	if cache.entries.len() > CACHE_MAX_ENTRIES || cache.total_bytes > CACHE_MAX_BYTES {
		return Err("bounded cache exceeded its contract".into());
	}

	let retained_entries = cache.entries.len();
	let retained_bytes = cache.total_bytes;

	drop(cache);

	fs::remove_dir_all(&cache_root)?;

	let authority_rows: i64 = client
		.query_one(
			"SELECT count(*) FROM decodex.probe_entities WHERE entity_key LIKE 'cache/%'",
			&[],
		)
		.await?
		.get(0);

	if authority_rows != 1_000 {
		return Err("cache deletion affected PostgreSQL authority".into());
	}

	let rebuilt: Value = client
		.query_one("SELECT value FROM decodex.probe_entities WHERE entity_key = 'cache/20'", &[])
		.await?
		.get(0);
	let mut cache = BoundedCache::new(cache_root)?;

	cache.insert("cache/20", &rebuilt)?;

	let replacement = serde_json::json!({"replacement": true});

	cache.insert("cache/20", &replacement)?;

	if cache.entries.len() != 1 || cache.total_bytes != serde_json::to_vec(&replacement)?.len() {
		return Err("cache key replacement violated entry or byte cap accounting".into());
	}

	Ok(serde_json::json!({
		"input_rows": authority_rows,
		"max_entries": CACHE_MAX_ENTRIES,
		"max_bytes": CACHE_MAX_BYTES,
		"retained_entries": retained_entries,
		"retained_bytes": retained_bytes,
		"delete_and_rebuild": true,
		"key_replacement": true,
	}))
}

#[tokio::main]
async fn main() -> Result<()> {
	let database_url = env::var("DATABASE_URL")?;
	let proof_root = PathBuf::from(env::var("DECODEX_PROOF_ROOT")?);
	let config = Config::from_str(&database_url)?;
	let manager = Manager::from_config(
		config,
		NoTls,
		ManagerConfig { recycling_method: RecyclingMethod::Fast },
	);
	let pool = Pool::builder(manager).max_size(32).build()?;
	let mut migration_client = pool.get().await?;

	migrations::runner().run_async(&mut **migration_client).await?;

	drop(migration_client);

	let first = pool.get().await?;
	let second = pool.get().await?;

	drop((first, second));

	let version = pool
		.get()
		.await?
		.query_one(
			"SELECT current_setting('server_version'), extversion, current_setting('data_checksums') \
			 FROM pg_extension WHERE extname = 'pgcrypto'",
			&[],
		)
		.await?;
	let receipt = serde_json::json!({
		"driver": "tokio-postgres 0.7.18 via deadpool-postgres 0.14.1",
		"migration_tooling": "refinery 0.9.2 embedded forward migrations with checksums",
		"pool": {"prewarmed_connections": 2, "maximum_connections": 32},
		"postgres_version": version.get::<_, String>(0),
		"pgcrypto_version": version.get::<_, String>(1),
		"data_checksums": version.get::<_, String>(2),
		"concurrent_leases": concurrent_leases(&pool).await?,
		"optimistic_revisions": optimistic_revisions(&pool).await?,
		"idempotent_commands": idempotent_commands(&pool).await?,
		"outbox_concurrency": outbox_concurrency(&pool).await?,
		"outbox_retry": outbox_retry(&pool).await?,
		"blob_boundary": blob_boundary(&pool, &proof_root).await?,
		"bounded_cache": bounded_cache(&pool, &proof_root).await?,
	});

	println!("{}", serde_json::to_string_pretty(&receipt)?);

	pool.close();

	Ok(())
}
