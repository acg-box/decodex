//! Atomic, crash-recoverable single-browser lease for X publication.

#[cfg(unix)] use std::os::unix::fs::MetadataExt as _;
use std::{
	fs::{self, File, OpenOptions},
	io::{ErrorKind, Write as _},
	path::Path,
	time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::{
	SocialBrowserLeaseReport,
	prelude::{Result, eyre},
};

const LEASE_SCHEMA: &str = "social_browser_lease/v1";
const LEASE_DIRECTORY: &str = "x-browser-active";
const LEASE_FILE: &str = "lease.json";
const MUTATION_FILE: &str = ".x-browser-mutation.lock";
const MINIMUM_TTL_SECONDS: u64 = 300;
const MAXIMUM_TTL_SECONDS: u64 = 7_200;
const INCOMPLETE_LEASE_GRACE_SECONDS: u64 = 60;

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct BrowserLeaseRecord {
	schema: String,
	lease_token: String,
	acquired_at_epoch_seconds: u64,
	expires_at_epoch_seconds: u64,
}

pub(crate) fn acquire(out_dir: &Path, ttl_seconds: u64) -> Result<SocialBrowserLeaseReport> {
	validate_ttl(ttl_seconds)?;

	let root = crate::repo_root()?;
	let out_dir = crate::resolve_against(&root, out_dir);
	fs::create_dir_all(&out_dir)?;
	let _mutation_lock = acquire_mutation_lock(&out_dir)?;
	let lease_dir = out_dir.join(LEASE_DIRECTORY);

	for _ in 0..3 {
		let now = now_epoch_seconds()?;
		let lease_token = random_token()?;

		match fs::create_dir(&lease_dir) {
			Ok(()) => {
				let record = BrowserLeaseRecord {
					schema: LEASE_SCHEMA.into(),
					lease_token: lease_token.clone(),
					acquired_at_epoch_seconds: now,
					expires_at_epoch_seconds: now + ttl_seconds,
				};
				let payload = serde_json::to_value(&record)?;

				if let Err(error) = crate::write_new_json(&lease_dir.join(LEASE_FILE), &payload) {
					let _ = fs::remove_dir_all(&lease_dir);

					return Err(error);
				}

				return Ok(report(
					&root,
					&lease_dir,
					"acquired",
					Some(lease_token),
					Some(record.expires_at_epoch_seconds),
				));
			},
			Err(error) if error.kind() == ErrorKind::AlreadyExists => {
				if !reclaim_if_stale(&lease_dir, &out_dir, now, &lease_token)? {
					return Err(eyre::eyre!("X browser lease is already active"));
				}
			},
			Err(error) => return Err(error.into()),
		}
	}

	Err(eyre::eyre!("X browser lease acquisition lost a concurrent race"))
}

pub(crate) fn verify(out_dir: &Path, lease_token: &str) -> Result<SocialBrowserLeaseReport> {
	let root = crate::repo_root()?;
	let out_dir = crate::resolve_against(&root, out_dir);
	let _mutation_lock = acquire_mutation_lock(&out_dir)?;
	let lease_dir = out_dir.join(LEASE_DIRECTORY);
	let record = load_record(&lease_dir)?;

	if record.lease_token != lease_token {
		return Err(eyre::eyre!("X browser lease token does not match the active lease"));
	}
	if record.expires_at_epoch_seconds <= now_epoch_seconds()? {
		return Err(eyre::eyre!("X browser lease has expired"));
	}

	Ok(report(&root, &lease_dir, "verified", None, Some(record.expires_at_epoch_seconds)))
}

pub(crate) fn renew(
	out_dir: &Path,
	lease_token: &str,
	ttl_seconds: u64,
) -> Result<SocialBrowserLeaseReport> {
	validate_ttl(ttl_seconds)?;
	let root = crate::repo_root()?;
	let out_dir = crate::resolve_against(&root, out_dir);
	let _mutation_lock = acquire_mutation_lock(&out_dir)?;
	let lease_dir = out_dir.join(LEASE_DIRECTORY);
	let now = now_epoch_seconds()?;
	let mut record = load_record(&lease_dir)?;

	if record.lease_token != lease_token {
		return Err(eyre::eyre!("X browser lease token does not match the active lease"));
	}
	if record.expires_at_epoch_seconds <= now {
		return Err(eyre::eyre!("X browser lease has expired"));
	}

	record.expires_at_epoch_seconds = now
		.checked_add(ttl_seconds)
		.ok_or_else(|| eyre::eyre!("X browser lease expiry overflowed"))?;
	replace_record(&lease_dir, &record)?;

	Ok(report(&root, &lease_dir, "renewed", None, Some(record.expires_at_epoch_seconds)))
}

pub(crate) fn release(out_dir: &Path, lease_token: &str) -> Result<SocialBrowserLeaseReport> {
	let root = crate::repo_root()?;
	let out_dir = crate::resolve_against(&root, out_dir);
	let _mutation_lock = acquire_mutation_lock(&out_dir)?;
	let lease_dir = out_dir.join(LEASE_DIRECTORY);
	let record = load_record(&lease_dir)?;

	if record.lease_token != lease_token {
		return Err(eyre::eyre!("X browser lease token does not match the active lease"));
	}

	let released_dir = out_dir.join(format!(".x-browser-released-{}", random_token()?));
	fs::rename(&lease_dir, &released_dir)?;
	fs::remove_dir_all(&released_dir)?;

	Ok(report(&root, &lease_dir, "released", None, None))
}

fn reclaim_if_stale(
	lease_dir: &Path,
	out_dir: &Path,
	now: u64,
	reclaimer_token: &str,
) -> Result<bool> {
	let incomplete_age = directory_age_seconds(lease_dir, now)?;
	let stale = match load_record(lease_dir) {
		Ok(record) => record.expires_at_epoch_seconds <= now,
		Err(_) => incomplete_age.is_some_and(|age| age >= INCOMPLETE_LEASE_GRACE_SECONDS),
	};

	if !stale {
		return Ok(false);
	}

	let reclaimed_dir = out_dir.join(format!(".x-browser-reclaimed-{reclaimer_token}"));

	match fs::rename(lease_dir, &reclaimed_dir) {
		Ok(()) => {
			fs::remove_dir_all(reclaimed_dir)?;

			Ok(true)
		},
		Err(error) if error.kind() == ErrorKind::NotFound => Ok(true),
		Err(error) => Err(error.into()),
	}
}

fn acquire_mutation_lock(out_dir: &Path) -> Result<File> {
	fs::create_dir_all(out_dir)?;
	let path = out_dir.join(MUTATION_FILE);

	if path.exists() && fs::symlink_metadata(&path)?.file_type().is_symlink() {
		return Err(eyre::eyre!("X browser mutation lock file must not be a symlink"));
	}
	let file =
		OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&path)?;
	let path_metadata = fs::symlink_metadata(&path)?;

	if path_metadata.file_type().is_symlink() || !path_metadata.is_file() {
		return Err(eyre::eyre!("X browser mutation lock path must be a regular file"));
	}
	#[cfg(unix)]
	{
		let file_metadata = file.metadata()?;

		if file_metadata.dev() != path_metadata.dev() || file_metadata.ino() != path_metadata.ino()
		{
			return Err(eyre::eyre!("X browser mutation lock path changed during open"));
		}
	}
	file.try_lock().map_err(|error| {
		eyre::eyre!("X browser lease mutation is already active or unavailable: {error}")
	})?;

	Ok(file)
}

fn load_record(lease_dir: &Path) -> Result<BrowserLeaseRecord> {
	if fs::symlink_metadata(lease_dir)?.file_type().is_symlink() {
		return Err(eyre::eyre!("X browser lease directory must not be a symlink"));
	}
	let lease_file = lease_dir.join(LEASE_FILE);

	if fs::symlink_metadata(&lease_file)?.file_type().is_symlink() {
		return Err(eyre::eyre!("X browser lease file must not be a symlink"));
	}
	let record: BrowserLeaseRecord = serde_json::from_value(crate::load_json(&lease_file)?)?;

	if record.schema != LEASE_SCHEMA
		|| record.acquired_at_epoch_seconds >= record.expires_at_epoch_seconds
	{
		return Err(eyre::eyre!("X browser lease record is invalid"));
	}

	Ok(record)
}

fn replace_record(lease_dir: &Path, record: &BrowserLeaseRecord) -> Result<()> {
	let temporary = lease_dir.join(format!(".lease-renewal-{}.json", random_token()?));
	let mut file = OpenOptions::new().write(true).create_new(true).open(&temporary)?;
	let payload = serde_json::to_string_pretty(record)?;

	file.write_all(payload.as_bytes())?;
	file.write_all(b"\n")?;
	file.sync_all()?;
	drop(file);

	if let Err(error) = fs::rename(&temporary, lease_dir.join(LEASE_FILE)) {
		let _ = fs::remove_file(&temporary);

		return Err(error.into());
	}

	Ok(())
}

fn validate_ttl(ttl_seconds: u64) -> Result<()> {
	if !(MINIMUM_TTL_SECONDS..=MAXIMUM_TTL_SECONDS).contains(&ttl_seconds) {
		return Err(eyre::eyre!(
			"browser lease ttl must be between {MINIMUM_TTL_SECONDS} and {MAXIMUM_TTL_SECONDS} seconds"
		));
	}

	Ok(())
}

fn directory_age_seconds(path: &Path, now: u64) -> Result<Option<u64>> {
	let modified = fs::symlink_metadata(path)?.modified()?;
	let modified = modified.duration_since(UNIX_EPOCH).unwrap_or(Duration::ZERO).as_secs();

	Ok(now.checked_sub(modified))
}

fn now_epoch_seconds() -> Result<u64> {
	Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())
}

fn random_token() -> Result<String> {
	let mut bytes = [0_u8; 32];
	getrandom::fill(&mut bytes).map_err(|_| eyre::eyre!("secure randomness is unavailable"))?;

	Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn report(
	root: &Path,
	path: &Path,
	status: &str,
	lease_token: Option<String>,
	expires_at_epoch_seconds: Option<u64>,
) -> SocialBrowserLeaseReport {
	SocialBrowserLeaseReport {
		status: status.into(),
		path: crate::path_arg(root, path),
		lease_token,
		expires_at_epoch_seconds,
	}
}

#[cfg(test)]
mod lease_tests {
	use std::thread;

	#[test]
	fn operating_system_lock_serializes_mutation_and_releases_on_drop() {
		let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
		let out_dir = temp_dir.path().to_path_buf();
		let first =
			super::acquire_mutation_lock(&out_dir).expect("first mutation lock should be acquired");
		let contender_dir = out_dir.clone();
		let contender = thread::spawn(move || super::acquire_mutation_lock(&contender_dir));
		let error = contender
			.join()
			.expect("contender thread should finish")
			.expect_err("concurrent mutation lock should fail")
			.to_string();

		assert!(error.contains("already active or unavailable"));
		drop(first);
		super::acquire_mutation_lock(&out_dir)
			.expect("mutation lock should be reusable after owner drop");
	}
}
