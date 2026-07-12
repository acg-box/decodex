//! Fail-closed selection of a generation-specific runtime database.

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt as _;
use std::{
	fs::{self, OpenOptions},
	io::Write as _,
	os::fd::AsRawFd as _,
	path::{Component, Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{
	prelude::{Result, eyre},
	state::StateStore,
};

const RUNTIME_FORMAT_SCHEMA: &str = "decodex/runtime-format/2";
const RUNTIME_FORMAT_MANIFEST: &str = "runtime-format.toml";
const RUNTIME_INITIALIZATION_JOURNAL: &str = "runtime-initialization.toml";
const LEGACY_DATABASE: &str = "runtime.sqlite3";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RuntimeFormatManifest {
	schema: String,
	generation: u64,
	database_relative_path: PathBuf,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum RuntimeInitializationStage {
	Planned,
	DatabasePrepared,
	AnchorInitialized,
	ManifestPublished,
	Complete,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RuntimeInitializationJournal {
	schema: String,
	generation: u64,
	genesis_hash: String,
	stage: RuntimeInitializationStage,
}

pub(super) fn selected_runtime_db_path_from(runtime_root: &Path) -> Result<PathBuf> {
	Ok(selected_runtime_from(runtime_root)?.1)
}

pub(super) fn selected_runtime_generation_from(runtime_root: &Path) -> Result<u64> {
	Ok(selected_runtime_from(runtime_root)?.0)
}

fn selected_runtime_from(runtime_root: &Path) -> Result<(u64, PathBuf)> {
	let legacy_path = runtime_root.join(LEGACY_DATABASE);
	if legacy_path.is_file() {
		eyre::bail!("legacy_runtime_database_requires_offline_archive_and_reset");
	}
	let manifest_path = runtime_root.join(RUNTIME_FORMAT_MANIFEST);
	let manifest_bytes = fs::read(&manifest_path).map_err(|error| {
		eyre::eyre!("runtime_format_manifest_unavailable:{}:{error}", manifest_path.display())
	})?;
	let manifest_text = std::str::from_utf8(&manifest_bytes)
		.map_err(|_| eyre::eyre!("runtime_format_manifest_not_utf8"))?;
	let manifest: RuntimeFormatManifest = toml::from_str(manifest_text)
		.map_err(|_| eyre::eyre!("runtime_format_manifest_invalid"))?;
	if manifest.schema != RUNTIME_FORMAT_SCHEMA || manifest.generation == 0 {
		eyre::bail!("runtime_format_manifest_unsupported");
	}
	let journal = read_initialization_journal(runtime_root)?;
	if journal.generation != manifest.generation
		|| journal.stage != RuntimeInitializationStage::Complete
	{
		eyre::bail!("runtime_initialization_journal_incomplete_or_mismatched");
	}
	validate_database_relative_path(&manifest.database_relative_path, manifest.generation)?;
	let selected = runtime_root.join(&manifest.database_relative_path);
	if selected == legacy_path {
		eyre::bail!("runtime_format_manifest_selects_legacy_database");
	}
	if !selected.is_file() {
		eyre::bail!("runtime_format_selected_database_unavailable");
	}
	Ok((manifest.generation, selected))
}

pub(super) fn publish_runtime_generation_from(
	runtime_root: &Path,
	generation: u64,
) -> Result<PathBuf> {
	if generation == 0 {
		eyre::bail!("runtime_generation_zero");
	}
	let legacy_path = runtime_root.join(LEGACY_DATABASE);
	if !legacy_path.is_dir() {
		eyre::bail!("legacy_runtime_tombstone_missing");
	}
	let relative_database =
		PathBuf::from("generations").join(generation.to_string()).join(LEGACY_DATABASE);
	let database = runtime_root.join(&relative_database);
	if !database.is_file() {
		eyre::bail!("runtime_generation_database_not_prepared");
	}
	let manifest = runtime_root.join(RUNTIME_FORMAT_MANIFEST);
	if manifest.exists() {
		eyre::bail!("runtime_format_manifest_already_published");
	}
	let temp = runtime_root.join(format!(".runtime-format.toml.{}.tmp", std::process::id()));
	if temp.exists() {
		fs::remove_file(&temp)?;
	}
	let body = format!(
		"schema = \"{RUNTIME_FORMAT_SCHEMA}\"\ngeneration = {generation}\ndatabase_relative_path = \"{}\"\n",
		relative_database.display()
	);
	let mut options = OpenOptions::new();
	options.create_new(true).write(true);
	#[cfg(unix)]
	options.mode(0o600);
	let mut file = options.open(&temp)?;
	file.write_all(body.as_bytes())?;
	file.sync_all()?;
	drop(file);
	if let Err(error) = fs::rename(&temp, &manifest) {
		let _ = fs::remove_file(&temp);
		return Err(error.into());
	}
	OpenOptions::new().read(true).open(runtime_root)?.sync_all()?;
	Ok(database)
}

pub(super) fn initialize_fresh_runtime_generation_from(
	runtime_root: &Path,
	generation: u64,
	genesis_hash: &[u8; 32],
) -> Result<PathBuf> {
	let lock_path = runtime_root.join(".runtime-initialize.lock");
	let _lock = RuntimeInitializationLock::acquire(&lock_path)?;
	initialize_fresh_runtime_generation_locked(runtime_root, generation, genesis_hash)
}

fn initialize_fresh_runtime_generation_locked(
	runtime_root: &Path,
	generation: u64,
	genesis_hash: &[u8; 32],
) -> Result<PathBuf> {
	if generation == 0 {
		eyre::bail!("runtime_generation_zero");
	}
	if !runtime_root.join(LEGACY_DATABASE).is_dir() {
		eyre::bail!("legacy_runtime_tombstone_missing");
	}
	let mut journal =
		load_or_create_initialization_journal(runtime_root, generation, genesis_hash)?;
	let genesis_hash = decode_hash(&journal.genesis_hash)?;
	let generations = runtime_root.join("generations");
	let generation_root = generations.join(generation.to_string());
	let database = generation_root.join(LEGACY_DATABASE);
	if journal.stage == RuntimeInitializationStage::Planned {
		fs::create_dir_all(&generation_root)?;
		let store = StateStore::open(&database)?;
		store.initialize_authority_generation(generation, &genesis_hash)?;
		let events = store.verify_authority_events()?;
		if !events.is_empty() {
			eyre::bail!("fresh_runtime_authority_chain_not_empty");
		}
		drop(store);
		OpenOptions::new().read(true).open(&database)?.sync_all()?;
		OpenOptions::new().read(true).open(&generation_root)?.sync_all()?;
		OpenOptions::new().read(true).open(&generations)?.sync_all()?;
		advance_initialization_journal(
			runtime_root,
			&mut journal,
			RuntimeInitializationStage::DatabasePrepared,
		)?;
	}
	if journal.stage == RuntimeInitializationStage::DatabasePrepared {
		let store = StateStore::open(&database)?;
		let events = store.verify_authority_events()?;
		#[cfg(not(test))]
		crate::lane_authority::protected_head::AuthorityAnchor::initialize(
			runtime_root,
			generation,
			&genesis_hash,
			&events,
		)?;
		#[cfg(test)]
		crate::lane_authority::protected_head::AuthorityAnchor::initialize_for_test(
			runtime_root,
			generation,
			&genesis_hash,
			&events,
		)?;
		drop(store);
		advance_initialization_journal(
			runtime_root,
			&mut journal,
			RuntimeInitializationStage::AnchorInitialized,
		)?;
	}
	if journal.stage == RuntimeInitializationStage::AnchorInitialized {
		if runtime_root.join(RUNTIME_FORMAT_MANIFEST).exists() {
			let manifest = read_runtime_manifest(runtime_root)?;
			if manifest.generation != generation {
				eyre::bail!("runtime_format_manifest_generation_mismatch");
			}
		} else {
			publish_runtime_generation_from(runtime_root, generation)?;
		}
		advance_initialization_journal(
			runtime_root,
			&mut journal,
			RuntimeInitializationStage::ManifestPublished,
		)?;
	}
	if journal.stage == RuntimeInitializationStage::ManifestPublished {
		let manifest = read_runtime_manifest(runtime_root)?;
		if manifest.generation != generation {
			eyre::bail!("runtime_format_manifest_generation_mismatch");
		}
		advance_initialization_journal(
			runtime_root,
			&mut journal,
			RuntimeInitializationStage::Complete,
		)?;
	}
	selected_runtime_db_path_from(runtime_root)
}

fn read_runtime_manifest(runtime_root: &Path) -> Result<RuntimeFormatManifest> {
	let text = fs::read_to_string(runtime_root.join(RUNTIME_FORMAT_MANIFEST))?;
	toml::from_str(&text).map_err(Into::into)
}

fn load_or_create_initialization_journal(
	runtime_root: &Path,
	generation: u64,
	genesis_hash: &[u8; 32],
) -> Result<RuntimeInitializationJournal> {
	let path = runtime_root.join(RUNTIME_INITIALIZATION_JOURNAL);
	if path.exists() {
		let journal = read_initialization_journal(runtime_root)?;
		if journal.generation != generation {
			eyre::bail!("runtime_initialization_generation_mismatch");
		}
		return Ok(journal);
	}
	if runtime_root.join(RUNTIME_FORMAT_MANIFEST).exists() {
		eyre::bail!("runtime_initialization_journal_missing_for_manifest");
	}
	let journal = RuntimeInitializationJournal {
		schema: String::from("decodex/runtime-initialization/1"),
		generation,
		genesis_hash: hex_hash(genesis_hash),
		stage: RuntimeInitializationStage::Planned,
	};
	write_initialization_journal(runtime_root, &journal)?;
	Ok(journal)
}

fn read_initialization_journal(runtime_root: &Path) -> Result<RuntimeInitializationJournal> {
	let journal: RuntimeInitializationJournal =
		toml::from_str(&fs::read_to_string(runtime_root.join(RUNTIME_INITIALIZATION_JOURNAL))?)?;
	if journal.schema != "decodex/runtime-initialization/1" {
		eyre::bail!("runtime_initialization_journal_schema_unsupported");
	}
	decode_hash(&journal.genesis_hash)?;
	Ok(journal)
}

fn advance_initialization_journal(
	runtime_root: &Path,
	journal: &mut RuntimeInitializationJournal,
	stage: RuntimeInitializationStage,
) -> Result<()> {
	journal.stage = stage;
	write_initialization_journal(runtime_root, journal)
}

fn write_initialization_journal(
	runtime_root: &Path,
	journal: &RuntimeInitializationJournal,
) -> Result<()> {
	let path = runtime_root.join(RUNTIME_INITIALIZATION_JOURNAL);
	let temp =
		runtime_root.join(format!(".{RUNTIME_INITIALIZATION_JOURNAL}.{}.tmp", std::process::id()));
	if temp.exists() {
		fs::remove_file(&temp)?;
	}
	let mut options = OpenOptions::new();
	options.create_new(true).write(true);
	#[cfg(unix)]
	options.mode(0o600);
	let mut file = options.open(&temp)?;
	file.write_all(toml::to_string(journal)?.as_bytes())?;
	file.sync_all()?;
	drop(file);
	if let Err(error) = fs::rename(&temp, &path) {
		let _ = fs::remove_file(&temp);
		return Err(error.into());
	}
	OpenOptions::new().read(true).open(runtime_root)?.sync_all()?;
	Ok(())
}

fn hex_hash(hash: &[u8; 32]) -> String {
	hash.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn decode_hash(value: &str) -> Result<[u8; 32]> {
	if value.len() != 64 {
		eyre::bail!("runtime_initialization_genesis_hash_invalid");
	}
	let mut hash = [0_u8; 32];
	for (index, byte) in hash.iter_mut().enumerate() {
		*byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
			.map_err(|_| eyre::eyre!("runtime_initialization_genesis_hash_invalid"))?;
	}
	Ok(hash)
}

struct RuntimeInitializationLock(std::fs::File);
impl RuntimeInitializationLock {
	fn acquire(path: &Path) -> Result<Self> {
		let file = OpenOptions::new().create(true).read(true).write(true).open(path)?;
		if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
			return Err(eyre::eyre!("runtime_initialization_lock_unavailable"));
		}
		Ok(Self(file))
	}
}
impl Drop for RuntimeInitializationLock {
	fn drop(&mut self) {
		unsafe {
			libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
		}
	}
}

fn validate_database_relative_path(path: &Path, generation: u64) -> Result<()> {
	if path.is_absolute()
		|| path.components().any(|component| !matches!(component, Component::Normal(_)))
	{
		eyre::bail!("runtime_format_database_path_not_confined");
	}
	let expected = PathBuf::from("generations").join(generation.to_string()).join(LEGACY_DATABASE);
	if path != expected {
		eyre::bail!("runtime_format_database_path_generation_mismatch");
	}
	Ok(())
}

#[cfg(test)]
mod tests {
	use std::fs;

	use tempfile::TempDir;

	use super::*;

	#[test]
	fn lane_authority_v2_c1_selects_only_manifest_generation_database() {
		let temp = TempDir::new().expect("tempdir");
		fs::create_dir_all(temp.path().join("generations/7")).expect("generation dir");
		fs::write(temp.path().join("generations/7/runtime.sqlite3"), b"prepared")
			.expect("database");
		fs::write(
			temp.path().join(RUNTIME_FORMAT_MANIFEST),
			"schema = \"decodex/runtime-format/2\"\ngeneration = 7\ndatabase_relative_path = \"generations/7/runtime.sqlite3\"\n",
		)
		.expect("manifest");
		write_initialization_journal(
			temp.path(),
			&RuntimeInitializationJournal {
				schema: String::from("decodex/runtime-initialization/1"),
				generation: 7,
				genesis_hash: hex_hash(&[1_u8; 32]),
				stage: RuntimeInitializationStage::Complete,
			},
		)
		.expect("journal");
		assert_eq!(
			selected_runtime_db_path_from(temp.path()).expect("selection"),
			temp.path().join("generations/7/runtime.sqlite3")
		);
	}

	#[test]
	fn lane_authority_v2_c1_mig_04() {
		let temp = TempDir::new().expect("tempdir");
		fs::write(temp.path().join(LEGACY_DATABASE), b"legacy").expect("legacy");
		fs::write(
			temp.path().join(RUNTIME_FORMAT_MANIFEST),
			"schema = \"decodex/runtime-format/2\"\ngeneration = 1\ndatabase_relative_path = \"generations/1/runtime.sqlite3\"\n",
		)
		.expect("manifest");
		assert!(
			selected_runtime_db_path_from(temp.path())
				.expect_err("legacy refusal")
				.to_string()
				.contains("legacy_runtime_database")
		);
	}

	#[test]
	fn lane_authority_v2_c1_publishes_prepared_generation_last_and_once() {
		let temp = TempDir::new().expect("tempdir");
		fs::create_dir(temp.path().join(LEGACY_DATABASE)).expect("tombstone");
		fs::create_dir_all(temp.path().join("generations/3")).expect("generation dir");
		let database = temp.path().join("generations/3/runtime.sqlite3");
		fs::write(&database, b"prepared").expect("database");
		assert_eq!(publish_runtime_generation_from(temp.path(), 3).expect("publish"), database);
		assert!(publish_runtime_generation_from(temp.path(), 3).is_err());
		assert!(
			!temp.path().join(format!(".runtime-format.toml.{}.tmp", std::process::id())).exists()
		);
	}

	#[test]
	fn lane_authority_v2_c1_refuses_publish_without_tombstone_or_prepared_database() {
		let temp = TempDir::new().expect("tempdir");
		assert!(publish_runtime_generation_from(temp.path(), 1).is_err());
		fs::create_dir(temp.path().join(LEGACY_DATABASE)).expect("tombstone");
		assert!(publish_runtime_generation_from(temp.path(), 1).is_err());
		assert!(!temp.path().join(RUNTIME_FORMAT_MANIFEST).exists());
	}

	#[test]
	fn lane_authority_v2_c1_rejects_manifest_path_escape_and_generation_drift() {
		for path in ["../runtime.sqlite3", "/tmp/runtime.sqlite3", "generations/2/runtime.sqlite3"]
		{
			let temp = TempDir::new().expect("tempdir");
			fs::write(
				temp.path().join(RUNTIME_FORMAT_MANIFEST),
				format!(
					"schema = \"decodex/runtime-format/2\"\ngeneration = 1\ndatabase_relative_path = \"{path}\"\n"
				),
			)
			.expect("manifest");
			assert!(selected_runtime_db_path_from(temp.path()).is_err(), "accepted {path}");
		}
	}

	#[test]
	fn lane_authority_v2_c1_fresh_initialization_prepares_chain_before_manifest_publish() {
		let temp = TempDir::new().expect("tempdir");
		fs::create_dir(temp.path().join(LEGACY_DATABASE)).expect("tombstone");
		let selected = initialize_fresh_runtime_generation_from(temp.path(), 9, &[8_u8; 32])
			.expect("initialize");
		assert_eq!(selected, temp.path().join("generations/9/runtime.sqlite3"));
		let store = StateStore::open(&selected).expect("store");
		assert!(store.verify_authority_events().expect("chain").is_empty());
		assert_eq!(selected_runtime_generation_from(temp.path()).expect("generation"), 9);
		assert!(initialize_fresh_runtime_generation_from(temp.path(), 10, &[9_u8; 32]).is_err());
		assert!(!temp.path().join("generations/10").exists());
	}

	#[test]
	fn lane_authority_v2_c1_fresh_initialization_requires_exclusive_operator_lock() {
		let temp = TempDir::new().expect("tempdir");
		fs::create_dir(temp.path().join(LEGACY_DATABASE)).expect("tombstone");
		let lock = OpenOptions::new()
			.create(true)
			.read(true)
			.write(true)
			.open(temp.path().join(".runtime-initialize.lock"))
			.expect("lock");
		assert_eq!(unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) }, 0);
		assert!(initialize_fresh_runtime_generation_from(temp.path(), 1, &[1_u8; 32]).is_err());
		unsafe {
			libc::flock(lock.as_raw_fd(), libc::LOCK_UN);
		}
		drop(lock);
		assert!(initialize_fresh_runtime_generation_from(temp.path(), 1, &[1_u8; 32]).is_ok());
	}

	#[test]
	fn lane_authority_v2_c1_mig_05() {
		for stage in [
			RuntimeInitializationStage::Planned,
			RuntimeInitializationStage::DatabasePrepared,
			RuntimeInitializationStage::AnchorInitialized,
			RuntimeInitializationStage::ManifestPublished,
		] {
			let temp = TempDir::new().expect("tempdir");
			fs::create_dir(temp.path().join(LEGACY_DATABASE)).expect("tombstone");
			let genesis = [13_u8; 32];
			write_initialization_journal(
				temp.path(),
				&RuntimeInitializationJournal {
					schema: String::from("decodex/runtime-initialization/1"),
					generation: 6,
					genesis_hash: hex_hash(&genesis),
					stage,
				},
			)
			.expect("journal");
			if stage != RuntimeInitializationStage::Planned {
				fs::create_dir_all(temp.path().join("generations/6")).expect("generation");
				let database = temp.path().join("generations/6/runtime.sqlite3");
				let store = StateStore::open(&database).expect("store");
				store.initialize_authority_generation(6, &genesis).expect("chain");
				drop(store);
			}
			if matches!(
				stage,
				RuntimeInitializationStage::AnchorInitialized
					| RuntimeInitializationStage::ManifestPublished
			) {
				crate::lane_authority::protected_head::AuthorityAnchor::initialize_for_test(
					temp.path(),
					6,
					&genesis,
					&[],
				)
				.expect("anchor");
			}
			if stage == RuntimeInitializationStage::ManifestPublished {
				publish_runtime_generation_from(temp.path(), 6).expect("manifest");
			}

			let selected = initialize_fresh_runtime_generation_from(temp.path(), 6, &[99_u8; 32])
				.expect("resume");
			assert_eq!(selected, temp.path().join("generations/6/runtime.sqlite3"));
			assert_eq!(
				read_initialization_journal(temp.path()).expect("complete").stage,
				RuntimeInitializationStage::Complete
			);
		}
	}
}
