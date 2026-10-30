//! Download, validate, and atomically publish a versioned runtime.

use std::{
	env,
	fs::{self, File, OpenOptions},
	io::{ErrorKind, Read, Write},
	path::{Path, PathBuf},
	process::Command,
	thread,
	time::Duration,
};

use reqwest::blocking::Response;
use sha2::{Digest as _, Sha256};
use tempfile::Builder;

use crate::{Result, RuntimeLock, development::PREPARE_REQUIRED};
use decodex_codex::schema::GeneratedSchemaEvidence;

/// Current acquisition phase, shared by developer tools and the desktop.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InstallProgress {
	/// Wait for another process installing the same package.
	Waiting,
	/// Download verified bytes. Total is the server's optional content length.
	Downloading {
		/// Number of bytes received so far.
		received: u64,
		/// Total archive length when supplied by the server.
		total: Option<u64>,
	},
	/// Check archive, signatures, executable, version, and protocol evidence.
	Verifying,
	/// The required release is available.
	Ready,
}

pub(crate) enum Archive {
	Remote(String),
	Local(PathBuf),
}

/// Paths for an immutable, verified Codex installation.
#[derive(Clone, Debug)]
pub struct InstalledRuntime {
	/// Package root, including the native bundle and its companion executables.
	pub root: PathBuf,
	/// Native executable. This path retains the upstream signing and resource context.
	pub executable: PathBuf,
	/// Protocol schemas generated from this exact executable.
	pub schemas: PathBuf,
}

/// Shared per-user store. No global current-version pointer exists.
#[derive(Clone, Debug)]
pub struct ComponentStore {
	pub(crate) root: PathBuf,
}
impl ComponentStore {
	/// Use a caller-owned store, for example an isolated test directory.
	pub fn new(root: PathBuf) -> Self {
		Self { root }
	}

	/// Use the user's application-data directory without modifying the app bundle.
	pub fn for_user() -> Result<Self> {
		let home = env::var_os("HOME")
			.filter(|v| !v.is_empty())
			.ok_or("The home directory is unavailable.")?;
		let relative = if cfg!(target_os = "macos") {
			"Library/Application Support/Decodex/components"
		} else {
			".local/share/decodex/components"
		};

		Ok(Self::new(PathBuf::from(home).join(relative)))
	}

	/// Locate this build's runtime without downloading or selecting another version.
	pub fn paths(&self, lock: &RuntimeLock) -> Result<InstalledRuntime> {
		let lock = self.resolve(lock)?.ok_or(PREPARE_REQUIRED)?;

		self.resolved_paths(&lock)
	}

	fn resolved_paths(&self, lock: &RuntimeLock) -> Result<InstalledRuntime> {
		let root = lock.directory(&self.root)?;

		Ok(InstalledRuntime {
			executable: root.join("CodexCLI.app/Contents/MacOS/codex"),
			schemas: root.join("decodex-schemas"),
			root,
		})
	}

	/// Check this platform's receipt, signed executables, and protocol against the pin.
	pub fn installed(&self, lock: &RuntimeLock) -> Result<Option<InstalledRuntime>> {
		lock.validate()?;

		let Some(lock) = self.resolve(lock)? else {
			return Ok(None);
		};

		self.installed_resolved(&lock)
	}

	fn installed_resolved(&self, lock: &RuntimeLock) -> Result<Option<InstalledRuntime>> {
		let paths = self.resolved_paths(lock)?;

		match fs::symlink_metadata(&paths.root) {
			Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
			Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {},
			_ => return Err("The installed Codex directory needs repair.".into()),
		}

		let receipt = RuntimeLock::read(&paths.root.join("decodex-runtime-lock.json"))?;
		let target = lock.target()?.1;

		if receipt.version != lock.version
			|| receipt.upstream_commit != lock.upstream_commit
			|| receipt.source != lock.source
			|| receipt.target()?.1 != target
		{
			return Err("The installed Codex component needs repair.".into());
		}

		// Reject a changed cached executable before invoking even its version command.
		check_evidence(lock, package_evidence(&paths.root)?)?;
		verify_signed_package(lock, &paths.root)?;

		Ok(Some(paths))
	}

	/// Acquire the exact component. Existing versions and running processes are untouched.
	pub fn ensure(
		&self,
		lock: &RuntimeLock,
		progress: impl FnMut(InstallProgress),
	) -> Result<InstalledRuntime> {
		lock.validate()?;

		if !cfg!(target_os = "macos") {
			return Err("This Codex package is available only for macOS.".into());
		}

		let lock = self.resolve(lock)?.ok_or(PREPARE_REQUIRED)?;

		self.ensure_resolved(&lock, progress)
	}

	pub(crate) fn ensure_resolved(
		&self,
		lock: &RuntimeLock,
		mut progress: impl FnMut(InstallProgress),
	) -> Result<InstalledRuntime> {
		let paths = self.resolved_paths(lock)?;
		let parent = paths.root.parent().ok_or("Invalid component directory.")?;

		fs::create_dir_all(parent).map_err(|e| e.to_string())?;

		let lease = OpenOptions::new()
			.create(true)
			.truncate(false)
			.read(true)
			.write(true)
			.open(parent.join("install.lock"))
			.map_err(|e| e.to_string())?;

		progress(InstallProgress::Waiting);

		lease.lock().map_err(|e| e.to_string())?;

		if let Ok(Some(installed)) = self.installed_resolved(lock) {
			progress(InstallProgress::Ready);

			return Ok(installed);
		}

		let stage =
			Builder::new().prefix(".download-").tempdir_in(parent).map_err(|e| e.to_string())?;
		let package = stage.path().join("package");
		let target = lock.target()?.1;
		let input = if lock.source.is_some() {
			Archive::Local(
				self.development_directory(lock)?.join(format!("{}.tar.gz", target.sha256)),
			)
		} else {
			Archive::Remote(target.url.clone())
		};
		let evidence = prepare_package(lock, &input, &target.sha256, stage.path(), &mut progress)?;

		check_evidence(lock, evidence)?;

		fs::write(
			package.join("decodex-runtime-lock.json"),
			serde_json::to_vec_pretty(&lock).map_err(|e| e.to_string())?,
		)
		.map_err(|e| e.to_string())?;

		publish_package(&package, &paths.root, &stage.path().join("replaced"))?;
		progress(InstallProgress::Ready);

		Ok(paths)
	}
}

pub(crate) fn prepare_package(
	lock: &RuntimeLock,
	input: &Archive,
	sha256: &str,
	stage: &Path,
	progress: &mut impl FnMut(InstallProgress),
) -> Result<(String, String)> {
	let archive = stage.join("package.tar.gz");

	match input {
		Archive::Remote(url) => download(url, sha256, &archive, progress)?,
		Archive::Local(path) => {
			fs::copy(path, &archive).map_err(|e| format!("Cannot read the prepared development archive: {e}. Run cargo make prepare-codex to rebuild it."))?;

			if digest(&archive)? != sha256 {
				return Err("The development archive needs repair. Run cargo make prepare-codex to rebuild it.".into());
			}
		},
	}

	progress(InstallProgress::Verifying);

	let package = stage.join("package");

	fs::create_dir(&package).map_err(|e| e.to_string())?;

	// The archive has passed the exact hash pinned by the trusted Decodex build.
	// macOS bsdtar also rejects absolute paths and parent traversal by default.
	run(Command::new("/usr/bin/tar")
		.args(["-xzf"])
		.arg(&archive)
		.arg("-C")
		.arg(&package)
		.args(["--no-same-owner", "--no-same-permissions"]))?;
	verify_signed_package(lock, &package)?;

	let schemas = package.join("decodex-schemas");

	if schemas.exists() {
		fs::remove_dir_all(&schemas).map_err(|e| e.to_string())?;
	}

	run(Command::new(package.join("CodexCLI.app/Contents/MacOS/codex"))
		.args(["app-server", "generate-json-schema", "--experimental", "--out"])
		.arg(package.join("decodex-schemas")))?;

	package_evidence(&package)
}

pub(crate) fn download(
	url: &str,
	sha256: &str,
	output: &Path,
	progress: &mut impl FnMut(InstallProgress),
) -> Result<()> {
	let client = reqwest::blocking::Client::builder()
		.https_only(true)
		.connect_timeout(Duration::from_secs(20))
		// Blocking responses apply this timeout to each streamed read. A stalled
		// connection must return control to Retry without limiting the whole download.
		.timeout(Duration::from_secs(30))
		.user_agent("Decodex component installer")
		.build()
		.map_err(|e| e.to_string())?;

	download_retry(&client, url, sha256, output, progress)
}

pub(crate) fn package_evidence(root: &Path) -> Result<(String, String)> {
	let schemas = GeneratedSchemaEvidence::load(&root.join("decodex-schemas"))
		.map_err(|missing| format!("The Codex protocol is incompatible: {}", missing.join(", ")))?;

	Ok((digest(&root.join("CodexCLI.app/Contents/MacOS/codex"))?, schemas.fingerprint))
}

pub(crate) fn digest(path: &Path) -> Result<String> {
	let mut file = File::open(path).map_err(|e| e.to_string())?;
	let mut hash = Sha256::new();
	let mut buffer = [0_u8; 128 * 1_024];

	loop {
		let length = file.read(&mut buffer).map_err(|e| e.to_string())?;

		if length == 0 {
			break;
		}

		hash.update(&buffer[..length]);
	}

	Ok(hash_text(hash.finalize().as_slice()))
}

pub(crate) fn hash_text(bytes: &[u8]) -> String {
	bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub(crate) fn run(command: &mut Command) -> Result<String> {
	let output = command.output().map_err(|e| e.to_string())?;

	if !output.status.success() {
		return Err(format!(
			"Component verification failed: {}",
			String::from_utf8_lossy(&output.stderr)
		));
	}

	String::from_utf8(output.stdout).map_err(|e| e.to_string())
}

fn publish_package(package: &Path, destination: &Path, replaced: &Path) -> Result<()> {
	// Keep a damaged installation until its replacement is fully verified. Moving it
	// inside the temporary directory also handles a file or symlink at the cache path.
	let had_previous = match fs::rename(destination, replaced) {
		Ok(()) => true,
		Err(error) if error.kind() == ErrorKind::NotFound => false,
		Err(error) => return Err(error.to_string()),
	};

	if let Err(error) = fs::rename(package, destination) {
		if had_previous {
			fs::rename(replaced, destination).map_err(|restore| {
				format!("Cannot publish Codex: {error}; cannot restore: {restore}")
			})?;
		}

		return Err(error.to_string());
	}

	Ok(())
}

fn download_retry(
	client: &reqwest::blocking::Client,
	url: &str,
	sha256: &str,
	output: &Path,
	progress: &mut impl FnMut(InstallProgress),
) -> Result<()> {
	let mut last = String::new();

	for attempt in 0..3 {
		match download_with(client, url, sha256, output, progress) {
			Ok(()) => return Ok(()),
			Err(error) => last = error,
		}

		let _ = fs::remove_file(output);

		if attempt < 2 {
			thread::sleep(Duration::from_millis(250 * (attempt + 1)));
		}
	}

	Err(format!("Codex download failed after 3 attempts: {last}"))
}

fn download_with(
	client: &reqwest::blocking::Client,
	url: &str,
	sha256: &str,
	output: &Path,
	progress: &mut impl FnMut(InstallProgress),
) -> Result<()> {
	let mut response =
		client.get(url).send().and_then(Response::error_for_status).map_err(|e| e.to_string())?;
	let total = response.content_length();
	let mut file = File::create(output).map_err(|e| e.to_string())?;
	let mut hasher = Sha256::new();
	let mut received = 0;
	let mut buffer = [0_u8; 128 * 1_024];

	loop {
		let length = response.read(&mut buffer).map_err(|e| e.to_string())?;

		if length == 0 {
			break;
		}

		received += length as u64;

		if received > 1_024 * 1_024 * 1_024 {
			return Err("The Codex archive exceeds the download limit.".into());
		}

		file.write_all(&buffer[..length]).map_err(|e| e.to_string())?;
		hasher.update(&buffer[..length]);

		progress(InstallProgress::Downloading { received, total });
	}

	file.sync_all().map_err(|e| e.to_string())?;

	if hash_text(hasher.finalize().as_slice()) != sha256 {
		return Err("The Codex archive does not match its pinned SHA-256.".into());
	}

	Ok(())
}

fn packaged_executor(root: &Path) -> Result<Option<PathBuf>> {
	let metadata: serde_json::Value = serde_json::from_slice(
		&fs::read(root.join("codex-package.json")).map_err(|e| e.to_string())?,
	)
	.map_err(|e| e.to_string())?;

	match metadata.get("execServer") {
		None => Ok(None),
		Some(value) if value.as_str() == Some("bin/codex-exec-server") => {
			let path = root.join("bin/codex-exec-server");

			if !path.is_file() {
				return Err("The Codex package is missing its advertised executor.".into());
			}

			Ok(Some(path))
		},
		Some(_) => Err("The Codex package advertises an unsupported executor path.".into()),
	}
}

fn verify_signed_package(lock: &RuntimeLock, root: &Path) -> Result<()> {
	let native = root.join("CodexCLI.app/Contents/MacOS/codex");
	let executor = packaged_executor(root)?;

	if !native
		.canonicalize()
		.map_err(|e| e.to_string())?
		.starts_with(root.canonicalize().map_err(|e| e.to_string())?)
	{
		return Err("The native Codex executable does not match its pin.".into());
	}

	let mut signed_paths = vec!["CodexCLI.app", "bin/codex-code-mode-host", "codex-path/rg"];

	if executor.is_some() {
		signed_paths.push("bin/codex-exec-server");
	}

	if root.join("codex-resources/zsh/bin/zsh").exists() {
		signed_paths.push("codex-resources/zsh/bin/zsh");
	}

	// These exact package members are independent. Retain every check under this
	// scope and join all workers before executing the pinned version command.
	thread::scope(|scope| {
		let checks: Vec<_> = signed_paths
			.into_iter()
			.map(|relative| {
				scope.spawn(move || {
					run(Command::new("/usr/bin/codesign")
						.args(["--verify", "--strict", "--deep"])
						.arg(root.join(relative)))
				})
			})
			.collect();

		for check in checks {
			check
				.join()
				.map_err(|_| "The component signature check did not complete.".to_owned())??;
		}

		Ok::<_, String>(())
	})?;

	if lock.source.is_some() {
		let commit = lock.upstream_commit.as_bytes();

		let code_mode_host = root.join("bin/codex-code-mode-host");

		for binary in [&native, &code_mode_host].into_iter().chain(executor.iter()) {
			let bytes = fs::read(binary).map_err(|e| e.to_string())?;

			if !bytes.windows(commit.len()).any(|part| part[0] == commit[0] && part == commit) {
				return Err(
					"The development executable was not built from the pinned commit.".into()
				);
			}
		}
	}

	let reported = run(Command::new(&native).arg("--version"))?;

	if reported.trim() != format!("codex-cli {}", lock.version) {
		return Err("The downloaded Codex reports another version.".into());
	}

	Ok(())
}

fn check_evidence(lock: &RuntimeLock, evidence: (String, String)) -> Result<()> {
	let target = lock.target()?.1;

	if evidence != (target.executable_sha256.clone(), target.schema_sha256.clone()) {
		return Err("The Codex executable or protocol schema does not match its pin.".into());
	}

	Ok(())
}

#[cfg(test)]
mod tests {
	use std::{net::TcpListener, thread};

	use sha2::Digest as _;

	use crate::{
		install::{self, ComponentStore, Duration, Read as _, Sha256, Write as _, fs},
		tests,
	};

	#[test]
	fn executor_metadata_preserves_legacy_packages_and_requires_advertised_helpers() {
		let temporary = tempfile::tempdir().expect("package");
		let root = temporary.path();
		let metadata = root.join("codex-package.json");

		fs::write(&metadata, r#"{"layoutVersion":1}"#).expect("legacy metadata");
		assert_eq!(install::packaged_executor(root).expect("legacy package"), None);

		fs::write(&metadata, r#"{"execServer":"bin/codex-exec-server"}"#)
			.expect("executor metadata");
		assert!(install::packaged_executor(root).expect_err("missing helper").contains("missing"));

		fs::create_dir(root.join("bin")).expect("binaries");
		let executor = root.join("bin/codex-exec-server");

		fs::write(&executor, "executor").expect("helper");
		assert_eq!(install::packaged_executor(root).expect("new package"), Some(executor));

		for value in [serde_json::json!("../executor"), serde_json::Value::Null] {
			fs::write(
				&metadata,
				serde_json::to_vec(&serde_json::json!({"execServer":value})).expect("JSON"),
			)
			.expect("invalid metadata");
			assert!(
				install::packaged_executor(root).expect_err("invalid path").contains("unsupported")
			);
		}
	}

	#[test]
	fn failed_publish_restores_the_previous_installation() {
		let temporary = tempfile::tempdir().expect("store");
		let destination = temporary.path().join("installed");
		let replaced = temporary.path().join("replaced");

		fs::create_dir(&destination).expect("cache");
		fs::write(destination.join("existing"), "preserve").expect("existing runtime");

		assert!(
			install::publish_package(&temporary.path().join("missing"), &destination, &replaced)
				.is_err()
		);
		assert_eq!(fs::read_to_string(destination.join("existing")).expect("restored"), "preserve");
		assert!(!replaced.exists());
	}

	#[test]
	fn verified_replacement_repairs_a_directory_or_file_without_touching_other_versions() {
		let temporary = tempfile::tempdir().expect("store");
		let previous = temporary.path().join("other-version");

		fs::write(&previous, "older runtime").expect("old version");

		for is_directory in [false, true] {
			let destination = temporary.path().join("installed");
			let stage = tempfile::tempdir_in(temporary.path()).expect("stage");
			let package = stage.path().join("package");

			fs::create_dir(&package).expect("verified package");
			fs::write(package.join("ready"), "verified").expect("contents");

			if is_directory {
				fs::create_dir(&destination).expect("damaged directory");
			} else {
				fs::write(&destination, "damaged file").expect("damaged file");
			}

			install::publish_package(&package, &destination, &stage.path().join("replaced"))
				.expect("repair");

			assert_eq!(
				fs::read_to_string(destination.join("ready")).expect("repaired"),
				"verified"
			);
			assert_eq!(fs::read_to_string(&previous).expect("other version"), "older runtime");

			fs::remove_dir_all(destination).expect("reset fixture");
		}
	}

	fn serve(responses: Vec<String>) -> (String, thread::JoinHandle<()>) {
		let listener = TcpListener::bind("127.0.0.1:0").expect("HTTP fixture");
		let address = listener.local_addr().expect("fixture address");
		let worker = thread::spawn(move || {
			for response in responses {
				let (mut connection, _) = listener.accept().expect("request");

				connection.set_read_timeout(Some(Duration::from_secs(5))).expect("timeout");

				let mut request = [0; 4_096];

				assert!(connection.read(&mut request).expect("request headers") > 0);

				connection.write_all(response.as_bytes()).expect("response");
			}
		});

		(format!("http://{address}/codex.tar.gz"), worker)
	}

	#[test]
	fn interrupted_download_is_discarded_and_retry_reads_a_complete_new_response() {
		let temporary = tempfile::tempdir().expect("store");
		let body = "verified package";
		let expected = install::hash_text(Sha256::digest(body.as_bytes()).as_slice());
		let (url, worker) = serve(vec![
			"HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\npartial".into(),
			format!(
				"HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
				body.len()
			),
		]);
		let client = reqwest::blocking::Client::builder()
			.no_proxy()
			.timeout(Duration::from_secs(5))
			.build()
			.expect("client");
		let output = temporary.path().join("package");

		install::download_retry(&client, &url, &expected, &output, &mut |_| {})
			.expect("automatic retry succeeds");

		assert_eq!(fs::read_to_string(output).expect("complete download"), body);

		worker.join().expect("HTTP fixture");
	}

	#[test]
	fn exhausted_retries_leave_no_partial_archive() {
		let temporary = tempfile::tempdir().expect("store");
		let response =
			"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
		let (url, worker) = serve(vec![response.into(), response.into(), response.into()]);
		let client = reqwest::blocking::Client::builder().no_proxy().build().expect("client");
		let output = temporary.path().join("package");

		assert!(
			install::download_retry(&client, &url, &"a".repeat(64), &output, &mut |_| {})
				.expect_err("unavailable")
				.contains("3 attempts")
		);
		assert!(!output.exists());

		worker.join().expect("server");
	}

	#[test]
	fn a_complete_response_with_the_wrong_hash_is_rejected() {
		let temporary = tempfile::tempdir().expect("store");
		let (url, worker) = serve(vec![
			"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nwrong".into(),
		]);
		let client = reqwest::blocking::Client::builder().no_proxy().build().expect("client");
		let result = install::download_with(
			&client,
			&url,
			&"0".repeat(64),
			&temporary.path().join("package"),
			&mut |_| {},
		);

		assert!(result.expect_err("hash mismatch").contains("SHA-256"));

		worker.join().expect("HTTP fixture");
	}

	#[test]
	fn corrupt_cached_executable_is_rejected_without_touching_previous_versions() {
		let temporary = tempfile::tempdir().expect("component store");
		let store = ComponentStore::new(temporary.path().to_owned());
		let mut lock = tests::release();

		if !cfg!(target_arch = "aarch64") {
			let mut target = lock.targets.remove("aarch64-apple-darwin").expect("fixture target");

			target.url = target.url.replace("aarch64-apple-darwin", "x86_64-apple-darwin");

			lock.targets.insert("x86_64-apple-darwin".into(), target);
		}

		let bytes = b"native image fixture";

		lock.targets.values_mut().next().expect("platform").executable_sha256 =
			install::hash_text(Sha256::digest(bytes).as_slice());

		let paths = store.paths(&lock).expect("versioned paths");

		fs::create_dir_all(paths.executable.parent().expect("bundle parent")).expect("bundle");
		fs::create_dir_all(&paths.schemas).expect("schemas");
		fs::write(paths.schemas.join("ClientRequest.json"), "{}").expect("schema fixture");
		fs::write(&paths.executable, bytes).expect("native fixture");
		fs::write(
			paths.root.join("decodex-runtime-lock.json"),
			serde_json::to_vec(&lock).expect("lock JSON"),
		)
		.expect("receipt");

		assert!(store.installed(&lock).is_err());

		let mut next = lock.clone();

		next.targets.values_mut().next().expect("platform").sha256 = "f".repeat(64);

		assert!(store.installed(&next).expect("new version").is_none());

		fs::write(&paths.executable, b"changed executable").expect("tamper fixture");

		assert!(store.installed(&lock).is_err());
		assert_eq!(
			fs::read(&paths.executable).expect("previous version preserved"),
			b"changed executable"
		);
	}

	#[test]
	fn an_invalid_requirement_never_creates_an_installation() {
		let temporary = tempfile::tempdir().expect("component store");
		let root = temporary.path().join("components");
		let store = ComponentStore::new(root.clone());
		let mut lock = tests::release();

		lock.targets.values_mut().next().expect("platform").url =
			"https://example.com/runtime.tar.gz".into();

		assert!(store.ensure(&lock, |_| {}).is_err());
		assert!(!root.exists());
	}
}
