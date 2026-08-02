//! One-time clean-start activation for strict Radar pair lineage.

use std::{
	ffi::OsStr,
	fs::File,
	path::{Path, PathBuf},
};

use serde::Serialize;
use serde_json::Value;

use crate::{
	SOCIAL_CANDIDATE_SCHEMA, SOCIAL_POST_SCHEMA,
	filesystem::{PinnedPrivateDirectory, PrivateFileIdentity, PrivateTreeSnapshot},
	prelude::{Result, eyre},
};

const REPORT_SCHEMA: &str = "decodex_social_content_v2_reset/v1";
const MARKER_SCHEMA: &str = "decodex_social_content_v2_activation/v1";
const CACHE_RELATIVE_PATH: &str = ".agent/automations/decodex/cache";
const MARKER_NAME: &str = "content-v2-activation.json";
const LOCKS_RELATIVE_PATH: &str = "social/x/locks";
const LOCK_NAME: &str = ".social-state-mutation.lock";
const MAX_RESET_ENTRIES: usize = 8192;
const MAX_RESET_FILES: usize = 4096;
const MAX_RESET_BYTES: u64 = 64 * 1024 * 1024;
const RESET_COLLECTIONS: [ResetCollection; 7] = [
	ResetCollection::quality_skip("social/x/candidates", QualitySkipKind::Candidate),
	ResetCollection::quality_skip("social/x/posts", QualitySkipKind::Post),
	ResetCollection::empty("social/x/outcomes"),
	ResetCollection::empty("social/x/reservations"),
	ResetCollection::empty("social/x/xurl-attempts"),
	ResetCollection::empty("manager/strategy"),
	ResetCollection::empty("manager/staging"),
];

#[derive(Clone, Copy)]
struct ResetCollection {
	relative_path: &'static str,
	authority: ResetAuthority,
}

impl ResetCollection {
	const fn quality_skip(relative_path: &'static str, kind: QualitySkipKind) -> Self {
		Self { relative_path, authority: ResetAuthority::QualitySkip(kind) }
	}

	const fn empty(relative_path: &'static str) -> Self {
		Self { relative_path, authority: ResetAuthority::MustBeEmpty }
	}
}

#[derive(Clone, Copy)]
enum ResetAuthority {
	QualitySkip(QualitySkipKind),
	MustBeEmpty,
}

#[derive(Clone, Copy)]
enum QualitySkipKind {
	Candidate,
	Post,
}

#[derive(Clone, Copy)]
struct ResetLimits {
	entries: usize,
	files: usize,
	bytes: u64,
}

impl ResetLimits {
	const PRODUCTION: Self =
		Self { entries: MAX_RESET_ENTRIES, files: MAX_RESET_FILES, bytes: MAX_RESET_BYTES };
}

struct ResetInventory {
	collections: Vec<CollectionInventory>,
	files: usize,
	directories: usize,
	bytes: u64,
}

struct CollectionInventory {
	collection: ResetCollection,
	snapshot: Option<PrivateTreeSnapshot>,
}

#[derive(Clone, PartialEq)]
struct MarkerSnapshot {
	identity: PrivateFileIdentity,
	bytes: Vec<u8>,
}

#[derive(Debug)]
pub(crate) struct SocialContentV2ResetRequest {
	pub(crate) root: PathBuf,
}

#[derive(Debug, Eq, PartialEq, Serialize)]
pub(crate) struct SocialContentV2ResetReport {
	pub(crate) schema: String,
	pub(crate) status: String,
	pub(crate) collections_cleared: usize,
	pub(crate) files_removed: usize,
	pub(crate) directories_removed: usize,
	pub(crate) bytes_removed: u64,
}

pub(crate) fn reset_social_content_v2(
	request: &SocialContentV2ResetRequest,
) -> Result<SocialContentV2ResetReport> {
	reset_social_content_v2_with(request, ResetLimits::PRODUCTION, || {})
}

fn reset_social_content_v2_with(
	request: &SocialContentV2ResetRequest,
	limits: ResetLimits,
	after_preflight: impl FnOnce(),
) -> Result<SocialContentV2ResetReport> {
	let root = reset_repository_root(request)?;
	let cache =
		crate::filesystem::open_or_create_exact_private_directory(&root.join(CACHE_RELATIVE_PATH))?;
	let locks = cache.open_descendant_directory(Path::new(LOCKS_RELATIVE_PATH), true)?;
	let (lock, lock_identity) = locks.open_or_create_lock(OsStr::new(LOCK_NAME))?;
	lock.lock()?;
	verify_reset_authority(&cache, &locks, lock_identity, &lock)?;

	let marker_value = serde_json::json!({"schema": MARKER_SCHEMA, "status": "active"});
	let marker_bytes = pretty_json_bytes(&marker_value)?;
	if read_marker(&cache, &marker_bytes)?.is_some() {
		verify_reset_authority(&cache, &locks, lock_identity, &lock)?;
		return Ok(report("already_active", 0, 0, 0, 0));
	}
	let inventory = inventory_reset_collections(&cache, limits)?;
	preflight_deletion_authority(&cache, &inventory)?;
	after_preflight();
	verify_reset_authority(&cache, &locks, lock_identity, &lock)?;
	if read_marker(&cache, &marker_bytes)?.is_some() {
		eyre::bail!("Publisher content-v2 activation marker changed during reset preflight");
	}
	let current = inventory_reset_collections(&cache, limits)?;
	if !same_inventory(&inventory, &current) {
		eyre::bail!("Publisher content-v2 reset collections changed during preflight");
	}

	let collection_count =
		inventory.collections.iter().filter(|collection| collection.snapshot.is_some()).count();
	for collection in &inventory.collections {
		if let Some(snapshot) = &collection.snapshot {
			cache.remove_descendant_tree_if_matches(
				Path::new(collection.collection.relative_path),
				snapshot,
			)?;
			verify_reset_authority(&cache, &locks, lock_identity, &lock)?;
		}
	}
	cache.write_new_json(OsStr::new(MARKER_NAME), &marker_value)?;
	let installed = read_marker(&cache, &marker_bytes)?
		.ok_or_else(|| eyre::eyre!("Publisher content-v2 activation marker was not installed"))?;
	if installed.bytes != marker_bytes {
		eyre::bail!("Publisher content-v2 activation marker readback mismatch");
	}
	verify_reset_authority(&cache, &locks, lock_identity, &lock)?;

	Ok(report("reset", collection_count, inventory.files, inventory.directories, inventory.bytes))
}

fn reset_repository_root(request: &SocialContentV2ResetRequest) -> Result<PathBuf> {
	let detected = crate::filesystem::repo_root()?;
	if request.root == detected {
		return Ok(detected);
	}
	#[cfg(test)]
	if request.root.starts_with(detected.join("target"))
		&& crate::filesystem::open_existing_exact_private_directory(&request.root)?.is_some()
	{
		return Ok(request.root.clone());
	}

	eyre::bail!("Publisher content-v2 reset root must be the detected repository root")
}

fn verify_reset_authority(
	cache: &PinnedPrivateDirectory,
	locks: &PinnedPrivateDirectory,
	lock_identity: PrivateFileIdentity,
	lock: &File,
) -> Result<()> {
	cache.verify_current_path()?;
	locks.verify_current_path()?;
	locks.verify_lock(OsStr::new(LOCK_NAME), lock_identity)?;
	if PrivateFileIdentity::from_metadata(&lock.metadata()?) != lock_identity {
		eyre::bail!("Publisher content-v2 reset lock descriptor changed");
	}
	cache.verify_current_path()
}

fn read_marker(cache: &PinnedPrivateDirectory, expected: &[u8]) -> Result<Option<MarkerSnapshot>> {
	let Some((value, identity, bytes)) =
		cache.read_optional_json(OsStr::new(MARKER_NAME), expected.len() as u64)?
	else {
		return Ok(None);
	};
	if bytes != expected
		|| value.get("schema").and_then(Value::as_str) != Some(MARKER_SCHEMA)
		|| value.get("status").and_then(Value::as_str) != Some("active")
	{
		eyre::bail!("Publisher content-v2 activation marker is invalid");
	}

	Ok(Some(MarkerSnapshot { identity, bytes }))
}

fn inventory_reset_collections(
	cache: &PinnedPrivateDirectory,
	limits: ResetLimits,
) -> Result<ResetInventory> {
	let mut collections = Vec::with_capacity(RESET_COLLECTIONS.len());
	let mut files = 0_usize;
	let mut directories = 0_usize;
	let mut bytes = 0_u64;
	for collection in RESET_COLLECTIONS {
		let used_entries = files
			.checked_add(directories)
			.ok_or_else(|| eyre::eyre!("Publisher content-v2 reset entry count overflowed"))?;
		let remaining_entries = limits
			.entries
			.checked_sub(used_entries)
			.ok_or_else(|| eyre::eyre!("Publisher content-v2 reset entry bound was exceeded"))?;
		let remaining_files = limits
			.files
			.checked_sub(files)
			.ok_or_else(|| eyre::eyre!("Publisher content-v2 reset file bound was exceeded"))?;
		let remaining_bytes = limits
			.bytes
			.checked_sub(bytes)
			.ok_or_else(|| eyre::eyre!("Publisher content-v2 reset byte bound was exceeded"))?;
		let snapshot = cache.inspect_descendant_tree(
			Path::new(collection.relative_path),
			remaining_entries,
			remaining_files,
			remaining_bytes,
		)?;
		if let Some(snapshot) = &snapshot {
			files = files
				.checked_add(snapshot.file_count())
				.ok_or_else(|| eyre::eyre!("Publisher content-v2 reset file count overflowed"))?;
			directories = directories.checked_add(snapshot.directory_count()).ok_or_else(|| {
				eyre::eyre!("Publisher content-v2 reset directory count overflowed")
			})?;
			bytes = bytes
				.checked_add(snapshot.byte_count())
				.ok_or_else(|| eyre::eyre!("Publisher content-v2 reset byte count overflowed"))?;
		}
		collections.push(CollectionInventory { collection, snapshot });
	}

	Ok(ResetInventory { collections, files, directories, bytes })
}

fn preflight_deletion_authority(
	cache: &PinnedPrivateDirectory,
	inventory: &ResetInventory,
) -> Result<()> {
	for item in &inventory.collections {
		let Some(snapshot) = &item.snapshot else {
			continue;
		};
		match item.collection.authority {
			ResetAuthority::MustBeEmpty if snapshot.file_count() != 0 => {
				eyre::bail!(
					"Publisher content-v2 reset found non-quality-skip authority in {}",
					item.collection.relative_path
				);
			},
			ResetAuthority::MustBeEmpty => {},
			ResetAuthority::QualitySkip(kind) => {
				let directory = cache
					.open_descendant_directory(Path::new(item.collection.relative_path), false)?;
				for record in directory.read_json_tree_if_matches(snapshot)? {
					if record.relative_path.extension().and_then(OsStr::to_str) != Some("json") {
						eyre::bail!("Publisher content-v2 reset record is not JSON");
					}
					require_zero_effect_quality_skip(&record.payload, kind)?;
				}
			},
		}
	}

	Ok(())
}

fn require_zero_effect_quality_skip(payload: &Value, kind: QualitySkipKind) -> Result<()> {
	let object = payload
		.as_object()
		.ok_or_else(|| eyre::eyre!("Publisher reset quality-skip record must be an object"))?;
	let decision = object
		.get("decision")
		.and_then(Value::as_object)
		.ok_or_else(|| eyre::eyre!("Publisher reset quality-skip decision is missing"))?;
	if decision.get("worthiness").and_then(Value::as_str) != Some("skip") {
		eyre::bail!("Publisher reset record is not a quality skip");
	}
	for field in ["idempotency_key", "reason"] {
		if decision.get(field).and_then(Value::as_str).is_none_or(|value| value.trim().is_empty()) {
			eyre::bail!("Publisher reset quality-skip decision is incomplete");
		}
	}
	match kind {
		QualitySkipKind::Candidate => {
			if object.get("schema").and_then(Value::as_str) != Some(SOCIAL_CANDIDATE_SCHEMA)
				|| object.contains_key("status")
			{
				eyre::bail!("Publisher reset candidate is not a zero-effect quality skip");
			}
			reject_nested_effect_states(payload, false)?;
		},
		QualitySkipKind::Post => {
			let skip_reason = object
				.get("skip")
				.and_then(Value::as_object)
				.and_then(|skip| skip.get("reason"))
				.and_then(Value::as_str);
			if object.get("schema").and_then(Value::as_str) != Some(SOCIAL_POST_SCHEMA)
				|| object.get("status").and_then(Value::as_str) != Some("skipped")
				|| skip_reason.is_none_or(|value| value.trim().is_empty())
				|| decision.get("daily_count_before").and_then(Value::as_u64) != Some(0)
				|| decision.get("daily_count_after").and_then(Value::as_u64) != Some(0)
			{
				eyre::bail!("Publisher reset post is not a zero-effect quality skip");
			}
			reject_nested_effect_states(payload, true)?;
		},
	}
	reject_external_effect_authority(payload)
}

fn reject_nested_effect_states(value: &Value, allow_root_skipped_status: bool) -> Result<()> {
	fn visit(value: &Value, root: bool, allow_root_skipped_status: bool) -> Result<()> {
		match value {
			Value::Object(object) =>
				for (key, child) in object {
					let lower = key.to_ascii_lowercase();
					let allowed =
						root && allow_root_skipped_status
							&& lower == "status" && child.as_str() == Some("skipped");
					if !allowed
						&& (lower == "status"
							|| lower == "state" || lower.ends_with("_status")
							|| lower.ends_with("_state"))
					{
						eyre::bail!("Publisher reset record contains effect-state authority");
					}
					visit(child, false, allow_root_skipped_status)?;
				},
			Value::Array(values) =>
				for child in values {
					visit(child, false, allow_root_skipped_status)?;
				},
			Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {},
		}

		Ok(())
	}

	visit(value, true, allow_root_skipped_status)
}

fn reject_external_effect_authority(value: &Value) -> Result<()> {
	match value {
		Value::Object(object) =>
			for (key, child) in object {
				let key = key.to_ascii_lowercase();
				if matches!(
					key.as_str(),
					"attempt"
						| "attempts" | "created"
						| "inflight" | "publication"
						| "published" | "post_id"
						| "published_url" | "published_urls"
						| "post_ids" | "call"
						| "calls" | "x_calls"
						| "reservation_ref"
						| "reservations" | "uncertain"
				) || key.ends_with("post_id")
					|| key.ends_with("_attempt")
					|| key.ends_with("_attempts")
					|| key.ends_with("_calls")
					|| key.ends_with("_call_count")
					|| key.starts_with("reservation_")
					|| key.starts_with("reserved_")
					|| key.starts_with("created_")
					|| key.starts_with("published_")
					|| key.contains("cost")
					|| key.contains("budget")
					|| key.contains("spend")
				{
					eyre::bail!("Publisher reset record contains external-effect authority");
				}
				reject_external_effect_authority(child)?;
			},
		Value::Array(values) =>
			for child in values {
				reject_external_effect_authority(child)?;
			},
		Value::String(state)
			if matches!(
				state.to_ascii_lowercase().as_str(),
				"created" | "inflight" | "published" | "uncertain"
			) =>
		{
			eyre::bail!("Publisher reset record contains external-effect state");
		},
		Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {},
	}

	Ok(())
}

fn same_inventory(left: &ResetInventory, right: &ResetInventory) -> bool {
	left.files == right.files
		&& left.directories == right.directories
		&& left.bytes == right.bytes
		&& left.collections.len() == right.collections.len()
		&& left.collections.iter().zip(&right.collections).all(|(left, right)| {
			left.collection.relative_path == right.collection.relative_path
				&& left.snapshot == right.snapshot
		})
}

fn pretty_json_bytes(value: &serde_json::Value) -> Result<Vec<u8>> {
	let mut bytes = serde_json::to_vec_pretty(value)?;
	bytes.push(b'\n');

	Ok(bytes)
}

fn report(
	status: &str,
	collections_cleared: usize,
	files_removed: usize,
	directories_removed: usize,
	bytes_removed: u64,
) -> SocialContentV2ResetReport {
	SocialContentV2ResetReport {
		schema: REPORT_SCHEMA.into(),
		status: status.into(),
		collections_cleared,
		files_removed,
		directories_removed,
		bytes_removed,
	}
}

#[cfg(test)]
mod tests {
	use std::{
		fs,
		os::unix::fs::{PermissionsExt as _, symlink},
	};

	use serde_json::json;

	use super::{
		CACHE_RELATIVE_PATH, LOCKS_RELATIVE_PATH, MARKER_NAME, RESET_COLLECTIONS, ResetLimits,
		SocialContentV2ResetRequest,
	};

	fn reset_root(prefix: &str) -> tempfile::TempDir {
		let temp = crate::repo_local_test_directory(prefix);
		fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700))
			.expect("reset test root mode");
		temp
	}

	fn cache_path(root: &std::path::Path) -> std::path::PathBuf {
		root.join(CACHE_RELATIVE_PATH)
	}

	fn collection_path(root: &std::path::Path, index: usize) -> std::path::PathBuf {
		cache_path(root).join(RESET_COLLECTIONS[index].relative_path)
	}

	fn marker_path(root: &std::path::Path) -> std::path::PathBuf {
		cache_path(root).join(MARKER_NAME)
	}

	fn quality_skip_candidate() -> serde_json::Value {
		json!({
			"schema": "social_candidate/v1",
			"decision": {
				"worthiness": "skip",
				"idempotency_key": "quality-skip",
				"reason": "not publish-worthy"
			}
		})
	}

	fn quality_skip_post() -> serde_json::Value {
		json!({
			"schema": "social_post/v1",
			"status": "skipped",
			"decision": {
				"worthiness": "skip",
				"idempotency_key": "quality-skip",
				"reason": "not publish-worthy",
				"daily_count_before": 0,
				"daily_count_after": 0
			},
			"skip": {"reason": "not publish-worthy"}
		})
	}

	fn install_safe_collections(root: &std::path::Path) {
		crate::write_new_json(
			&collection_path(root, 0).join("candidate.json"),
			&quality_skip_candidate(),
		)
		.expect("quality-skip candidate fixture");
		crate::write_new_json(&collection_path(root, 1).join("post.json"), &quality_skip_post())
			.expect("quality-skip post fixture");
		for index in 2..RESET_COLLECTIONS.len() {
			let placeholder = collection_path(root, index).join("placeholder.json");
			crate::write_new_json(&placeholder, &json!({"placeholder": true}))
				.expect("empty collection fixture");
			fs::remove_file(placeholder).expect("empty collection placeholder removal");
		}
	}

	fn assert_all_collections_present(root: &std::path::Path) {
		for collection in RESET_COLLECTIONS {
			assert!(
				cache_path(root).join(collection.relative_path).is_dir(),
				"reset failure must preserve {}",
				collection.relative_path
			);
		}
	}

	#[test]
	fn reset_clears_exact_social_stores_preserves_authority_and_is_idempotent() {
		let temp = reset_root("social-content-v2-reset-");
		let root = temp.path().to_path_buf();
		install_safe_collections(&root);
		let preserved = [
			"social/x/xurl-authorization-contract.json",
			"social/x/x-pricing-receipt.json",
			"social/x/x-pricing-failure.json",
			"social/x/xurl-runtime/xurl",
			"unrelated/keep.json",
		];
		for path in preserved {
			crate::write_new_json(&cache_path(&root).join(path), &json!({"keep": true}))
				.expect("preserved authority fixture");
		}

		let report =
			crate::reset_social_content_v2(&SocialContentV2ResetRequest { root: root.clone() })
				.expect("Publisher clean-start reset should succeed");
		assert_eq!(report.schema, "decodex_social_content_v2_reset/v1");
		assert_eq!(report.status, "reset");
		assert_eq!(report.collections_cleared, RESET_COLLECTIONS.len());
		assert_eq!(report.files_removed, 2);
		for collection in RESET_COLLECTIONS {
			assert!(!cache_path(&root).join(collection.relative_path).exists());
		}
		for path in preserved {
			assert!(cache_path(&root).join(path).exists(), "reset must preserve {path}");
		}
		assert!(marker_path(&root).is_file());

		let second = crate::reset_social_content_v2(&SocialContentV2ResetRequest { root })
			.expect("second Publisher reset should be a no-op");
		assert_eq!(second.status, "already_active");
		assert_eq!(second.collections_cleared, 0);
		assert_eq!(second.files_removed, 0);
		assert_eq!(second.directories_removed, 0);
		assert_eq!(second.bytes_removed, 0);
	}

	#[test]
	fn reset_rejects_external_effect_and_budget_authority_before_any_deletion() {
		let cases = [
			(
				"uncertain-attempt",
				4,
				json!({
					"schema": "xurl_publication_attempt/v1",
					"status": "uncertain",
					"calls": [{"status": "inflight"}]
				}),
			),
			(
				"active-reservation",
				3,
				json!({
					"schema": "social_publish_reservation/v1",
					"status": "active",
					"reserved_cost_ceiling_microusd": 55
				}),
			),
			(
				"current-month-spend",
				4,
				json!({
					"schema": "xurl_publication_attempt/v1",
					"status": "failed",
					"billing_month": "2026-08",
					"recorded_cost_ceiling_microusd": 75
				}),
			),
			(
				"published-post",
				1,
				json!({
					"schema": "social_post/v1",
					"status": "published",
					"decision": {
						"worthiness": "publish",
						"daily_count_before": 0,
						"daily_count_after": 1
					},
					"publication": {"post_id": "123"}
				}),
			),
			(
				"embedded-uncertain-candidate",
				0,
				json!({
					"schema": "social_candidate/v1",
					"decision": {
						"worthiness": "skip",
						"idempotency_key": "quality-skip",
						"reason": "not publish-worthy"
					},
					"effect": {"phase": "uncertain"}
				}),
			),
			(
				"embedded-published-post",
				1,
				json!({
					"schema": "social_post/v1",
					"status": "skipped",
					"decision": {
						"worthiness": "skip",
						"idempotency_key": "quality-skip",
						"reason": "not publish-worthy",
						"daily_count_before": 0,
						"daily_count_after": 0
					},
					"skip": {"reason": "not publish-worthy"},
					"published": true
				}),
			),
		];
		for (name, collection_index, payload) in cases {
			let temp = reset_root("social-content-v2-authority-");
			let root = temp.path().to_path_buf();
			install_safe_collections(&root);
			let unsafe_path = collection_path(&root, collection_index).join(format!("{name}.json"));
			crate::write_new_json(&unsafe_path, &payload).expect("effect authority fixture");

			let error =
				crate::reset_social_content_v2(&SocialContentV2ResetRequest { root: root.clone() })
					.expect_err("effect or budget authority must block reset");
			let message = error.to_string();
			assert!(
				message.contains("quality skip")
					|| message.contains("quality-skip")
					|| message.contains("external-effect")
			);
			assert_all_collections_present(&root);
			assert!(unsafe_path.exists());
			assert!(!marker_path(&root).exists());
		}
	}

	#[test]
	fn reset_preflight_rejects_symlinks_and_hardlinks_without_deleting_other_stores() {
		for unsafe_kind in ["symlink", "hardlink"] {
			let temp = reset_root("social-content-v2-unsafe-");
			let root = temp.path().to_path_buf();
			install_safe_collections(&root);
			let preserved = collection_path(&root, 0).join("candidate.json");
			let unsafe_path = collection_path(&root, 1).join("unsafe.json");
			let target = cache_path(&root).join("unrelated.json");

			crate::write_new_json(&target, &json!({"target": true})).expect("target fixture");
			match unsafe_kind {
				"symlink" => symlink(&target, &unsafe_path).expect("symlink fixture"),
				"hardlink" => fs::hard_link(&target, &unsafe_path).expect("hardlink fixture"),
				_ => unreachable!(),
			}

			let error =
				crate::reset_social_content_v2(&SocialContentV2ResetRequest { root: root.clone() })
					.expect_err("unsafe reset entry must fail closed");
			assert!(
				error.to_string().contains("symlink") || error.to_string().contains("one-link")
			);
			assert!(preserved.exists());
			assert!(!marker_path(&root).exists());
		}
	}

	#[test]
	fn reset_detects_common_root_locks_root_and_absent_collection_replacement() {
		for race in ["cache-root", "locks-root", "absent-collection"] {
			let temp = reset_root("social-content-v2-race-");
			let root = temp.path().to_path_buf();
			crate::write_new_json(
				&collection_path(&root, 0).join("candidate.json"),
				&quality_skip_candidate(),
			)
			.expect("candidate fixture");
			let cache = cache_path(&root);
			let displaced = cache.with_file_name(format!("displaced-{race}"));
			let error = super::reset_social_content_v2_with(
				&SocialContentV2ResetRequest { root: root.clone() },
				ResetLimits::PRODUCTION,
				|| match race {
					"cache-root" => {
						fs::rename(&cache, &displaced).expect("cache root displacement");
						crate::write_new_json(
							&cache.join("replacement.json"),
							&json!({"new": true}),
						)
						.expect("cache root replacement");
					},
					"locks-root" => {
						let locks = cache.join(LOCKS_RELATIVE_PATH);
						let moved = cache.join("social/x/displaced-locks");
						fs::rename(&locks, &moved).expect("locks root displacement");
						crate::write_new_json(
							&locks.join("replacement.json"),
							&json!({"new": true}),
						)
						.expect("locks root replacement");
					},
					"absent-collection" => {
						crate::write_new_json(
							&collection_path(&root, 1).join("late.json"),
							&quality_skip_post(),
						)
						.expect("late collection replacement");
					},
					_ => unreachable!(),
				},
			)
			.expect_err("reset authority replacement must fail closed");
			assert!(!error.to_string().is_empty());
			assert!(!marker_path(&root).exists());
			match race {
				"cache-root" => {
					assert!(displaced.join("social/x/candidates/candidate.json").exists());
					assert!(cache.join("replacement.json").exists());
				},
				"locks-root" | "absent-collection" => {
					assert!(collection_path(&root, 0).join("candidate.json").exists());
				},
				_ => unreachable!(),
			}
		}
	}

	#[test]
	fn marker_readback_preserves_post_activation_v2_state_byte_for_byte() {
		let temp = reset_root("social-content-v2-current-state-");
		let root = temp.path().to_path_buf();
		let first =
			crate::reset_social_content_v2(&SocialContentV2ResetRequest { root: root.clone() })
				.expect("initial activation");
		assert_eq!(first.status, "reset");

		let records = [
			(0, "candidate.json", crate::tests::valid_social_candidate()),
			(1, "post.json", crate::tests::valid_social_post()),
			(2, "outcome.json", crate::tests::valid_social_outcome()),
			(3, "2026-07-27/reservation.json", crate::tests::valid_social_publish_reservation()),
			(
				4,
				"2026-07/attempt.json",
				json!({
					"schema": "decodex/xurl-publish-attempt/4",
					"run_id": "019fa400-0000-7000-8000-000000000001",
					"status": "published",
					"post_id": "2000000000000000001",
					"reserved_cost_ceiling_microusd": 30000,
					"calls": [{"operation": "content_create", "status": "succeeded"}]
				}),
			),
			(5, "strategy.json", crate::tests::valid_social_strategy("2026-07-27")),
			(6, "staging.json", crate::tests::valid_social_candidate()),
		];
		let mut snapshots = Vec::with_capacity(records.len());
		for (collection, relative, payload) in records {
			let path = collection_path(&root, collection).join(relative);
			crate::write_new_json(&path, &payload).expect("post-activation Publisher state");
			snapshots.push((path.clone(), fs::read(path).expect("post-activation bytes")));
		}

		let readback =
			crate::reset_social_content_v2(&SocialContentV2ResetRequest { root: root.clone() })
				.expect("marker readback should succeed");
		assert_eq!(readback.status, "already_active");
		assert_eq!(readback.collections_cleared, 0);
		assert_eq!(readback.files_removed, 0);
		assert_eq!(readback.directories_removed, 0);
		assert_eq!(readback.bytes_removed, 0);
		for (path, bytes) in snapshots {
			assert_eq!(
				fs::read(&path).expect("preserved Publisher record"),
				bytes,
				"{}",
				path.display()
			);
		}
		assert!(marker_path(&root).is_file());
	}

	#[test]
	fn reset_inventory_rejects_max_plus_one_entries_without_deletion() {
		let temp = reset_root("social-content-v2-entry-bound-");
		let root = temp.path().to_path_buf();
		for index in 0..3 {
			crate::write_new_json(
				&collection_path(&root, 0).join(format!("candidate-{index}.json")),
				&quality_skip_candidate(),
			)
			.expect("bounded candidate fixture");
		}
		let error = super::reset_social_content_v2_with(
			&SocialContentV2ResetRequest { root: root.clone() },
			ResetLimits { entries: 3, files: 8, bytes: 4096 },
			|| {},
		)
		.expect_err("root plus max entries must reject max-plus-one");
		assert!(error.to_string().contains("bounded") || error.to_string().contains("bound"));
		assert_eq!(
			fs::read_dir(collection_path(&root, 0)).expect("candidate directory").count(),
			3
		);
		assert!(!marker_path(&root).exists());
	}
}
