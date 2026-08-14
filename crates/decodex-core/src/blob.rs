use std::{
	fmt::{Debug, Display, Formatter},
	fs::{self, DirEntry},
	io::{Error, ErrorKind},
	path::{Path, PathBuf},
	time::{Duration, SystemTime},
};

use sha2::{Digest as _, Sha256};

use crate::{DecodexPaths, PathError, StorageError, paths};

/// Maximum byte size of one content-addressed blob accepted by this in-memory API.
pub const MAX_BLOB_BYTES: usize = 64 * 1_024 * 1_024;

const MAX_BLOBS_PER_SHARD: usize = 4_096;

/// Canonical lowercase SHA-256 content identity.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct BlobHash([u8; 32]);
impl BlobHash {
	/// Hash bytes through the vetted SHA-256 implementation selected by the workspace.
	pub fn digest(bytes: &[u8]) -> Self {
		Self(Sha256::digest(bytes).into())
	}

	/// Parse exactly 64 lowercase hexadecimal characters.
	pub fn parse(value: &str) -> Result<Self, StorageError> {
		if value.len() != 64
			|| !value.bytes().all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
		{
			return Err(StorageError::InvalidBlobHash);
		}

		let mut bytes = [0_u8; 32];

		for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
			bytes[index] = (decode_hex(pair[0])? << 4) | decode_hex(pair[1])?;
		}

		Ok(Self(bytes))
	}

	/// Canonical lowercase hexadecimal text.
	pub fn to_hex(self) -> String {
		hex(&self.0)
	}
}

impl Debug for BlobHash {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.debug_tuple("BlobHash").field(&self.to_hex()).finish()
	}
}

impl Display for BlobHash {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str(&self.to_hex())
	}
}

/// Atomic, content-addressed, integrity-verifying local blob owner.
#[derive(Clone, Debug)]
pub struct BlobStore {
	paths: DecodexPaths,
}
impl BlobStore {
	/// Bind the store to the typed Decodex root and verify its fixed layout.
	pub fn open(paths: DecodexPaths) -> Result<Self, StorageError> {
		paths.ensure_layout()?;

		Ok(Self { paths })
	}

	/// Atomically persist bounded bytes by SHA-256. Existing bytes are accepted only
	/// after full integrity verification.
	pub fn put(&self, bytes: &[u8]) -> Result<BlobHash, StorageError> {
		if bytes.len() > MAX_BLOB_BYTES {
			return Err(StorageError::BlobTooLarge { limit: MAX_BLOB_BYTES });
		}

		let hash = BlobHash::digest(bytes);
		let (relative_directory, path) = self.blob_path(hash);

		self.paths.ensure_owned_directory(&relative_directory)?;

		if fs::symlink_metadata(&path).is_err_and(|error| error.kind() == ErrorKind::NotFound) {
			self.ensure_shard_capacity(path.parent().ok_or(StorageError::InvalidCacheEntry)?)?;
		}

		match paths::atomic_write_new(&self.paths, &path, bytes, MAX_BLOB_BYTES) {
			Ok(()) => {},
			Err(PathError::AlreadyExists) => self.verify_existing(hash, &path)?,
			Err(error) => return Err(error.into()),
		}

		Ok(hash)
	}

	/// Read bounded bytes and verify that their SHA-256 identity still matches the
	/// requested content address.
	pub fn read(&self, hash: BlobHash) -> Result<Vec<u8>, StorageError> {
		let (_, path) = self.blob_path(hash);
		let bytes = paths::read_private_file(&self.paths, &path, MAX_BLOB_BYTES)?;

		if BlobHash::digest(&bytes) != hash {
			return Err(StorageError::BlobIntegrityMismatch);
		}

		Ok(bytes)
	}

	/// Absolute owned path for durable-store artifact metadata. The path is derived only
	/// from the validated root and digest, never from caller path text.
	pub fn path_for(&self, hash: BlobHash) -> PathBuf {
		self.blob_path(hash).1
	}

	/// Return one bounded deterministic shard page of canonical grace-aged files.
	/// durable-store authority must prove each candidate unreferenced before removal and
	/// pass the returned cursor to make repeated calls cover the complete namespace.
	pub fn old_inventory(
		&self,
		grace: Duration,
		limit: usize,
		after: Option<BlobInventoryCursor>,
	) -> Result<BlobInventoryPage, StorageError> {
		if limit == 0 || limit > 256 {
			return Err(StorageError::InvalidCacheLimits);
		}

		let cutoff =
			SystemTime::now().checked_sub(grace).ok_or(StorageError::InvalidCacheLimits)?;

		self.validate_inventory_root()?;

		let cursor = after.unwrap_or_default();

		if cursor.shard > u16::from(u8::MAX) {
			return Ok(BlobInventoryPage { entries: Vec::new(), next_cursor: None });
		}

		let shard_name = format!("{:02x}", cursor.shard);
		let shard_path = self.paths.join("blobs/sha256").join(&shard_name);
		let mut files = match fs::read_dir(shard_path) {
			Ok(files) => files
				.map(|file| inventory_file(file, &shard_name))
				.collect::<Result<Vec<_>, _>>()?,
			Err(error) if error.kind() == ErrorKind::NotFound => Vec::new(),
			Err(_) => return Err(StorageError::InvalidCacheEntry),
		};

		if files.len() > MAX_BLOBS_PER_SHARD {
			return Err(StorageError::InvalidCacheLimits);
		}

		files.sort_by_key(|entry| entry.hash);

		let entries = files
			.into_iter()
			.filter(|entry| cursor.after.is_none_or(|after| entry.hash > after))
			.filter(|entry| entry.modified <= cutoff)
			.take(limit)
			.map(|entry| BlobInventoryEntry { hash: entry.hash })
			.collect::<Vec<_>>();
		let next_cursor = if entries.len() == limit {
			entries
				.last()
				.map(|entry| BlobInventoryCursor { shard: cursor.shard, after: Some(entry.hash) })
		} else if cursor.shard < u16::from(u8::MAX) {
			Some(BlobInventoryCursor { shard: cursor.shard + 1, after: None })
		} else {
			None
		};

		Ok(BlobInventoryPage { entries, next_cursor })
	}

	fn validate_inventory_root(&self) -> Result<(), StorageError> {
		let root = self.paths.join("blobs/sha256");
		let mut count = 0_usize;

		for shard in fs::read_dir(root).map_err(|_| StorageError::InvalidCacheEntry)? {
			count += 1;

			if count > 256 {
				return Err(StorageError::InvalidCacheLimits);
			}

			let shard = shard.map_err(|_| StorageError::InvalidCacheEntry)?;
			let name = shard.file_name();
			let name = name.to_str().ok_or(StorageError::InvalidCacheEntry)?;

			if name.len() != 2
				|| !name.bytes().all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
				|| !shard.file_type().map_err(|_| StorageError::InvalidCacheEntry)?.is_dir()
			{
				return Err(StorageError::InvalidCacheEntry);
			}
		}

		Ok(())
	}

	/// Recheck age and remove one candidate. Call only while holding the shared durable-store
	/// blob-writer/collector advisory lock and after proving no database reference exists.
	pub fn remove_orphan_if_old(
		&self,
		hash: BlobHash,
		grace: Duration,
	) -> Result<bool, StorageError> {
		let path = self.path_for(hash);
		let metadata = match fs::metadata(&path) {
			Ok(metadata) => metadata,
			Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
			Err(_) => return Err(StorageError::InvalidCacheEntry),
		};
		let modified = metadata.modified().map_err(|_| StorageError::InvalidCacheEntry)?;

		if modified
			> SystemTime::now().checked_sub(grace).ok_or(StorageError::InvalidCacheLimits)?
		{
			return Ok(false);
		}

		paths::remove_private_file(&self.paths, &path)?;

		Ok(true)
	}

	fn verify_existing(&self, hash: BlobHash, path: &Path) -> Result<(), StorageError> {
		let bytes = paths::read_private_file(&self.paths, path, MAX_BLOB_BYTES)?;

		if BlobHash::digest(&bytes) != hash {
			return Err(StorageError::BlobIntegrityMismatch);
		}

		Ok(())
	}

	fn ensure_shard_capacity(&self, shard: &Path) -> Result<(), StorageError> {
		let mut count = 0_usize;

		for entry in fs::read_dir(shard).map_err(|_| StorageError::InvalidCacheEntry)? {
			entry.map_err(|_| StorageError::InvalidCacheEntry)?;

			count += 1;

			if count >= MAX_BLOBS_PER_SHARD {
				return Err(StorageError::InvalidCacheLimits);
			}
		}

		Ok(())
	}

	fn blob_path(&self, hash: BlobHash) -> (PathBuf, PathBuf) {
		let encoded = hash.to_hex();
		let relative_directory = PathBuf::from("blobs/sha256").join(&encoded[..2]);
		let path = self.paths.join(&relative_directory).join(encoded);

		(relative_directory, path)
	}
}

/// One old content-addressed file eligible for database-coordinated orphan inspection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BlobInventoryEntry {
	/// Filename-derived canonical content address.
	pub hash: BlobHash,
}

/// Opaque deterministic continuation through the fixed SHA-256 shard namespace.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BlobInventoryCursor {
	shard: u16,
	after: Option<BlobHash>,
}

/// One bounded inventory page and its complete-scan continuation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlobInventoryPage {
	/// Grace-aged candidates in canonical hash order.
	pub entries: Vec<BlobInventoryEntry>,
	/// Continuation for the next bounded shard scan; `None` means complete.
	pub next_cursor: Option<BlobInventoryCursor>,
}

struct InventoryFile {
	hash: BlobHash,
	modified: SystemTime,
}

fn inventory_file(
	file: Result<DirEntry, Error>,
	shard: &str,
) -> Result<InventoryFile, StorageError> {
	let file = file.map_err(|_| StorageError::InvalidCacheEntry)?;

	if !file.file_type().map_err(|_| StorageError::InvalidCacheEntry)?.is_file() {
		return Err(StorageError::InvalidCacheEntry);
	}

	let encoded = file.file_name();
	let encoded = encoded.to_str().ok_or(StorageError::InvalidCacheEntry)?;
	let hash = BlobHash::parse(encoded)?;

	if !encoded.starts_with(shard) {
		return Err(StorageError::InvalidCacheEntry);
	}

	let modified = file
		.metadata()
		.map_err(|_| StorageError::InvalidCacheEntry)?
		.modified()
		.map_err(|_| StorageError::InvalidCacheEntry)?;

	Ok(InventoryFile { hash, modified })
}
fn decode_hex(value: u8) -> Result<u8, StorageError> {
	match value {
		b'0'..=b'9' => Ok(value - b'0'),
		b'a'..=b'f' => Ok(value - b'a' + 10),
		_ => Err(StorageError::InvalidBlobHash),
	}
}

fn hex(bytes: &[u8]) -> String {
	const HEX: &[u8; 16] = b"0123456789abcdef";

	let mut encoded = String::with_capacity(bytes.len() * 2);

	for &byte in bytes {
		encoded.push(char::from(HEX[usize::from(byte >> 4)]));
		encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
	}

	encoded
}
