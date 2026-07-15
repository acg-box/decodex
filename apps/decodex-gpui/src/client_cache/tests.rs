//! Independent compilation and filesystem acceptance tests for the private cache.

use std::{
	fs::{self, OpenOptions},
	io::{ErrorKind, Seek as _, SeekFrom, Write as _},
	path::{Path, PathBuf},
};

use tempfile::TempDir;

use crate::client_cache::{
	CacheAuthority, CacheError, CacheLimits, ClientCache, EntityId, EntityRevision, FaultInjector,
	FaultPoint, ObjectCertainty, ObjectInput, ProtocolVersion, ServerId, UncertainResolution,
};

const PROTOCOL: ProtocolVersion = ProtocolVersion { major: 1, minor: 2 };

fn authority(server: &str, schema: u64) -> CacheAuthority {
	let server = ServerId::new(server).expect("test server identity is bounded");

	CacheAuthority::new(&server, PROTOCOL, schema).expect("test authority is valid")
}

fn limits(generations: usize) -> CacheLimits {
	CacheLimits::new(2 * 1_024 * 1_024, 32, generations).expect("test limits are valid")
}

fn root(temporary: &TempDir) -> PathBuf {
	temporary.path().canonicalize().expect("temporary root canonicalizes").join("cache")
}

fn entity(value: &str) -> EntityId {
	EntityId::new(value).expect("test entity identity is bounded")
}

fn generation_paths(root: &Path) -> Vec<PathBuf> {
	let mut paths = fs::read_dir(root.join("generations"))
		.expect("generation directory exists")
		.map(|entry| entry.expect("generation entry is readable").path())
		.collect::<Vec<_>>();

	paths.sort();

	paths
}

fn sole_object_path(root: &Path) -> PathBuf {
	let generations = generation_paths(root);
	let mut objects = fs::read_dir(generations[0].join("objects"))
		.expect("object directory exists")
		.map(|entry| entry.expect("object entry is readable").path())
		.collect::<Vec<_>>();

	assert_eq!(objects.len(), 1);

	objects.remove(0)
}

#[test]
fn immutable_generation_is_content_attested_and_raw_identity_never_names_a_path() {
	let temporary = TempDir::new().expect("temporary directory is available");
	let root = root(&temporary);
	let cache =
		ClientCache::open(&root, limits(3), authority("server-a", 7)).expect("empty cache opens");
	let hostile = entity("../../raw/entity");
	let first = cache
		.publish(
			&[ObjectInput::new(
				&hostile,
				EntityRevision(4),
				b"attested-content",
				ObjectCertainty::Authoritative,
			)],
			&[],
		)
		.expect("generation publishes");

	assert_eq!(first.sequence, 1);
	assert_eq!(first.records, 1);
	assert_eq!(first.physical_objects, 1);
	assert_eq!(first.uncertain_records, 0);
	assert_eq!(first.payload_bytes, 16);
	assert_eq!(first.authority, authority("server-a", 7));
	assert_eq!(first.generation.len(), 64);

	let published_manifest =
		fs::read(root.join("generations").join(&first.generation).join("manifest.json"))
			.expect("manifest is readable");

	assert!(!root.join("generations").join("..").join("raw").exists());

	let second_entity = entity("entity-2");

	cache
		.publish(
			&[ObjectInput::new(
				&second_entity,
				EntityRevision(1),
				b"next",
				ObjectCertainty::Authoritative,
			)],
			&[],
		)
		.expect("next generation publishes");

	assert_eq!(
		fs::read(root.join("generations").join(first.generation).join("manifest.json"))
			.expect("old manifest remains readable"),
		published_manifest
	);
}

#[test]
fn physical_object_bounds_count_shared_content_once() {
	let temporary = TempDir::new().expect("temporary directory is available");
	let root = root(&temporary);
	let one_physical_object =
		CacheLimits::new(16 * 1_024, 1, 2).expect("single-object limits are valid");
	let cache = ClientCache::open(&root, one_physical_object, authority("server-a", 1))
		.expect("empty cache opens");
	let first = entity("first");
	let second = entity("second");
	let published = cache
		.publish(
			&[
				ObjectInput::new(
					&first,
					EntityRevision(1),
					b"shared",
					ObjectCertainty::Authoritative,
				),
				ObjectInput::new(
					&second,
					EntityRevision(1),
					b"shared",
					ObjectCertainty::Authoritative,
				),
			],
			&[],
		)
		.expect("shared content publishes once");

	assert_eq!(published.records, 2);
	assert_eq!(published.physical_objects, 1);
	assert_eq!(
		ClientCache::open(&root, one_physical_object, authority("server-a", 1))
			.expect("deduplicated cache reopens")
			.inspect_current()
			.expect("current generation inspects"),
		Some(published)
	);
}

#[test]
fn uncertainty_is_carried_until_an_explicit_authoritative_resolution() {
	let temporary = TempDir::new().expect("temporary directory is available");
	let root = root(&temporary);
	let cache =
		ClientCache::open(&root, limits(4), authority("server-a", 1)).expect("empty cache opens");
	let uncertain = entity("uncertain");
	let certain = entity("certain");

	cache
		.publish(
			&[ObjectInput::new(
				&uncertain,
				EntityRevision(9),
				b"preserve-me",
				ObjectCertainty::Uncertain,
			)],
			&[],
		)
		.expect("uncertain generation publishes");

	let carried = cache
		.publish(
			&[ObjectInput::new(
				&certain,
				EntityRevision(1),
				b"certain",
				ObjectCertainty::Authoritative,
			)],
			&[],
		)
		.expect("uncertain object is inherited");

	assert_eq!(carried.records, 2);
	assert_eq!(carried.uncertain_records, 1);
	assert_eq!(
		cache.publish(
			&[ObjectInput::new(
				&uncertain,
				EntityRevision(9),
				b"replacement",
				ObjectCertainty::Authoritative,
			)],
			&[UncertainResolution::new(&uncertain, EntityRevision(9))],
		),
		Err(CacheError::ConflictingObject)
	);

	let resolved = cache
		.publish(&[], &[UncertainResolution::new(&uncertain, EntityRevision(9))])
		.expect("explicit resolution publishes");

	assert_eq!(resolved.records, 0);
	assert_eq!(resolved.uncertain_records, 0);
	assert_eq!(
		cache.publish(&[], &[UncertainResolution::new(&uncertain, EntityRevision(9))]),
		Err(CacheError::UnknownResolution)
	);
}

#[test]
fn server_protocol_and_schema_switches_are_explicit_and_never_inherit_objects() {
	let temporary = TempDir::new().expect("temporary directory is available");
	let root = root(&temporary);
	let old =
		ClientCache::open(&root, limits(4), authority("server-a", 1)).expect("empty cache opens");
	let uncertain = entity("uncertain");

	old.publish(
		&[ObjectInput::new(
			&uncertain,
			EntityRevision(1),
			b"old-authority",
			ObjectCertainty::Uncertain,
		)],
		&[],
	)
	.expect("old generation publishes");

	assert!(matches!(
		ClientCache::open(&root, limits(4), authority("server-b", 1)),
		Err(CacheError::AuthorityMismatch)
	));
	assert!(matches!(
		ClientCache::open(&root, limits(4), authority("server-a", 2)),
		Err(CacheError::AuthorityMismatch)
	));

	let protocol_switch = CacheAuthority::new(
		&ServerId::new("server-a").expect("identity is valid"),
		ProtocolVersion { major: 2, minor: 0 },
		1,
	)
	.expect("protocol authority is structurally valid");

	assert!(matches!(
		ClientCache::open(&root, limits(4), protocol_switch),
		Err(CacheError::AuthorityMismatch)
	));

	let switched =
		ClientCache::prepare_authority_switch(&root, limits(4), authority("server-b", 1))
			.expect("valid old cache can prepare an explicit switch");

	assert_eq!(switched.inspect_current(), Err(CacheError::AuthorityMismatch));

	let current = switched.publish(&[], &[]).expect("new authority publishes independently");

	assert_eq!(current.authority, authority("server-b", 1));
	assert_eq!(current.records, 0);
	assert_eq!(current.uncertain_records, 0);
}

struct OrderedFaults {
	crash_at: Option<FaultPoint>,
	seen: Vec<FaultPoint>,
}
impl FaultInjector for OrderedFaults {
	fn check(&mut self, point: FaultPoint) -> Result<(), CacheError> {
		self.seen.push(point);

		if self.crash_at == Some(point) { Err(CacheError::InjectedCrash(point)) } else { Ok(()) }
	}
}

struct HostFailure(FaultPoint);
impl FaultInjector for HostFailure {
	fn check(&mut self, point: FaultPoint) -> Result<(), CacheError> {
		if self.0 == point { Err(CacheError::Io(ErrorKind::Other)) } else { Ok(()) }
	}
}

#[test]
fn publication_order_exposes_each_filesystem_durability_boundary() {
	let temporary = TempDir::new().expect("temporary directory is available");
	let root = root(&temporary);
	let cache =
		ClientCache::open(&root, limits(3), authority("server-a", 1)).expect("empty cache opens");
	let object = entity("entity");
	let mut faults = OrderedFaults { crash_at: None, seen: Vec::new() };

	cache
		.publish_with_faults(
			&[ObjectInput::new(
				&object,
				EntityRevision(1),
				b"bytes",
				ObjectCertainty::Authoritative,
			)],
			&[],
			&mut faults,
		)
		.expect("publication completes");

	assert_eq!(
		faults.seen,
		vec![
			FaultPoint::WriterLocked,
			FaultPoint::StagingCreated,
			FaultPoint::ObjectsDurable,
			FaultPoint::ManifestDurable,
			FaultPoint::GenerationPublished,
			FaultPoint::CurrentPointerDurable,
			FaultPoint::CurrentPointerRenamed,
			FaultPoint::CurrentPointerPublished,
			FaultPoint::WriterMarkerRemove,
			FaultPoint::WriterDirectorySync,
		]
	);
}

#[test]
fn every_injected_crash_leaves_a_fail_closed_remnant_and_full_disposal_recovers() {
	let points = [
		FaultPoint::WriterLocked,
		FaultPoint::StagingCreated,
		FaultPoint::ObjectsDurable,
		FaultPoint::ManifestDurable,
		FaultPoint::GenerationPublished,
		FaultPoint::CurrentPointerDurable,
		FaultPoint::CurrentPointerRenamed,
		FaultPoint::CurrentPointerPublished,
		FaultPoint::WriterMarkerRemove,
		FaultPoint::WriterDirectorySync,
	];

	for point in points {
		let temporary = TempDir::new().expect("temporary directory is available");
		let root = root(&temporary);
		let cache = ClientCache::open(&root, limits(3), authority("server-a", 1))
			.expect("empty cache opens");
		let object = entity("entity");
		let mut faults = OrderedFaults { crash_at: Some(point), seen: Vec::new() };

		assert_eq!(
			cache.publish_with_faults(
				&[ObjectInput::new(
					&object,
					EntityRevision(1),
					b"bytes",
					ObjectCertainty::Authoritative,
				)],
				&[],
				&mut faults,
			),
			Err(CacheError::InjectedCrash(point))
		);
		assert!(matches!(
			ClientCache::open(&root, limits(3), authority("server-a", 1)),
			Err(CacheError::CrashRemnant)
		));

		ClientCache::dispose_all(&root).expect("complete disposal removes crash remnants");

		let rebuilt = ClientCache::open(&root, limits(3), authority("server-a", 1))
			.expect("empty cache rebuilds");

		assert_eq!(rebuilt.inspect_current().expect("inspection succeeds"), None);
	}
}

#[test]
fn host_failure_after_publication_begins_retains_a_fail_closed_writer_marker() {
	let temporary = TempDir::new().expect("temporary directory is available");
	let root = root(&temporary);
	let cache =
		ClientCache::open(&root, limits(3), authority("server-a", 1)).expect("empty cache opens");
	let mut failure = HostFailure(FaultPoint::CurrentPointerPublished);

	assert_eq!(
		cache.publish_with_faults(&[], &[], &mut failure),
		Err(CacheError::Io(ErrorKind::Other))
	);
	assert!(matches!(
		ClientCache::open(&root, limits(3), authority("server-a", 1)),
		Err(CacheError::CrashRemnant)
	));
}

#[test]
fn writer_release_failures_never_report_success_for_publication_or_disposal() {
	for point in [FaultPoint::WriterMarkerRemove, FaultPoint::WriterDirectorySync] {
		let publication = TempDir::new().expect("temporary directory is available");
		let publication_root = root(&publication);
		let cache = ClientCache::open(&publication_root, limits(3), authority("server-a", 1))
			.expect("empty cache opens");
		let mut failure = HostFailure(point);

		assert_eq!(
			cache.publish_with_faults(&[], &[], &mut failure),
			Err(CacheError::Io(ErrorKind::Other))
		);
		assert!(matches!(
			ClientCache::open(&publication_root, limits(3), authority("server-a", 1)),
			Err(CacheError::CrashRemnant)
		));

		let disposal = TempDir::new().expect("temporary directory is available");
		let disposal_root = root(&disposal);
		let cache = ClientCache::open(&disposal_root, limits(3), authority("server-a", 1))
			.expect("empty cache opens");
		let old = cache.publish(&[], &[]).expect("old generation publishes");

		cache.publish(&[], &[]).expect("current generation publishes");

		let mut failure = HostFailure(point);

		assert_eq!(
			cache.dispose_generation_with_faults(&old.generation, &mut failure),
			Err(CacheError::Io(ErrorKind::Other))
		);
		assert!(matches!(
			ClientCache::open(&disposal_root, limits(3), authority("server-a", 1)),
			Err(CacheError::CrashRemnant)
		));
	}
}

#[test]
fn corruption_truncation_hash_mismatch_and_unknown_versions_fail_closed() {
	let corrupt = TempDir::new().expect("temporary directory is available");
	let corrupt_root = root(&corrupt);
	let cache = ClientCache::open(&corrupt_root, limits(3), authority("server-a", 1))
		.expect("empty cache opens");
	let object = entity("entity");

	cache
		.publish(
			&[ObjectInput::new(
				&object,
				EntityRevision(1),
				b"original",
				ObjectCertainty::Authoritative,
			)],
			&[],
		)
		.expect("generation publishes");

	fs::write(sole_object_path(&corrupt_root), b"tampered").expect("test corrupts cached object");

	assert!(matches!(
		ClientCache::open(&corrupt_root, limits(3), authority("server-a", 1)),
		Err(CacheError::IntegrityMismatch)
	));

	let truncated = TempDir::new().expect("temporary directory is available");
	let truncated_root = root(&truncated);
	let cache = ClientCache::open(&truncated_root, limits(3), authority("server-a", 1))
		.expect("empty cache opens");

	cache.publish(&[], &[]).expect("empty generation publishes");

	fs::write(truncated_root.join("current"), b"{").expect("test truncates current pointer");

	assert!(matches!(
		ClientCache::open(&truncated_root, limits(3), authority("server-a", 1)),
		Err(CacheError::InvalidMetadata)
	));

	let unknown = TempDir::new().expect("temporary directory is available");
	let unknown_root = root(&unknown);
	let cache = ClientCache::open(&unknown_root, limits(3), authority("server-a", 1))
		.expect("empty cache opens");
	let generation = cache.publish(&[], &[]).expect("empty generation publishes").generation;

	fs::write(
		unknown_root.join("current"),
		format!(r#"{{"version":2,"generation":"{generation}"}}"#),
	)
	.expect("test writes unknown pointer version");

	assert!(matches!(
		ClientCache::open(&unknown_root, limits(3), authority("server-a", 1)),
		Err(CacheError::InvalidMetadata)
	));
}

#[test]
fn deterministic_eviction_preserves_the_current_valid_generation() {
	let temporary = TempDir::new().expect("temporary directory is available");
	let root = root(&temporary);
	let cache =
		ClientCache::open(&root, limits(2), authority("server-a", 1)).expect("empty cache opens");
	let object = entity("entity");
	let first = cache
		.publish(
			&[ObjectInput::new(&object, EntityRevision(1), b"one", ObjectCertainty::Authoritative)],
			&[],
		)
		.expect("first generation publishes");
	let second = cache
		.publish(
			&[ObjectInput::new(&object, EntityRevision(2), b"two", ObjectCertainty::Authoritative)],
			&[],
		)
		.expect("second generation publishes");
	let third = cache
		.publish(
			&[ObjectInput::new(
				&object,
				EntityRevision(3),
				b"three",
				ObjectCertainty::Authoritative,
			)],
			&[],
		)
		.expect("oldest non-current generation is evicted");

	assert!(!root.join("generations").join(first.generation).exists());
	assert!(root.join("generations").join(second.generation).exists());
	assert!(root.join("generations").join(&third.generation).exists());
	assert_eq!(cache.inspect_current().expect("current inspection succeeds"), Some(third));
}

#[test]
fn bounds_refusal_does_not_damage_the_current_generation() {
	let temporary = TempDir::new().expect("temporary directory is available");
	let root = root(&temporary);
	let limits = CacheLimits::new(1_024, 2, 2).expect("small test limits are valid");
	let cache =
		ClientCache::open(&root, limits, authority("server-a", 1)).expect("empty cache opens");
	let object = entity("entity");
	let first = cache.publish(&[], &[]).expect("empty current generation publishes");
	let oversized = vec![0_u8; 2_048];

	assert_eq!(
		cache.publish(
			&[ObjectInput::new(
				&object,
				EntityRevision(1),
				&oversized,
				ObjectCertainty::Authoritative,
			)],
			&[],
		),
		Err(CacheError::BoundsExceeded)
	);
	assert_eq!(cache.inspect_current().expect("current remains valid"), Some(first));
}

#[test]
fn open_rejects_over_cap_sparse_content_before_hash_mismatch() {
	let temporary = TempDir::new().expect("temporary directory is available");
	let root = root(&temporary);
	let cache =
		ClientCache::open(&root, limits(3), authority("server-a", 1)).expect("empty cache opens");
	let object = entity("entity");

	cache
		.publish(
			&[ObjectInput::new(
				&object,
				EntityRevision(1),
				b"small",
				ObjectCertainty::Authoritative,
			)],
			&[],
		)
		.expect("generation publishes");

	OpenOptions::new()
		.write(true)
		.open(sole_object_path(&root))
		.expect("object fixture opens")
		.set_len(1_025)
		.expect("sparse over-cap fixture is created");

	let small_limits = CacheLimits::new(1_024, 32, 3).expect("small limits are valid");

	assert!(matches!(
		ClientCache::open(&root, small_limits, authority("server-a", 1)),
		Err(CacheError::BoundsExceeded)
	));
}

#[test]
fn open_rejects_over_cap_cumulative_content_before_hash_mismatch() {
	let temporary = TempDir::new().expect("temporary directory is available");
	let root = root(&temporary);
	let cache =
		ClientCache::open(&root, limits(3), authority("server-a", 1)).expect("empty cache opens");
	let first = entity("first");
	let second = entity("second");
	let first_bytes = vec![b'a'; 700];
	let second_bytes = vec![b'b'; 700];

	let generation = cache
		.publish(
			&[
				ObjectInput::new(
					&first,
					EntityRevision(1),
					&first_bytes,
					ObjectCertainty::Authoritative,
				),
				ObjectInput::new(
					&second,
					EntityRevision(1),
					&second_bytes,
					ObjectCertainty::Authoritative,
				),
			],
			&[],
		)
		.expect("generation publishes")
		.generation;

	for entry in fs::read_dir(root.join("generations").join(generation).join("objects"))
		.expect("object directory is readable")
	{
		let mut object = OpenOptions::new()
			.write(true)
			.open(entry.expect("object entry is readable").path())
			.expect("object fixture opens");

		object.seek(SeekFrom::Start(0)).expect("object fixture seeks");
		object.write_all(b"x").expect("object digest is corrupted");
	}

	let small_limits = CacheLimits::new(1_000, 32, 3).expect("small limits are valid");

	assert!(matches!(
		ClientCache::open(&root, small_limits, authority("server-a", 1)),
		Err(CacheError::BoundsExceeded)
	));
}

#[test]
fn offline_generation_disposal_rejects_current_and_removes_only_the_target() {
	let temporary = TempDir::new().expect("temporary directory is available");
	let root = root(&temporary);
	let cache =
		ClientCache::open(&root, limits(3), authority("server-a", 1)).expect("empty cache opens");
	let first = cache.publish(&[], &[]).expect("first generation publishes");
	let second = cache.publish(&[], &[]).expect("second generation publishes");

	assert_eq!(
		cache
			.inspect_generation(&first.generation)
			.expect("non-current generation inspects offline"),
		first
	);
	assert_eq!(cache.dispose_generation(&second.generation), Err(CacheError::CurrentGeneration));

	cache.dispose_generation(&first.generation).expect("non-current generation disposes offline");

	assert!(!root.join("generations").join(first.generation).exists());
	assert!(root.join("generations").join(second.generation).exists());
	assert_eq!(cache.dispose_generation(&"0".repeat(64)), Err(CacheError::GenerationNotFound));
}

#[cfg(unix)]
#[test]
fn symlinks_and_foreign_shapes_fail_closed_while_full_disposal_never_follows_them() {
	let temporary = TempDir::new().expect("temporary directory is available");
	let root = root(&temporary);
	let cache =
		ClientCache::open(&root, limits(3), authority("server-a", 1)).expect("empty cache opens");

	drop(cache);

	let outside = temporary.path().join("outside");

	fs::write(&outside, b"must-survive").expect("outside fixture is writable");

	std::os::unix::fs::symlink(&outside, root.join("foreign-link"))
		.expect("test symlink is created");

	assert!(matches!(
		ClientCache::open(&root, limits(3), authority("server-a", 1)),
		Err(CacheError::ForeignArtifact)
	));

	ClientCache::dispose_all(&root).expect("complete disposal unlinks without following");

	assert_eq!(fs::read(outside).expect("outside fixture survives"), b"must-survive");

	let linked_root = temporary.path().join("linked-root");

	std::os::unix::fs::symlink(temporary.path(), &linked_root).expect("root symlink is created");

	assert!(matches!(
		ClientCache::open(&linked_root, limits(3), authority("server-a", 1)),
		Err(CacheError::UnsafeRoot)
	));
	assert_eq!(ClientCache::dispose_all(&linked_root), Err(CacheError::UnsafeRoot));
}

#[test]
fn foreign_object_and_manifest_shapes_fail_closed() {
	let temporary = TempDir::new().expect("temporary directory is available");
	let root = root(&temporary);
	let cache =
		ClientCache::open(&root, limits(3), authority("server-a", 1)).expect("empty cache opens");
	let generation = cache.publish(&[], &[]).expect("generation publishes").generation;
	let foreign = root.join("generations").join(generation).join("foreign");

	OpenOptions::new()
		.write(true)
		.create_new(true)
		.open(foreign)
		.expect("foreign fixture is created")
		.write_all(b"foreign")
		.expect("foreign fixture is written");

	assert!(matches!(
		ClientCache::open(&root, limits(3), authority("server-a", 1)),
		Err(CacheError::ForeignArtifact)
	));
}

#[test]
fn invalid_configuration_and_conflicting_content_fail_without_publication() {
	assert_eq!(CacheLimits::new(0, 1, 1), Err(CacheError::InvalidLimits));

	let server = ServerId::new("server-a").expect("identity is valid");

	assert_eq!(CacheAuthority::new(&server, PROTOCOL, 0), Err(CacheError::InvalidAuthority));

	let temporary = TempDir::new().expect("temporary directory is available");
	let root = root(&temporary);
	let cache =
		ClientCache::open(&root, limits(3), authority("server-a", 1)).expect("empty cache opens");
	let object = entity("entity");

	assert_eq!(
		cache.publish(
			&[
				ObjectInput::new(
					&object,
					EntityRevision(1),
					b"one",
					ObjectCertainty::Authoritative,
				),
				ObjectInput::new(
					&object,
					EntityRevision(1),
					b"two",
					ObjectCertainty::Authoritative,
				),
			],
			&[],
		),
		Err(CacheError::ConflictingObject)
	);
	assert_eq!(cache.inspect_current().expect("cache remains empty"), None);
	assert_eq!(
		cache.publish(
			&[
				ObjectInput::new(
					&object,
					EntityRevision(1),
					b"same",
					ObjectCertainty::Authoritative,
				),
				ObjectInput::new(
					&object,
					EntityRevision(1),
					b"same",
					ObjectCertainty::Authoritative,
				),
			],
			&[],
		),
		Err(CacheError::ConflictingObject)
	);
}
