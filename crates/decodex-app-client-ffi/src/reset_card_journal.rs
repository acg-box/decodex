//! Durable native-client intent before the service acknowledges a Reset Card request.
//! The legacy path, JSON schema and fcntl lock are retained for existing installations.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
	collections::{HashMap, HashSet},
	fs::{self, File, OpenOptions},
	io::{self, Read, Write},
	os::{
		fd::AsRawFd,
		unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
	},
	path::{Path, PathBuf},
	sync::{
		Mutex, OnceLock,
		atomic::{AtomicU64, Ordering},
	},
};

const SCHEMA: &str = "decodex/reset-card-pending/2";
const LIMIT: usize = 64;
const BYTES: u64 = 64 * 1024;
static KEYS: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();
static DISPATCHES: OnceLock<Mutex<HashMap<u64, Dispatch>>> = OnceLock::new();
static NEXT: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct Descriptor {
	#[serde(rename = "grantedAtUnixSeconds")]
	granted: i64,
	#[serde(rename = "expiresAtUnixSeconds", skip_serializing_if = "Option::is_none")]
	expires: Option<i64>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Authority {
	#[serde(rename = "profileName")]
	profile: String,
	#[serde(rename = "serverID")]
	server: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Target {
	authority: Authority,
	#[serde(rename = "accountID")]
	account: String,
	#[serde(rename = "expectedRevision")]
	revision: u64,
	descriptor: Descriptor,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Attempt {
	target: Target,
	#[serde(rename = "idempotencyKey")]
	key: String,
}
impl Attempt {
	fn valid(&self) -> bool {
		let t = &self.target;
		!t.authority.profile.is_empty()
			&& t.authority.profile.len() <= 64
			&& t.authority
				.profile
				.bytes()
				.all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
			&& super::is_canonical_uuid(&t.authority.server)
			&& super::is_canonical_uuid(&t.account)
			&& super::is_canonical_uuid(&self.key)
			&& t.revision > 0
			&& t.descriptor.granted >= 0
			&& t.descriptor.expires.is_none_or(|expiry| expiry > t.descriptor.granted)
	}

	fn same_target(&self, other: &Self) -> bool {
		self.target.account == other.target.account
			&& self.target.descriptor == other.target.descriptor
	}
}
#[derive(Serialize, Deserialize)]
struct Document {
	schema: String,
	attempts: Vec<Attempt>,
}
#[derive(Serialize)]
struct Loaded {
	blocked: bool,
	attempts: Vec<Attempt>,
}
impl Loaded {
	fn blocked() -> Self {
		Self { blocked: true, attempts: vec![] }
	}
}
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Observation {
	Completed,
	FailedBeforeEffect,
	Rejected,
	Prepared,
	EffectAmbiguous,
	NotFound,
	Unavailable,
	Unconfirmed,
}
impl Observation {
	fn retires(&self) -> bool {
		matches!(self, Self::Completed | Self::FailedBeforeEffect | Self::Rejected)
	}
}
#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
	Load { path: PathBuf },
	Insert { path: PathBuf, attempt: Attempt },
	Resolve { path: PathBuf, attempt: Attempt, observation: Observation },
	BeginDispatch { path: PathBuf, attempt: Attempt },
	FinishDispatch { lease: u64, observation: Observation },
}
struct Dispatch {
	_lock: Lock,
	path: PathBuf,
	attempt: Attempt,
}

pub(super) fn request(bytes: &[u8]) -> Option<Value> {
	let request: Request = serde_json::from_slice(bytes).ok()?;
	match request {
		Request::Load { path } => serde_json::to_value(load(&path)).ok(),
		Request::Insert { path, attempt } => {
			let _lock = Lock::acquire(&path, true).ok()?;
			let mut current = writable(&path)?;
			if !attempt.valid() {
				return None;
			}
			if let Some(existing) = current.iter().find(|item| item.key == attempt.key) {
				return (existing == &attempt).then(|| json!({"attempts": current}));
			}
			if current.len() >= LIMIT || current.iter().any(|item| item.same_target(&attempt)) {
				return None;
			}
			current.push(attempt);
			persist(&path, &current).ok()?;
			Some(json!({"attempts":current}))
		},
		Request::Resolve { path, attempt, observation } => {
			let _lock = Lock::acquire(&path, true).ok()?;
			let current = writable(&path)?;
			let attempts =
				if observation.retires() { remove(&path, current, &attempt)? } else { current };
			Some(json!({"attempts": attempts}))
		},
		Request::BeginDispatch { path, attempt } => {
			let lock = Lock::acquire(&path, false).ok()?;
			let current = writable(&path)?;
			if !current.contains(&attempt) {
				return None;
			}
			let lease = NEXT.fetch_add(1, Ordering::Relaxed);
			dispatches().lock().ok()?.insert(lease, Dispatch { _lock: lock, path, attempt });
			Some(json!({"lease":lease}))
		},
		Request::FinishDispatch { lease, observation } => {
			let dispatch = dispatches().lock().ok()?.remove(&lease)?;
			let update = if !observation.retires() {
				"retained"
			} else if writable(&dispatch.path)
				.and_then(|current| remove(&dispatch.path, current, &dispatch.attempt))
				.is_some()
			{
				"removed"
			} else {
				"removal_failed"
			};
			Some(json!({"update":update}))
		},
	}
}
fn dispatches() -> &'static Mutex<HashMap<u64, Dispatch>> {
	DISPATCHES.get_or_init(Mutex::default)
}
fn writable(path: &Path) -> Option<Vec<Attempt>> {
	let loaded = load(path);
	(!loaded.blocked).then_some(loaded.attempts)
}
fn remove(path: &Path, mut current: Vec<Attempt>, attempt: &Attempt) -> Option<Vec<Attempt>> {
	if let Some(existing) = current.iter().find(|item| item.key == attempt.key) {
		if existing != attempt {
			return None;
		}
		current.retain(|item| item.key != attempt.key);
		persist(path, &current).ok()?;
	}
	Some(current)
}
fn load(path: &Path) -> Loaded {
	let bytes = match read(path) {
		Ok(Some(bytes)) => bytes,
		Ok(None) => return Loaded { blocked: false, attempts: vec![] },
		Err(_) => return Loaded::blocked(),
	};
	let Ok(document) = serde_json::from_slice::<Document>(&bytes) else {
		return Loaded::blocked();
	};
	let mut seen = HashSet::new();
	let recovered: Vec<_> = document
		.attempts
		.iter()
		.filter(|a| {
			a.valid()
				&& seen.insert(a.key.clone())
				&& document.attempts.iter().filter(|b| b.valid() && a.key == b.key).all(|b| a == &b)
		})
		.take(LIMIT)
		.cloned()
		.collect();
	let unique_targets = document
		.attempts
		.iter()
		.enumerate()
		.all(|(i, a)| !document.attempts[..i].iter().any(|b| a.same_target(b)));
	Loaded {
		blocked: document.schema != SCHEMA || recovered != document.attempts || !unique_targets,
		attempts: recovered,
	}
}
fn denied() -> io::Error {
	io::Error::from(io::ErrorKind::PermissionDenied)
}
fn parent(path: &Path) -> io::Result<&Path> {
	path.parent().filter(|p| !p.as_os_str().is_empty()).ok_or_else(denied)
}
fn private_file(m: &fs::Metadata) -> bool {
	m.is_file()
		&& m.mode() & 0o7777 == 0o600
		&& m.uid() == unsafe { libc::geteuid() }
		&& m.nlink() == 1
}
fn private_dir(m: &fs::Metadata) -> bool {
	m.is_dir() && m.mode() & 0o7777 == 0o700 && m.uid() == unsafe { libc::geteuid() }
}
fn same(a: &fs::Metadata, b: &fs::Metadata) -> bool {
	a.dev() == b.dev() && a.ino() == b.ino()
}
fn open_dir(path: &Path) -> io::Result<File> {
	let before = fs::symlink_metadata(path)?;
	if !private_dir(&before) {
		return Err(denied());
	}
	let file = OpenOptions::new()
		.read(true)
		.custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
		.open(path)?;
	let after = file.metadata()?;
	if !private_dir(&after) || !same(&before, &after) {
		return Err(denied());
	}
	Ok(file)
}
fn ensure_dir(path: &Path) -> io::Result<()> {
	match fs::symlink_metadata(path) {
		Err(e) if e.kind() == io::ErrorKind::NotFound => {
			fs::DirBuilder::new().recursive(true).mode(0o700).create(path)?;
			// Creation mode can be restricted by umask; never change an existing directory.
			let directory = OpenOptions::new()
				.read(true)
				.custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
				.open(path)?;
			directory.set_permissions(fs::Permissions::from_mode(0o700))?;
		},
		Err(e) => return Err(e),
		Ok(_) => {},
	}
	open_dir(path).map(|_| ())
}
fn read(path: &Path) -> io::Result<Option<Vec<u8>>> {
	match open_dir(parent(path)?) {
		Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
		Err(e) => return Err(e),
		Ok(_) => {},
	}
	let before = match fs::symlink_metadata(path) {
		Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
		other => other?,
	};
	if !private_file(&before) || before.len() > BYTES {
		return Err(denied());
	}
	let mut file = OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW).open(path)?;
	let opened = file.metadata()?;
	if !private_file(&opened) || opened.len() > BYTES || !same(&before, &opened) {
		return Err(denied());
	}
	let mut bytes = Vec::with_capacity(opened.len() as usize);
	Read::by_ref(&mut file).take(BYTES + 1).read_to_end(&mut bytes)?;
	let after = file.metadata()?;
	if !private_file(&after)
		|| !same(&opened, &after)
		|| after.len() != opened.len()
		|| bytes.len() as u64 != opened.len()
	{
		return Err(denied());
	}
	Ok(Some(bytes))
}
fn persist(path: &Path, attempts: &[Attempt]) -> io::Result<()> {
	let bytes =
		serde_json::to_vec(&Document { schema: SCHEMA.into(), attempts: attempts.to_vec() })?;
	if bytes.len() as u64 > BYTES {
		return Err(denied());
	}
	let directory = parent(path)?;
	ensure_dir(directory)?;
	let name = path.file_name().ok_or_else(denied)?.to_string_lossy();
	let temporary = directory.join(format!(
		".{name}.{}-{}.tmp",
		std::process::id(),
		NEXT.fetch_add(1, Ordering::Relaxed)
	));
	let mut file = OpenOptions::new()
		.write(true)
		.create_new(true)
		.mode(0o600)
		.custom_flags(libc::O_NOFOLLOW)
		.open(&temporary)?;
	let result = (|| {
		file.set_permissions(fs::Permissions::from_mode(0o600))?;
		if !private_file(&file.metadata()?) {
			return Err(denied());
		}
		file.write_all(&bytes)?;
		// macOS full synchronization retains the previous native journal durability.
		if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_FULLFSYNC) } != 0 {
			file.sync_all()?;
		}
		drop(file);
		fs::rename(&temporary, path)?;
		open_dir(directory)?.sync_all()?;
		if read(path)? != Some(bytes) {
			return Err(denied());
		}
		Ok(())
	})();
	if result.is_err() {
		let _ = fs::remove_file(&temporary);
	}
	result
}
struct Lock {
	file: Option<File>,
	key: PathBuf,
}
impl Lock {
	fn acquire(path: &Path, wait: bool) -> io::Result<Self> {
		let directory = parent(path)?;
		ensure_dir(directory)?;
		let key = fs::canonicalize(directory)?.join(path.file_name().ok_or_else(denied)?);
		if !KEYS.get_or_init(Mutex::default).lock().map_err(|_| denied())?.insert(key.clone()) {
			return Err(denied());
		}
		let mut lock = Self { file: None, key };
		let name = path.file_name().ok_or_else(denied)?.to_string_lossy();
		let lock_path = directory.join(format!(".{name}.lock"));
		let (file, created) = match OpenOptions::new()
			.read(true)
			.write(true)
			.create_new(true)
			.mode(0o600)
			.custom_flags(libc::O_NOFOLLOW)
			.open(&lock_path)
		{
			Ok(file) => (file, true),
			Err(e) if e.kind() == io::ErrorKind::AlreadyExists => (
				OpenOptions::new()
					.read(true)
					.write(true)
					.custom_flags(libc::O_NOFOLLOW)
					.open(&lock_path)?,
				false,
			),
			Err(e) => return Err(e),
		};
		if created {
			file.set_permissions(fs::Permissions::from_mode(0o600))?;
		}
		let opened = file.metadata()?;
		let named = fs::symlink_metadata(lock_path)?;
		if !private_file(&opened) || !private_file(&named) || !same(&opened, &named) {
			return Err(denied());
		}
		set_lock(&file, libc::F_WRLCK, if wait { libc::F_SETLKW } else { libc::F_SETLK })?;
		lock.file = Some(file);
		Ok(lock)
	}
}
fn set_lock(file: &File, kind: i16, command: i32) -> io::Result<()> {
	let mut lock: libc::flock = unsafe { std::mem::zeroed() };
	lock.l_type = kind;
	lock.l_whence = libc::SEEK_SET as _;
	loop {
		if unsafe { libc::fcntl(file.as_raw_fd(), command, &lock) } == 0 {
			return Ok(());
		}
		let error = io::Error::last_os_error();
		if error.kind() != io::ErrorKind::Interrupted {
			return Err(error);
		}
	}
}
impl Drop for Lock {
	fn drop(&mut self) {
		if let Some(file) = self.file.take() {
			let _ = set_lock(&file, libc::F_UNLCK, libc::F_SETLK);
		}
		if let Ok(mut keys) = KEYS.get_or_init(Mutex::default).lock() {
			keys.remove(&self.key);
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	fn attempt() -> Value {
		json!({"target":{"authority":{"profileName":"local","serverID":"aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"},
            "accountID":"11111111-1111-4111-8111-111111111111","expectedRevision":7,
            "descriptor":{"grantedAtUnixSeconds":100}},"idempotencyKey":"22222222-2222-4222-8222-222222222222"})
	}
	fn call(value: Value) -> Option<Value> {
		request(&serde_json::to_vec(&value).unwrap())
	}
	struct Fixture(PathBuf);
	impl Fixture {
		fn new() -> Self {
			let path = std::env::temp_dir().join(format!(
				"decodex-journal-test-{}-{}",
				std::process::id(),
				NEXT.fetch_add(1, Ordering::Relaxed)
			));
			fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
			Self(path)
		}

		fn path(&self) -> PathBuf {
			self.0.join("pending.json")
		}
	}
	impl Drop for Fixture {
		fn drop(&mut self) {
			fs::remove_dir_all(&self.0).unwrap();
		}
	}

	#[test]
	fn legacy_document_preserves_identity_and_conflicting_entries_block_writes() {
		let fixture = Fixture::new();
		let path = fixture.path();
		let original = attempt();
		let mut conflict = original.clone();
		conflict["target"]["expectedRevision"] = 8.into();
		let bytes =
			serde_json::to_vec(&json!({"schema":SCHEMA,"attempts":[original,conflict]})).unwrap();
		fs::write(&path, &bytes).unwrap();
		fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
		assert_eq!(
			call(json!({"operation":"load","path":path})),
			Some(json!({"blocked":true,"attempts":[]}))
		);
		assert!(
			call(
				json!({"operation":"resolve","path":path,"attempt":attempt(),"observation":"completed"})
			)
			.is_none()
		);
		assert_eq!(fs::read(&path).unwrap(), bytes);
		// This is the old Swift format, including a missing optional expiry field.
		fs::write(
			&path,
			serde_json::to_vec(&json!({"schema":SCHEMA,"attempts":[attempt()]})).unwrap(),
		)
		.unwrap();
		assert_eq!(
			call(json!({"operation":"load","path":path})),
			Some(json!({"blocked":false,"attempts":[attempt()]}))
		);
	}

	#[test]
	fn dispatch_lease_blocks_other_owners_and_retains_unknown_results() {
		let fixture = Fixture::new();
		let path = fixture.path();
		assert!(call(json!({"operation":"insert","path":path,"attempt":attempt()})).is_some());
		let begin = || {
			call(json!({"operation":"begin_dispatch","path":path,"attempt":attempt()})).unwrap()["lease"].as_u64().unwrap()
		};
		let lease = begin();
		assert!(
			call(
				json!({"operation":"resolve","path":path,"attempt":attempt(),"observation":"completed"})
			)
			.is_none()
		);
		let child = std::process::Command::new(std::env::current_exe().unwrap())
			.args([
				"--exact",
				"reset_card_journal::tests::other_process_cannot_acquire_dispatch_lock",
				"--nocapture",
			])
			.env("DECODEX_JOURNAL_LOCK_TEST", &path)
			.status()
			.unwrap();
		assert!(child.success());
		assert_eq!(
			call(json!({"operation":"finish_dispatch","lease":lease,"observation":"unconfirmed"})),
			Some(json!({"update":"retained"}))
		);
		assert_eq!(
			call(json!({"operation":"load","path":path})),
			Some(json!({"blocked":false,"attempts":[attempt()]}))
		);
		assert!(
			call(json!({"operation":"finish_dispatch","lease":lease,"observation":"completed"}))
				.is_none()
		);
		let next = begin();
		assert_eq!(
			call(json!({"operation":"finish_dispatch","lease":next,"observation":"completed"})),
			Some(json!({"update":"removed"}))
		);
		assert_eq!(
			call(json!({"operation":"load","path":path})),
			Some(json!({"blocked":false,"attempts":[]}))
		);
	}

	#[test]
	fn dispatch_completion_preserves_a_journal_that_becomes_unwritable() {
		for corrupt in [false, true] {
			let fixture = Fixture::new();
			let path = fixture.path();
			call(json!({"operation":"insert","path":path,"attempt":attempt()})).unwrap();
			let lease = call(json!({"operation":"begin_dispatch","path":path,"attempt":attempt()}))
				.unwrap()["lease"]
				.as_u64()
				.unwrap();
			if corrupt {
				fs::write(&path, b"damaged journal").unwrap();
			} else {
				fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
			}
			let before = fs::read(&path).unwrap();
			let mode = fs::metadata(&path).unwrap().mode();
			assert_eq!(
				call(
					json!({"operation":"finish_dispatch","lease":lease,"observation":"completed"})
				),
				Some(json!({"update":"removal_failed"}))
			);
			assert_eq!(fs::read(&path).unwrap(), before);
			assert_eq!(fs::metadata(&path).unwrap().mode(), mode);
			assert!(Lock::acquire(&path, false).is_ok(), "completion must release its lease");
		}
	}

	#[test]
	fn only_terminal_observations_retire_a_saved_request() {
		for observation in [
			"prepared",
			"effect_ambiguous",
			"not_found",
			"unavailable",
			"unconfirmed",
			"completed",
			"failed_before_effect",
			"rejected",
		] {
			let fixture = Fixture::new();
			let path = fixture.path();
			call(json!({"operation":"insert","path":path,"attempt":attempt()})).unwrap();
			let expected =
				if matches!(observation, "completed" | "failed_before_effect" | "rejected") {
					json!([])
				} else {
					json!([attempt()])
				};
			assert_eq!(
				call(
					json!({"operation":"resolve","path":path,"attempt":attempt(),"observation":observation})
				),
				Some(json!({"attempts":expected}))
			);
		}
	}

	#[test]
	fn other_process_cannot_acquire_dispatch_lock() {
		let Some(path) = std::env::var_os("DECODEX_JOURNAL_LOCK_TEST") else {
			return;
		};
		assert!(Lock::acquire(Path::new(&path), false).is_err());
	}
}
