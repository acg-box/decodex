//! XY-1306 atomic blob-integrity and disposable cache-bound coverage.

#[path = "support/test_root.rs"] mod support;

use std::fs;

use getrandom as _;
#[cfg(unix)] use libc as _;
use serde as _;
use sha2 as _;
use tempfile::NamedTempFile;
use toml as _;

use decodex_core::{
	BlobHash, BlobStore, BoundedCache, CacheLimits, DecodexRoot, MAX_BLOB_BYTES, MAX_CACHE_BYTES,
	MAX_CACHE_ENTRIES, MAX_CACHE_ENTRY_BYTES, PathError, StorageError,
};
use support::TestRoot;

#[test]
fn blob_writes_are_content_addressed_atomic_contained_and_verified() {
	let fixture = TestRoot::new();
	let store = BlobStore::open(fixture.paths.clone()).expect("blob store");
	let hash = store.put(b"abc").expect("blob write");

	assert_eq!(hash.to_hex(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",);
	assert!(store.path_for(hash).starts_with(fixture.paths.root().as_path()));
	assert_eq!(store.read(hash).expect("verified read"), b"abc");

	let blob_path = store.path_for(hash);
	let shard = blob_path.parent().expect("blob shard");
	let names = support::private_file_names(shard);

	assert_eq!(names, vec![store.path_for(hash)]);
	assert!(!names.iter().any(|path| {
		path.file_name()
			.and_then(|name| name.to_str())
			.is_some_and(|name| name.starts_with(".tmp-"))
	}));
}

#[test]
fn tampered_blob_bytes_fail_integrity_verification() {
	let fixture = TestRoot::new();
	let store = BlobStore::open(fixture.paths.clone()).expect("blob store");
	let hash = store.put(b"authoritative evidence").expect("blob write");

	support::write_private(&store.path_for(hash), b"tampered evidence!!!");

	assert_eq!(store.read(hash).unwrap_err(), StorageError::BlobIntegrityMismatch);
	assert_eq!(
		store.put(b"authoritative evidence").unwrap_err(),
		StorageError::BlobIntegrityMismatch,
	);
}

#[test]
fn blob_inputs_and_hash_text_are_mechanically_bounded() {
	assert_eq!(BlobHash::parse("../../escape").unwrap_err(), StorageError::InvalidBlobHash);
	assert_eq!(BlobHash::parse(&"A".repeat(64)).unwrap_err(), StorageError::InvalidBlobHash);

	let fixture = TestRoot::new();
	let store = BlobStore::open(fixture.paths.clone()).expect("blob store");
	let oversized = vec![0_u8; MAX_BLOB_BYTES + 1];

	assert_eq!(
		store.put(&oversized).unwrap_err(),
		StorageError::BlobTooLarge { limit: MAX_BLOB_BYTES },
	);
}

#[cfg(unix)]
#[test]
fn blob_symlinks_and_unexpected_file_kinds_fail_closed() {
	let fixture = TestRoot::new();
	let store = BlobStore::open(fixture.paths.clone()).expect("blob store");
	let hash = BlobHash::digest(b"symlink target");
	let path = store.path_for(hash);
	let shard = path.parent().expect("blob shard");

	fs::create_dir(shard).expect("blob shard fixture");
	support::set_mode(shard, 0o700);

	let outside = NamedTempFile::new().expect("outside blob");

	std::os::unix::fs::symlink(outside.path(), &path).expect("blob symlink fixture");

	assert!(matches!(store.read(hash), Err(StorageError::Path(PathError::Symlink))));
	assert!(matches!(store.put(b"symlink target"), Err(StorageError::Path(PathError::Symlink))));

	fs::remove_file(&path).expect("remove symlink");
	fs::create_dir(&path).expect("directory in blob position");

	assert!(matches!(store.read(hash), Err(StorageError::Path(PathError::UnexpectedFileKind)),));
}

#[test]
fn cache_limits_have_non_bypassable_hard_ceilings() {
	assert_eq!(CacheLimits::new(0, 1, 1), Err(StorageError::InvalidCacheLimits));
	assert_eq!(
		CacheLimits::new(MAX_CACHE_ENTRIES + 1, 1, 1),
		Err(StorageError::InvalidCacheLimits),
	);
	assert_eq!(CacheLimits::new(1, MAX_CACHE_BYTES + 1, 1), Err(StorageError::InvalidCacheLimits),);
	assert_eq!(
		CacheLimits::new(1, MAX_CACHE_ENTRY_BYTES, MAX_CACHE_ENTRY_BYTES + 1),
		Err(StorageError::InvalidCacheLimits),
	);
	assert_eq!(CacheLimits::new(1, 8, 9), Err(StorageError::InvalidCacheLimits));
}

#[test]
fn cache_is_bounded_replaceable_disposable_and_non_authoritative() {
	let fixture = TestRoot::new();
	let blob_store = BlobStore::open(fixture.paths.clone()).expect("blob store");
	let blob_hash = blob_store.put(b"authoritative blob").expect("blob write");
	let limits = CacheLimits::new(2, 8, 6).expect("cache limits");
	let cache = BoundedCache::open(fixture.paths.clone(), limits).expect("cache");

	cache.put("one", b"1111").expect("cache one");
	cache.put("two", b"2222").expect("cache two");

	let usage = cache.put("three", b"3333").expect("cache eviction");

	assert_eq!(usage.entries, 2);
	assert_eq!(usage.bytes, 8);

	let hits = ["one", "two", "three"]
		.into_iter()
		.filter(|key| cache.get(key).expect("cache read").is_some())
		.count();

	assert_eq!(hits, 2);

	let usage = cache.put("three", b"33").expect("cache replacement");

	assert!(usage.entries <= 2);
	assert!(usage.bytes <= 8);
	assert_eq!(cache.get("three").expect("replacement read"), Some(b"33".to_vec()));

	cache.clear().expect("disposable clear");

	assert_eq!(cache.usage().expect("empty usage").entries, 0);
	assert_eq!(
		blob_store.read(blob_hash).expect("blob survives cache clear"),
		b"authoritative blob"
	);
}

#[test]
fn oversized_cache_entry_is_rejected_before_any_write() {
	let fixture = TestRoot::new();
	let limits = CacheLimits::new(2, 8, 4).expect("cache limits");
	let cache = BoundedCache::open(fixture.paths.clone(), limits).expect("cache");

	assert_eq!(
		cache.put("oversized", b"12345").unwrap_err(),
		StorageError::CacheEntryTooLarge { limit: 4 },
	);
	assert_eq!(cache.usage().expect("empty cache").entries, 0);
}

#[test]
fn preexisting_oversized_disposable_cache_file_is_evicted_on_open() {
	let fixture = TestRoot::new();

	fixture.paths.ensure_layout().expect("private layout");

	let name = format!("{}.cache", BlobHash::digest(b"oversized").to_hex());

	support::write_private(&fixture.paths.cache_dir().join(name), b"12345");

	let limits = CacheLimits::new(2, 8, 4).expect("cache limits");
	let cache = BoundedCache::open(fixture.paths.clone(), limits).expect("bounded cache");

	assert_eq!(cache.usage().expect("usage").entries, 0);
}

#[test]
fn interrupted_atomic_cache_temporary_files_are_discarded_and_clearable() {
	let fixture = TestRoot::new();
	let limits = CacheLimits::new(2, 8, 4).expect("cache limits");
	let cache = BoundedCache::open(fixture.paths.clone(), limits).expect("bounded cache");
	let temporary = fixture.paths.cache_dir().join(format!(".tmp-{}", "a".repeat(32)));

	support::write_private(&temporary, b"interrupted bytes outside configured entry limit");

	cache.clear().expect("clear interrupted temporary file");

	assert!(support::private_file_names(&fixture.paths.cache_dir()).is_empty());

	support::write_private(&temporary, b"another interrupted write");

	let reopened =
		BoundedCache::open(fixture.paths.clone(), limits).expect("recover cache on open");

	assert_eq!(reopened.usage().expect("recovered usage").entries, 0);
	assert!(support::private_file_names(&fixture.paths.cache_dir()).is_empty());
}

#[test]
fn oversized_newer_cache_entry_is_evicted_even_when_older_entry_is_small() {
	let fixture = TestRoot::new();

	fixture.paths.ensure_layout().expect("private layout");

	let small = format!("{}.cache", BlobHash::digest(b"small").to_hex());
	let large = format!("{}.cache", BlobHash::digest(b"large").to_hex());

	support::write_private(&fixture.paths.cache_dir().join(small), b"1234");
	support::write_private(&fixture.paths.cache_dir().join(large), b"12345");

	let limits = CacheLimits::new(2, 16, 4).expect("cache limits");
	let cache = BoundedCache::open(fixture.paths.clone(), limits).expect("bounded cache");

	assert_eq!(cache.usage().expect("usage").entries, 1);
	assert_eq!(cache.usage().expect("usage").bytes, 4);
}

#[test]
fn unexpected_cache_names_and_file_kinds_fail_closed() {
	let fixture = TestRoot::new();

	fixture.paths.ensure_layout().expect("private layout");

	support::write_private(&fixture.paths.cache_dir().join("not-a-content-key.cache"), b"cache");

	let limits = CacheLimits::new(2, 16, 8).expect("cache limits");

	assert_eq!(
		BoundedCache::open(fixture.paths.clone(), limits).unwrap_err(),
		StorageError::InvalidCacheEntry,
	);

	fs::remove_file(fixture.paths.cache_dir().join("not-a-content-key.cache"))
		.expect("remove invalid cache file");
	support::write_private(
		&fixture.paths.cache_dir().join(".tmp-not-an-exact-atomic-name"),
		b"cache",
	);

	assert_eq!(
		BoundedCache::open(fixture.paths.clone(), limits).unwrap_err(),
		StorageError::InvalidCacheEntry,
	);

	fs::remove_file(fixture.paths.cache_dir().join(".tmp-not-an-exact-atomic-name"))
		.expect("remove invalid temporary name");
	fs::create_dir(fixture.paths.cache_dir().join("directory.cache"))
		.expect("directory cache fixture");

	assert!(matches!(
		BoundedCache::open(fixture.paths.clone(), limits),
		Err(StorageError::Path(PathError::UnexpectedFileKind)),
	));
}

#[cfg(unix)]
#[test]
fn cache_symlinks_and_insecure_permissions_fail_closed() {
	let fixture = TestRoot::new();

	fixture.paths.ensure_layout().expect("private layout");

	let limits = CacheLimits::new(2, 16, 8).expect("cache limits");
	let valid_name = format!("{}.cache", BlobHash::digest(b"entry").to_hex());
	let path = fixture.paths.cache_dir().join(valid_name);
	let outside = NamedTempFile::new().expect("outside cache");

	std::os::unix::fs::symlink(outside.path(), &path).expect("cache symlink fixture");

	assert!(matches!(
		BoundedCache::open(fixture.paths.clone(), limits),
		Err(StorageError::Path(PathError::Symlink)),
	));

	fs::remove_file(&path).expect("remove cache symlink");
	support::write_private(&path, b"cache");
	support::set_mode(&path, 0o644);

	assert!(matches!(
		BoundedCache::open(fixture.paths.clone(), limits),
		Err(StorageError::Path(PathError::InsecurePermissions)),
	));
}

#[cfg(unix)]
#[test]
fn replacing_a_root_ancestor_after_open_cannot_redirect_cache_io_into_codex_home() {
	let home = tempfile::tempdir().expect("temporary home");
	let canonical_home = home.path().canonicalize().expect("canonical temporary home");
	let anchor = canonical_home.join("owned-parent");

	fs::create_dir(&anchor).expect("owned parent fixture");
	support::set_mode(&anchor, 0o700);

	let paths = DecodexRoot::new(anchor.join(".decodex")).expect("safe root").paths();
	let limits = CacheLimits::new(2, 16, 8).expect("cache limits");
	let cache = BoundedCache::open(paths, limits).expect("open cache before ancestor swap");
	let original = canonical_home.join("original-owned-parent");

	fs::rename(&anchor, &original).expect("move original root ancestor");

	let codex_home = canonical_home.join(".codex");
	let redirected_root = codex_home.join(".decodex");
	let redirected_cache = redirected_root.join("cache");

	for directory in [&codex_home, &redirected_root, &redirected_cache] {
		fs::create_dir(directory).expect("prepared redirected tree");
		support::set_mode(directory, 0o700);
	}

	std::os::unix::fs::symlink(&codex_home, &anchor).expect("swapped ancestor symlink");

	for result in [
		cache.put("redirect-attempt", b"blocked").map(|_| ()),
		cache.get("redirect-attempt").map(|_| ()),
		cache.usage().map(|_| ()),
		cache.clear(),
	] {
		assert!(matches!(result, Err(StorageError::Path(PathError::Symlink))));
	}

	assert_eq!(fs::read_dir(redirected_cache).expect("read redirected cache").count(), 0);
}
