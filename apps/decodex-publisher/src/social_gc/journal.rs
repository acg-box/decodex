use std::{
	collections::BTreeMap,
	ffi::{OsStr, OsString},
	fs::{self, File},
	io::Write as _,
	os::unix::fs::{MetadataExt as _, PermissionsExt as _},
	path::{Component, Path, PathBuf},
	time::{Duration, SystemTime, UNIX_EPOCH},
};

use rustix::{
	fs::{self as unix_fs, AtFlags, Mode, OFlags, RenameFlags},
	io::Errno,
};
use serde::{Deserialize, Serialize};

use super::{
	GcFailure, GcJournalStep, GcResult, MAX_GC_BYTES, MAX_GC_FILES, digest_hex,
	inventory::{self, StoredFile, private_identity_sha256},
};
use crate::{
	SocialGcRequest,
	filesystem::{PinnedPrivateDirectory, PrivateFileIdentity, open_private_directory_descriptor},
};

const JOURNAL_FILE: &str = "social-gc-journal.json";
const JOURNAL_SCHEMA: &str = "decodex/social-gc-journal/2";
const MAX_JOURNAL_BYTES: u64 = 1024 * 1024;
const MAX_JOURNAL_KEY_BYTES: usize = 1024;
const MAX_JOURNAL_TEMP_FILES: usize = 64;
const MAX_LOCK_DIRECTORY_ENTRIES: usize = 8_192;
const MAX_CLOCK_SKEW: Duration = Duration::from_secs(5 * 60);

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct GcJournal {
	schema: String,
	entries: Vec<JournalEntry>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct JournalEntry {
	key: String,
	raw_sha256: String,
	identity_sha256: String,
	parent: ParentDirectoryIdentity,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
struct ParentDirectoryIdentity {
	dev: u64,
	ino: u64,
	uid: u32,
	mode: u32,
}

struct CurrentEntry {
	key: String,
	directory: PinnedPrivateDirectory,
	name: OsString,
	identity: PrivateFileIdentity,
}

pub(super) fn persist(
	request: &SocialGcRequest,
	files: &[StoredFile],
	hook: &mut impl FnMut(GcJournalStep) -> GcResult<()>,
) -> GcResult<()> {
	if files.is_empty() {
		return Ok(());
	}
	let root = crate::repo_root().map_err(|_| GcFailure("social_gc_journal_failed"))?;
	let locks_path = absolute_path(&root, &request.locks_dir)?;
	let mut entries = Vec::with_capacity(files.len());
	for file in files {
		if file.key.len() > MAX_JOURNAL_KEY_BYTES {
			return Err(GcFailure("social_gc_journal_failed"));
		}
		let resolved = resolve_entry_path(&root, request, &file.key)?;
		if resolved != file.path() {
			return Err(GcFailure("social_gc_journal_failed"));
		}
		let parent_path = resolved.parent().ok_or(GcFailure("social_gc_journal_failed"))?;
		let Some(parent_directory) = crate::open_existing_exact_private_directory(parent_path)
			.map_err(|_| GcFailure("social_gc_journal_failed"))?
		else {
			return Err(GcFailure("social_gc_journal_failed"));
		};
		let parent_descriptor = open_private_directory_descriptor(parent_path, false)
			.map_err(|_| GcFailure("social_gc_journal_failed"))?;
		let parent = directory_identity(&parent_directory, &parent_descriptor)
			.map_err(|_| GcFailure("social_gc_journal_failed"))?;
		if parent_directory.identity().map_err(|_| GcFailure("social_gc_journal_failed"))?
			!= file.directory.identity().map_err(|_| GcFailure("social_gc_journal_failed"))?
		{
			return Err(GcFailure("social_gc_journal_failed"));
		}
		let (raw_sha256, identity_sha256) =
			file.journal_snapshot().map_err(|_| GcFailure("social_gc_delete_race"))?;
		entries.push(JournalEntry { key: file.key.clone(), raw_sha256, identity_sha256, parent });
	}
	entries.sort_by(|left, right| left.key.cmp(&right.key));
	if entries.len() > MAX_GC_FILES
		|| entries.windows(2).any(|entries| entries[0].key == entries[1].key)
	{
		return Err(GcFailure("social_gc_journal_failed"));
	}
	let journal = GcJournal { schema: JOURNAL_SCHEMA.into(), entries };
	let mut raw =
		serde_json::to_vec(&journal).map_err(|_| GcFailure("social_gc_journal_failed"))?;
	raw.push(b'\n');
	if raw.is_empty() || raw.len() as u64 > MAX_JOURNAL_BYTES {
		return Err(GcFailure("social_gc_journal_failed"));
	}

	let directory = open_private_directory_descriptor(&locks_path, false)
		.map_err(|_| GcFailure("social_gc_journal_failed"))?;
	let temporary_name = temporary_name()?;
	let mut published = false;
	let result = (|| {
		let descriptor = unix_fs::openat(
			&directory,
			&temporary_name,
			OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::CLOEXEC | OFlags::NOFOLLOW,
			Mode::from_bits_retain(0o600),
		)
		.map_err(|_| GcFailure("social_gc_journal_failed"))?;
		let mut temporary = File::from(descriptor);
		temporary
			.set_permissions(fs::Permissions::from_mode(0o600))
			.map_err(|_| GcFailure("social_gc_journal_failed"))?;
		let metadata = temporary.metadata().map_err(|_| GcFailure("social_gc_journal_failed"))?;
		if !metadata.is_file()
			|| metadata.uid() != current_uid()
			|| metadata.nlink() != 1
			|| metadata.permissions().mode() & 0o777 != 0o600
		{
			return Err(GcFailure("social_gc_journal_failed"));
		}
		temporary.write_all(&raw).map_err(|_| GcFailure("social_gc_journal_failed"))?;
		hook(GcJournalStep::BeforeJournalFileSync)?;
		temporary.sync_all().map_err(|_| GcFailure("social_gc_journal_failed"))?;
		hook(GcJournalStep::AfterJournalFileSync)?;
		drop(temporary);

		hook(GcJournalStep::BeforeJournalPublish)?;
		unix_fs::renameat_with(
			&directory,
			&temporary_name,
			&directory,
			JOURNAL_FILE,
			RenameFlags::NOREPLACE,
		)
		.map_err(|error| {
			if error == Errno::EXIST {
				GcFailure("social_gc_journal_exists")
			} else {
				GcFailure("social_gc_journal_failed")
			}
		})?;
		published = true;
		hook(GcJournalStep::AfterJournalPublish)?;
		hook(GcJournalStep::BeforeJournalPublishDirectorySync)?;
		directory.sync_all().map_err(|_| GcFailure("social_gc_journal_failed"))?;
		hook(GcJournalStep::AfterJournalPublishDirectorySync)?;

		Ok(())
	})();
	if !published {
		let _ = unix_fs::unlinkat(&directory, &temporary_name, AtFlags::empty());
	}
	result
}

pub(super) fn recover(
	request: &SocialGcRequest,
	hook: &mut impl FnMut(GcJournalStep) -> GcResult<()>,
) -> GcResult<()> {
	let root = crate::repo_root().map_err(|_| GcFailure("social_gc_recovery_failed"))?;
	let locks_path = absolute_path(&root, &request.locks_dir)?;
	let Some(locks_directory) = crate::open_existing_exact_private_directory(&locks_path)
		.map_err(|_| GcFailure("social_gc_recovery_failed"))?
	else {
		return Ok(());
	};
	let journal_name = OsStr::new(JOURNAL_FILE);
	let locks_descriptor = open_private_directory_descriptor(&locks_path, false)
		.map_err(|_| GcFailure("social_gc_recovery_failed"))?;
	require_same_directory(&locks_directory, &locks_descriptor)?;
	cleanup_temporary_journals(&locks_directory, &locks_descriptor)?;
	if !child_exists(&locks_descriptor, journal_name)? {
		return Ok(());
	}
	let (value, journal_identity, raw) = locks_directory
		.read_json(journal_name, MAX_JOURNAL_BYTES)
		.map_err(|_| GcFailure("social_gc_recovery_failed"))?;
	let journal: GcJournal =
		serde_json::from_value(value).map_err(|_| GcFailure("social_gc_recovery_failed"))?;
	validate_journal(&journal, raw.len())?;

	let mut directories = BTreeMap::new();
	let mut current_entries = Vec::new();
	let mut total_bytes = 0_u64;
	for entry in &journal.entries {
		let path = resolve_entry_path(&root, request, &entry.key)?;
		let parent = path.parent().ok_or(GcFailure("social_gc_recovery_failed"))?;
		let name = path.file_name().ok_or(GcFailure("social_gc_recovery_failed"))?.to_owned();
		let Some(directory) = crate::open_existing_exact_private_directory(parent)
			.map_err(|_| GcFailure("social_gc_recovery_failed"))?
		else {
			return Err(GcFailure("social_gc_recovery_failed"));
		};
		directory.verify_current_path().map_err(|_| GcFailure("social_gc_recovery_failed"))?;
		let parent_descriptor = open_private_directory_descriptor(parent, false)
			.map_err(|_| GcFailure("social_gc_recovery_failed"))?;
		let parent_identity = directory_identity(&directory, &parent_descriptor)
			.map_err(|_| GcFailure("social_gc_recovery_failed"))?;
		if parent_identity != entry.parent {
			return Err(GcFailure("social_gc_recovery_failed"));
		}
		directories.insert((parent_identity.dev, parent_identity.ino), directory.clone());
		if !child_exists(&parent_descriptor, &name)? {
			continue;
		}
		let (_value, identity, bytes) = directory
			.read_json(&name, MAX_JOURNAL_BYTES)
			.map_err(|_| GcFailure("social_gc_recovery_failed"))?;
		total_bytes = total_bytes
			.checked_add(bytes.len() as u64)
			.filter(|total| *total <= MAX_GC_BYTES)
			.ok_or(GcFailure("social_gc_recovery_failed"))?;
		if digest_hex(&bytes) != entry.raw_sha256
			|| private_identity_sha256(identity) != entry.identity_sha256
		{
			return Err(GcFailure("social_gc_recovery_failed"));
		}
		current_entries.push(CurrentEntry { key: entry.key.clone(), directory, name, identity });
	}

	for entry in &current_entries {
		entry
			.directory
			.verify_current_path()
			.map_err(|_| GcFailure("social_gc_recovery_failed"))?;
		hook(GcJournalStep::BeforePlannedFileUnlink(entry.key.clone()))?;
		entry
			.directory
			.unlink_verified(&entry.name, entry.identity)
			.map_err(|_| GcFailure("social_gc_recovery_failed"))?;
		hook(GcJournalStep::AfterPlannedFileUnlink(entry.key.clone()))?;
	}
	for (identity, directory) in &directories {
		directory.verify_current_path().map_err(|_| GcFailure("social_gc_recovery_failed"))?;
		hook(GcJournalStep::BeforeDataDirectorySync(*identity))?;
		directory.sync().map_err(|_| GcFailure("social_gc_recovery_failed"))?;
		hook(GcJournalStep::AfterDataDirectorySync(*identity))?;
	}

	locks_directory.verify_current_path().map_err(|_| GcFailure("social_gc_recovery_failed"))?;
	hook(GcJournalStep::BeforeJournalUnlink)?;
	locks_directory
		.unlink_verified(journal_name, journal_identity)
		.map_err(|_| GcFailure("social_gc_recovery_failed"))?;
	hook(GcJournalStep::AfterJournalUnlink)?;
	hook(GcJournalStep::BeforeJournalRemovalDirectorySync)?;
	locks_directory.sync().map_err(|_| GcFailure("social_gc_recovery_failed"))?;
	hook(GcJournalStep::AfterJournalRemovalDirectorySync)?;

	Ok(())
}

fn validate_journal(journal: &GcJournal, raw_len: usize) -> GcResult<()> {
	if journal.schema != JOURNAL_SCHEMA
		|| journal.entries.is_empty()
		|| journal.entries.len() > MAX_GC_FILES
		|| raw_len == 0
		|| raw_len as u64 > MAX_JOURNAL_BYTES
	{
		return Err(GcFailure("social_gc_recovery_failed"));
	}
	let mut previous = None;
	for entry in &journal.entries {
		if entry.key.is_empty()
			|| entry.key.len() > MAX_JOURNAL_KEY_BYTES
			|| !is_digest(&entry.raw_sha256)
			|| !is_digest(&entry.identity_sha256)
			|| entry.parent.uid != current_uid()
			|| entry.parent.mode != 0o700
			|| previous.is_some_and(|previous| previous >= entry.key.as_str())
		{
			return Err(GcFailure("social_gc_recovery_failed"));
		}
		previous = Some(entry.key.as_str());
	}

	Ok(())
}

fn resolve_entry_path(root: &Path, request: &SocialGcRequest, key: &str) -> GcResult<PathBuf> {
	let key_path = Path::new(key);
	let path = if key_path.is_absolute() { key_path.to_path_buf() } else { root.join(key_path) };
	validate_absolute_path(&path)?;
	let candidates = absolute_path(root, &request.candidates_dir)?;
	let reservations = absolute_path(root, &request.reservations_dir)?;
	let posts = absolute_path(root, &request.posts_dir)?;
	let outcomes = absolute_path(root, &request.outcomes_dir)?;
	let attempts = absolute_path(root, &request.attempts_dir)?;
	let strategies = absolute_path(root, &request.strategies_dir)?;

	if has_shape(&path, &candidates, &[PathPart::Artifact])
		|| has_shape(&path, &posts, &[PathPart::Artifact])
		|| has_shape(&path, &outcomes, &[PathPart::Artifact])
		|| has_shape(&path, &strategies, &[PathPart::Artifact])
		|| has_shape(&path, &reservations, &[PathPart::Day, PathPart::Digest])
		|| has_shape(&path, &attempts, &[PathPart::Month, PathPart::Attempt])
	{
		return Ok(path);
	}

	Err(GcFailure("social_gc_recovery_failed"))
}

#[derive(Clone, Copy)]
enum PathPart {
	Artifact,
	Attempt,
	Day,
	Digest,
	Month,
}

fn has_shape(path: &Path, root: &Path, shape: &[PathPart]) -> bool {
	let Ok(relative) = path.strip_prefix(root) else {
		return false;
	};
	let components = relative.components().collect::<Vec<_>>();
	if components.len() != shape.len() {
		return false;
	}
	components.iter().zip(shape).all(|(component, expected)| {
		let Component::Normal(value) = component else {
			return false;
		};
		match expected {
			PathPart::Artifact => inventory::is_artifact_filename(value),
			PathPart::Attempt => inventory::is_attempt_filename(value),
			PathPart::Day => value.to_str().is_some_and(inventory::is_day),
			PathPart::Digest => inventory::is_digest_filename(value),
			PathPart::Month => value.to_str().is_some_and(inventory::is_month),
		}
	})
}

fn absolute_path(root: &Path, raw: &Path) -> GcResult<PathBuf> {
	let path = crate::resolve_against(root, raw);
	validate_absolute_path(&path)?;
	Ok(path)
}

fn validate_absolute_path(path: &Path) -> GcResult<()> {
	if !path.is_absolute()
		|| path
			.components()
			.any(|component| !matches!(component, Component::RootDir | Component::Normal(_)))
	{
		return Err(GcFailure("social_gc_recovery_failed"));
	}
	Ok(())
}

fn temporary_name() -> GcResult<OsString> {
	let mut random = [0_u8; 16];
	getrandom::fill(&mut random).map_err(|_| GcFailure("social_gc_journal_failed"))?;
	let suffix = random.iter().map(|byte| format!("{byte:02x}")).collect::<String>();
	Ok(format!(".{JOURNAL_FILE}.{suffix}.tmp").into())
}

fn cleanup_temporary_journals(
	directory: &PinnedPrivateDirectory,
	descriptor: &File,
) -> GcResult<()> {
	let names = directory
		.entries_bounded(MAX_LOCK_DIRECTORY_ENTRIES)
		.map_err(|_| GcFailure("social_gc_recovery_failed"))?;
	let mut temporary_count = 0_usize;
	let mut removed = false;
	for name in names {
		if !is_temporary_name(&name) {
			continue;
		}
		temporary_count =
			temporary_count.checked_add(1).ok_or(GcFailure("social_gc_recovery_failed"))?;
		if temporary_count > MAX_JOURNAL_TEMP_FILES {
			return Err(GcFailure("social_gc_recovery_failed"));
		}
		let expected = temporary_identity(descriptor, &name)?;
		let now = SystemTime::now();
		let modified = expected.modified;
		if modified.duration_since(UNIX_EPOCH).is_err()
			|| modified.duration_since(now).is_ok_and(|future| future > MAX_CLOCK_SKEW)
		{
			return Err(GcFailure("social_gc_recovery_failed"));
		}
		let actual = temporary_identity(descriptor, &name)?;
		if actual != expected {
			return Err(GcFailure("social_gc_recovery_failed"));
		}
		unix_fs::unlinkat(descriptor, &name, AtFlags::empty())
			.map_err(|_| GcFailure("social_gc_recovery_failed"))?;
		removed = true;
	}
	if removed {
		directory.sync().map_err(|_| GcFailure("social_gc_recovery_failed"))?;
	}

	Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TemporaryIdentity {
	dev: u64,
	ino: u64,
	len: u64,
	mtime: i64,
	mtime_nsec: i64,
	uid: u32,
	mode: u32,
	nlink: u64,
	modified: SystemTime,
}

fn temporary_identity(directory: &File, name: &OsStr) -> GcResult<TemporaryIdentity> {
	let descriptor = unix_fs::openat(
		directory,
		name,
		OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
		Mode::empty(),
	)
	.map_err(|_| GcFailure("social_gc_recovery_failed"))?;
	let file = File::from(descriptor);
	let metadata = file.metadata().map_err(|_| GcFailure("social_gc_recovery_failed"))?;
	if !metadata.is_file()
		|| metadata.uid() != current_uid()
		|| metadata.permissions().mode() & 0o777 != 0o600
		|| metadata.nlink() != 1
		|| metadata.len() > MAX_JOURNAL_BYTES
	{
		return Err(GcFailure("social_gc_recovery_failed"));
	}

	Ok(TemporaryIdentity {
		dev: metadata.dev(),
		ino: metadata.ino(),
		len: metadata.len(),
		mtime: metadata.mtime(),
		mtime_nsec: metadata.mtime_nsec(),
		uid: metadata.uid(),
		mode: metadata.permissions().mode() & 0o777,
		nlink: metadata.nlink(),
		modified: metadata.modified().map_err(|_| GcFailure("social_gc_recovery_failed"))?,
	})
}

fn is_temporary_name(name: &OsStr) -> bool {
	let Some(value) = name.to_str() else {
		return false;
	};
	let Some(random) = value
		.strip_prefix(&format!(".{JOURNAL_FILE}."))
		.and_then(|value| value.strip_suffix(".tmp"))
	else {
		return false;
	};
	random.len() == 32
		&& random.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn child_exists(directory: &File, name: &OsStr) -> GcResult<bool> {
	match unix_fs::openat(
		directory,
		name,
		OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
		Mode::empty(),
	) {
		Ok(file) => {
			drop(file);
			Ok(true)
		},
		Err(Errno::NOENT) => Ok(false),
		Err(_) => Err(GcFailure("social_gc_recovery_failed")),
	}
}

fn require_same_directory(directory: &PinnedPrivateDirectory, descriptor: &File) -> GcResult<()> {
	directory_identity(directory, descriptor)
		.map_err(|_| GcFailure("social_gc_recovery_failed"))?;
	Ok(())
}

fn directory_identity(
	directory: &PinnedPrivateDirectory,
	descriptor: &File,
) -> Result<ParentDirectoryIdentity, ()> {
	let metadata = descriptor.metadata().map_err(|_| ())?;
	let expected = directory.identity().map_err(|_| ())?;
	let identity = ParentDirectoryIdentity {
		dev: metadata.dev(),
		ino: metadata.ino(),
		uid: metadata.uid(),
		mode: metadata.permissions().mode() & 0o777,
	};
	if (identity.dev, identity.ino) != expected
		|| identity.uid != current_uid()
		|| identity.mode != 0o700
	{
		return Err(());
	}

	Ok(identity)
}

fn is_digest(value: &str) -> bool {
	value.len() == 64
		&& value.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn current_uid() -> u32 {
	unsafe { libc::geteuid() }
}
