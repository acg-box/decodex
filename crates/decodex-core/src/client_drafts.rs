//! Durable local editor bytes, separate from submitted product state and disposable cache.
use std::{fmt, io::ErrorKind, path::Path};

use sha2::{Digest as _, Sha256};

use crate::{DecodexPaths, PathError, path_unix, paths};

/// Aggregate byte ceiling for one desktop draft snapshot.
pub const MAX_CLIENT_DRAFT_BYTES: usize = 4 * 1024 * 1024;
const MAGIC: &[u8; 8] = b"DDRAFT1\n";
const OVERHEAD: usize = 8 + 8 + 32;

/// Redacted failure from the local editor store.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClientDraftError {
	/// Another local writer currently holds the publication lock.
	Busy,
	/// A newer snapshot exists; retain local edits rather than overwrite it.
	Conflict,
	/// Stored bytes failed format or integrity validation.
	Malformed,
	/// The snapshot exceeds the aggregate byte limit.
	Oversized,
	/// The private filesystem boundary rejected an operation.
	Path(PathError),
	/// Publication failed or its durability is uncertain; reload before retrying.
	WriteUnconfirmed(PathError),
}
impl From<PathError> for ClientDraftError {
	fn from(error: PathError) -> Self {
		Self::Path(error)
	}
}
impl fmt::Display for ClientDraftError {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(formatter, "client draft storage failure: {self:?}")
	}
}
impl std::error::Error for ClientDraftError {}

/// One checked local snapshot. Its bytes are never executable input by themselves.
#[derive(Clone, Eq, PartialEq)]
pub struct ClientDraftSnapshot {
	/// Monotonic publication revision; zero means no snapshot exists.
	pub revision: u64,
	/// Bounded editor data owned and validated by the desktop protocol client.
	pub payload: Vec<u8>,
}
impl fmt::Debug for ClientDraftSnapshot {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		formatter
			.debug_struct("ClientDraftSnapshot")
			.field("revision", &self.revision)
			.field("payload", &"<redacted>")
			.finish()
	}
}

/// Local draft file owner. It does not open or create the service database.
#[derive(Clone, Debug)]
pub struct ClientDraftStore {
	paths: DecodexPaths,
}
impl ClientDraftStore {
	/// Open the desktop draft store beneath the platform's canonical Decodex root.
	pub fn open_default() -> Result<Self, ClientDraftError> {
		Self::open(crate::DecodexRoot::platform_default()?.paths())
	}

	/// Open only desktop draft storage beneath an explicitly configured local root.
	pub fn open_at(root: &Path) -> Result<Self, ClientDraftError> {
		Self::open(crate::DecodexRoot::new(root)?.paths())
	}

	fn open(paths: DecodexPaths) -> Result<Self, ClientDraftError> {
		paths.ensure_owned_directory(Path::new("client-drafts"))?;
		Ok(Self { paths })
	}

	/// Read a complete atomic snapshot. Corruption is an error, never an empty draft.
	pub fn load(&self) -> Result<ClientDraftSnapshot, ClientDraftError> {
		let bytes = match paths::read_private_file(
			&self.paths,
			&self.paths.join("client-drafts/current"),
			MAX_CLIENT_DRAFT_BYTES + OVERHEAD,
		) {
			Ok(bytes) => bytes,
			Err(PathError::Io { kind: ErrorKind::NotFound, .. }) =>
				return Ok(ClientDraftSnapshot { revision: 0, payload: Vec::new() }),
			Err(error) => return Err(error.into()),
		};
		if bytes.len() < OVERHEAD || &bytes[..8] != MAGIC {
			return Err(ClientDraftError::Malformed);
		}
		let end = bytes.len() - 32;
		if Sha256::digest(&bytes[..end])[..] != bytes[end..] {
			return Err(ClientDraftError::Malformed);
		}
		let revision =
			u64::from_le_bytes(bytes[8..16].try_into().map_err(|_| ClientDraftError::Malformed)?);
		if revision == 0 {
			return Err(ClientDraftError::Malformed);
		}
		Ok(ClientDraftSnapshot { revision, payload: bytes[16..end].to_vec() })
	}

	/// Atomically replace the expected snapshot under a private cross-process lock.
	/// Empty bytes are a saved edit. Conflicts preserve prior data. Reload after
	/// an I/O failure because replacement may have occurred before a failed sync.
	pub fn save(&self, expected_revision: u64, payload: &[u8]) -> Result<u64, ClientDraftError> {
		if payload.len() > MAX_CLIENT_DRAFT_BYTES {
			return Err(ClientDraftError::Oversized);
		}
		let lock = path_unix::open_private_lock_file(
			&self.paths,
			&self.paths.join("client-drafts/writer.lock"),
		)?;
		lock.try_lock().map_err(|_| ClientDraftError::Busy)?;
		let current = self.load()?;
		if current.revision != expected_revision {
			return Err(ClientDraftError::Conflict);
		}
		let revision = current.revision.checked_add(1).ok_or(ClientDraftError::Malformed)?;
		let mut bytes = Vec::with_capacity(payload.len() + OVERHEAD);
		bytes.extend_from_slice(MAGIC);
		bytes.extend_from_slice(&revision.to_le_bytes());
		bytes.extend_from_slice(payload);
		let digest = Sha256::digest(&bytes);
		bytes.extend_from_slice(&digest);
		paths::atomic_write_replace(
			&self.paths,
			&self.paths.join("client-drafts/current"),
			&bytes,
			MAX_CLIENT_DRAFT_BYTES + OVERHEAD,
		)
		.map_err(ClientDraftError::WriteUnconfirmed)?;
		Ok(revision)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::os::unix::fs::{PermissionsExt as _, symlink};

	fn fixture() -> (tempfile::TempDir, ClientDraftStore) {
		let directory = tempfile::tempdir().unwrap();
		let root =
			crate::DecodexRoot::new(directory.path().canonicalize().unwrap().join("root")).unwrap();
		let store = ClientDraftStore::open_at(root.as_path()).unwrap();
		(directory, store)
	}

	#[test]
	fn client_drafts_reopen_preserve_empty_edits_and_reject_stale_writers() {
		let (_directory, first) = fixture();
		assert_eq!(first.load().unwrap().revision, 0);
		let other = ClientDraftStore::open(first.paths.clone()).unwrap();
		let original = b"private unsent answer";
		assert_eq!(first.save(0, original).unwrap(), 1);
		assert_eq!(other.save(0, b"stale"), Err(ClientDraftError::Conflict));
		assert_eq!(other.load().unwrap().payload, original);
		assert_eq!(other.save(1, b"").unwrap(), 2);
		drop(first);
		let reopened = ClientDraftStore::open(other.paths.clone()).unwrap();
		assert_eq!(reopened.load().unwrap(), ClientDraftSnapshot { revision: 2, payload: vec![] });
		assert!(
			!format!("{:?}", ClientDraftSnapshot { revision: 1, payload: original.to_vec() })
				.contains("private unsent")
		);
		assert!(!reopened.paths.server_dir().exists());
		assert!(!reopened.paths.cache_dir().exists());
	}

	#[test]
	fn client_drafts_lock_and_limits_preserve_the_committed_snapshot() {
		let (_directory, store) = fixture();
		store.save(0, b"saved").unwrap();
		let lock = path_unix::open_private_lock_file(
			&store.paths,
			&store.paths.join("client-drafts/writer.lock"),
		)
		.unwrap();
		lock.try_lock().unwrap();
		assert_eq!(store.save(1, b"busy"), Err(ClientDraftError::Busy));
		drop(lock);
		assert_eq!(
			store.save(1, &vec![0; MAX_CLIENT_DRAFT_BYTES + 1]),
			Err(ClientDraftError::Oversized)
		);
		assert_eq!(store.load().unwrap().payload, b"saved");
		assert_eq!(store.save(1, b"next").unwrap(), 2);
		let file = store.paths.join("client-drafts/current");
		assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
		let mut bytes = std::fs::read(&file).unwrap();
		bytes[16] ^= 1;
		std::fs::write(&file, &bytes).unwrap();
		assert_eq!(store.load(), Err(ClientDraftError::Malformed));
		assert_eq!(store.save(2, b"replacement"), Err(ClientDraftError::Malformed));
		assert_eq!(std::fs::read(&file).unwrap(), bytes);
	}

	#[test]
	fn client_drafts_reject_redirected_paths_and_keep_external_bytes() {
		let (directory, store) = fixture();
		let outside = directory.path().join("outside");
		std::fs::write(&outside, b"untouched").unwrap();
		symlink(&outside, store.paths.join("client-drafts/writer.lock")).unwrap();
		assert!(store.save(0, b"private").is_err());
		std::fs::remove_file(store.paths.join("client-drafts/writer.lock")).unwrap();
		symlink(&outside, store.paths.join("client-drafts/current")).unwrap();
		assert!(store.load().is_err());
		assert!(store.save(0, b"private").is_err());
		assert_eq!(std::fs::read(outside).unwrap(), b"untouched");
	}
	#[test]
	fn client_drafts_ignore_unpublished_staging_and_detect_revision_corruption() {
		let (_directory, store) = fixture();
		let staging = store.paths.join("client-drafts/.tmp-unpublished");
		std::fs::write(&staging, b"interrupted partial write").unwrap();
		assert_eq!(store.load().unwrap().revision, 0);
		store.save(0, b"committed").unwrap();
		std::fs::write(&staging, b"later interrupted partial write").unwrap();
		assert_eq!(store.load().unwrap().payload, b"committed");
		let file = store.paths.join("client-drafts/current");
		let mut bytes = std::fs::read(&file).unwrap();
		bytes[8] ^= 1;
		std::fs::write(&file, bytes).unwrap();
		assert_eq!(store.load(), Err(ClientDraftError::Malformed));
	}
	#[test]
	fn concurrent_draft_publications_have_one_winner() {
		let (_directory, store) = fixture();
		let start = std::sync::Arc::new(std::sync::Barrier::new(2));
		let writers: Vec<_> = [b"first".as_slice(), b"second".as_slice()]
			.into_iter()
			.map(|payload| {
				let store = store.clone();
				let start = start.clone();
				std::thread::spawn(move || {
					start.wait();
					store.save(0, payload)
				})
			})
			.collect();
		let results: Vec<_> = writers.into_iter().map(|writer| writer.join().unwrap()).collect();
		assert_eq!(results.iter().filter(|result| **result == Ok(1)).count(), 1);
		assert_eq!(
			results
				.iter()
				.filter(|result| matches!(
					result,
					Err(ClientDraftError::Busy | ClientDraftError::Conflict)
				))
				.count(),
			1
		);
		let saved = store.load().unwrap();
		assert_eq!(saved.revision, 1);
		assert!([b"first".as_slice(), b"second".as_slice()].contains(&saved.payload.as_slice()));
	}
}
