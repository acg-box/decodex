use std::{
	fmt::{Debug, Display, Formatter},
	path::{Path, PathBuf},
};

use sha2::{Digest as _, Sha256};

use crate::{DecodexPaths, PathError, StorageError, paths};

/// Maximum byte size of one content-addressed blob accepted by this in-memory API.
pub const MAX_BLOB_BYTES: usize = 64 * 1_024 * 1_024;

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

	/// Absolute owned path for PostgreSQL artifact metadata. The path is derived only
	/// from the validated root and digest, never from caller path text.
	pub fn path_for(&self, hash: BlobHash) -> PathBuf {
		self.blob_path(hash).1
	}

	fn verify_existing(&self, hash: BlobHash, path: &Path) -> Result<(), StorageError> {
		let bytes = paths::read_private_file(&self.paths, path, MAX_BLOB_BYTES)?;

		if BlobHash::digest(&bytes) != hash {
			return Err(StorageError::BlobIntegrityMismatch);
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
