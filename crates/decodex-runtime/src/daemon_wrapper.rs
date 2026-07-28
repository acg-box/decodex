use std::{
	collections::{BTreeMap, BTreeSet, HashSet},
	env,
	ffi::OsString,
	fmt,
	fs::{self, OpenOptions},
	io::{Read, Write},
	os::unix::fs::{MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _},
	path::{Path, PathBuf},
	process::{Command, Stdio},
	sync::{
		Arc,
		atomic::{AtomicUsize, Ordering},
		mpsc::{self, Receiver, TryRecvError},
	},
	thread::{self, JoinHandle},
	time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest as _, Sha256};

const DESCRIPTOR_SCHEMA: &str = "decodex/daemon-wrapper/1";
const WRAPPER_NAME: &str = "decodexd.app";
const BUNDLE_IDENTIFIER: &str = "box.acg.decodex.daemon";
const BUNDLE_EXECUTABLE: &str = "decodexd";
const BUNDLE_PACKAGE_TYPE: &str = "APPL";
const TEAM_IDENTIFIER: &str = "T54QFA7W2S";
const APPLICATION_IDENTIFIER: &str = "T54QFA7W2S.box.acg.decodex.daemon";
const PROFILE_ACCESS_GROUP: &str = "T54QFA7W2S.*";
const PROFILE_CHANNEL: &str = "development";

const CODESIGN_PATH: &str = "/usr/bin/codesign";
const SECURITY_PATH: &str = "/usr/bin/security";
const PLUTIL_PATH: &str = "/usr/bin/plutil";
const TOOL_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_TOOL_OUTPUT_BYTES: usize = 2 * 1_024 * 1_024;
const MAX_EXECUTABLE_BYTES: usize = 256 * 1_024 * 1_024;
const MAX_PROFILE_BYTES: usize = 4 * 1_024 * 1_024;
const MAX_PLIST_BYTES: usize = 128 * 1_024;
const MAX_CERTIFICATE_BYTES: usize = 128 * 1_024;
const MAX_CERTIFICATE_SET_BYTES: usize = 512 * 1_024;
const MAX_CERTIFICATE_COUNT: usize = 16;
const MACHO_MAGICS: [[u8; 4]; 8] = [
	[0xfe, 0xed, 0xfa, 0xce],
	[0xce, 0xfa, 0xed, 0xfe],
	[0xfe, 0xed, 0xfa, 0xcf],
	[0xcf, 0xfa, 0xed, 0xfe],
	[0xca, 0xfe, 0xba, 0xbe],
	[0xbe, 0xba, 0xfe, 0xca],
	[0xca, 0xfe, 0xba, 0xbf],
	[0xbf, 0xba, 0xfe, 0xca],
];

const INFO_PLIST_AUTHORITY: &[u8] = include_bytes!("../../../apps/decodexd/packaging/Info.plist");

/// Fixed non-secret identity of the current signed `decodexd` wrapper.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DaemonWrapperDescriptor {
	schema: String,
	wrapper_path: String,
	executable_path: String,
	executable_sha256: String,
	executable_byte_count: u64,
	info_plist_path: String,
	info_plist_sha256: String,
	bundle_identifier: String,
	bundle_executable: String,
	bundle_package_type: String,
	background_only: bool,
	embedded_profile_path: String,
	embedded_profile_sha256: String,
	team_identifier: String,
	application_identifier: String,
	profile_expires_at: String,
	profile_channel: String,
	signed_entitlements_sha256: String,
	keychain_access_groups: Vec<String>,
	signature_identity_sha256: String,
}

impl DaemonWrapperDescriptor {
	/// Return the canonical absolute wrapper path.
	pub fn wrapper_path(&self) -> &str {
		&self.wrapper_path
	}

	/// Return the canonical absolute wrapper-main path.
	pub fn executable_path(&self) -> &str {
		&self.executable_path
	}

	/// Return the verified wrapper-main SHA-256.
	pub(crate) fn executable_sha256(&self) -> &str {
		&self.executable_sha256
	}

	/// Return the verified wrapper-main byte count.
	pub(crate) fn executable_byte_count(&self) -> u64 {
		self.executable_byte_count
	}

	/// Return the one verified daemon Keychain access group.
	pub(crate) fn keychain_access_group(&self) -> &str {
		self.keychain_access_groups.first().map_or("", String::as_str)
	}
}

/// Non-secret refusal from the fixed daemon-wrapper inspector.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DaemonWrapperError;

impl fmt::Display for DaemonWrapperError {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		formatter.write_str("daemon wrapper identity is unavailable")
	}
}

impl std::error::Error for DaemonWrapperError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ToolFailure {
	Spawn,
	Pipes,
	Input,
	Timeout,
	Output,
	Status,
	Cleanup,
}

struct ToolOutput {
	stdout: Vec<u8>,
	stderr: Vec<u8>,
}

fn tool_error(_failure: ToolFailure) -> DaemonWrapperError {
	DaemonWrapperError
}

fn spawn_reader<R: Read + Send + 'static>(
	mut reader: R,
	total: Arc<AtomicUsize>,
) -> (Receiver<Result<Vec<u8>, ToolFailure>>, JoinHandle<()>) {
	let (sender, receiver) = mpsc::channel();
	let handle = thread::spawn(move || {
		let mut body = Vec::new();
		let mut chunk = [0_u8; 64 * 1_024];
		loop {
			let read = match reader.read(&mut chunk) {
				Ok(read) => read,
				Err(_) => {
					let _ = sender.send(Err(ToolFailure::Pipes));
					return;
				},
			};
			if read == 0 {
				let _ = sender.send(Ok(body));
				return;
			}
			let prior = total.fetch_add(read, Ordering::Relaxed);
			if prior.saturating_add(read) > MAX_TOOL_OUTPUT_BYTES {
				let _ = sender.send(Err(ToolFailure::Output));
				return;
			}
			body.extend_from_slice(&chunk[..read]);
		}
	});
	(receiver, handle)
}

fn receive_reader(
	receiver: &Receiver<Result<Vec<u8>, ToolFailure>>,
	target: &mut Option<Vec<u8>>,
) -> Result<(), ToolFailure> {
	if target.is_some() {
		return Ok(());
	}
	match receiver.try_recv() {
		Ok(Ok(body)) => {
			*target = Some(body);
			Ok(())
		},
		Ok(Err(failure)) => Err(failure),
		Err(TryRecvError::Empty) => Ok(()),
		Err(TryRecvError::Disconnected) => Err(ToolFailure::Pipes),
	}
}

fn run_bounded_child(
	mut command: Command,
	input: Option<Vec<u8>>,
) -> Result<ToolOutput, DaemonWrapperError> {
	command
		.stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() })
		.stdout(Stdio::piped())
		.stderr(Stdio::piped());
	let mut child = command.spawn().map_err(|_| tool_error(ToolFailure::Spawn))?;
	let pipes = (child.stdout.take(), child.stderr.take(), child.stdin.take());
	let (stdout, stderr, stdin) = match pipes {
		(Some(stdout), Some(stderr), stdin) if input.is_none() || stdin.is_some() =>
			(stdout, stderr, stdin),
		_ => {
			let _ = child.kill();
			return match child.wait() {
				Ok(_) => Err(tool_error(ToolFailure::Pipes)),
				Err(_) => Err(tool_error(ToolFailure::Cleanup)),
			};
		},
	};
	let total = Arc::new(AtomicUsize::new(0));
	let (stdout_receiver, stdout_thread) = spawn_reader(stdout, Arc::clone(&total));
	let (stderr_receiver, stderr_thread) = spawn_reader(stderr, total);
	let input_thread = match (input, stdin) {
		(Some(body), Some(mut stdin)) =>
			Some(thread::spawn(move || stdin.write_all(&body).map_err(|_| ToolFailure::Input))),
		(None, _) => None,
		(Some(_), None) => {
			let _ = child.kill();
			let _ = child.wait();
			let _ = stdout_thread.join();
			let _ = stderr_thread.join();
			return Err(tool_error(ToolFailure::Pipes));
		},
	};

	let deadline = Instant::now() + TOOL_TIMEOUT;
	let mut stdout_body = None;
	let mut stderr_body = None;
	let mut status = None;
	let failure = loop {
		if let Err(failure) = receive_reader(&stdout_receiver, &mut stdout_body) {
			break Some(failure);
		}
		if let Err(failure) = receive_reader(&stderr_receiver, &mut stderr_body) {
			break Some(failure);
		}
		if status.is_none() {
			match child.try_wait() {
				Ok(current) => status = current,
				Err(_) => break Some(ToolFailure::Status),
			}
		}
		if status.is_some() && stdout_body.is_some() && stderr_body.is_some() {
			break None;
		}
		if Instant::now() >= deadline {
			break Some(ToolFailure::Timeout);
		}
		thread::sleep(Duration::from_millis(10));
	};

	if failure.is_some() && child.try_wait().ok().flatten().is_none() {
		let _ = child.kill();
	}
	let cleanup_status = child.wait();
	let input_status = input_thread.map(|handle| handle.join());
	let stdout_status = stdout_thread.join();
	let stderr_status = stderr_thread.join();
	if let Some(failure) = failure {
		if cleanup_status.is_err()
			|| input_status.is_some_and(|result| result.is_err())
			|| stdout_status.is_err()
			|| stderr_status.is_err()
		{
			return Err(tool_error(ToolFailure::Cleanup));
		}
		return Err(tool_error(failure));
	}
	if cleanup_status.is_err() || stdout_status.is_err() || stderr_status.is_err() {
		return Err(tool_error(ToolFailure::Cleanup));
	}
	if let Some(input_status) = input_status {
		match input_status {
			Ok(Ok(())) => {},
			Ok(Err(failure)) => return Err(tool_error(failure)),
			Err(_) => return Err(tool_error(ToolFailure::Cleanup)),
		}
	}
	if !status.is_some_and(|value| value.success()) {
		return Err(tool_error(ToolFailure::Status));
	}
	Ok(ToolOutput {
		stdout: stdout_body.ok_or_else(|| tool_error(ToolFailure::Pipes))?,
		stderr: stderr_body.ok_or_else(|| tool_error(ToolFailure::Pipes))?,
	})
}

fn security_profile(path: &Path) -> Result<Vec<u8>, DaemonWrapperError> {
	let mut command = Command::new(SECURITY_PATH);
	command.args(["cms", "-D", "-i"]).arg(path);
	Ok(run_bounded_child(command, None)?.stdout)
}

fn plutil_json_path(path: &Path) -> Result<Value, DaemonWrapperError> {
	let mut command = Command::new(PLUTIL_PATH);
	command.args(["-convert", "json", "-o", "-", "--"]).arg(path);
	let output = run_bounded_child(command, None)?;
	serde_json::from_slice(&output.stdout).map_err(|_| DaemonWrapperError)
}

fn plutil_json_bytes(body: Vec<u8>) -> Result<Value, DaemonWrapperError> {
	if body.len() > MAX_TOOL_OUTPUT_BYTES {
		return Err(DaemonWrapperError);
	}
	let mut command = Command::new(PLUTIL_PATH);
	command.args(["-convert", "json", "-o", "-", "--", "-"]);
	let output = run_bounded_child(command, Some(body))?;
	serde_json::from_slice(&output.stdout).map_err(|_| DaemonWrapperError)
}

fn codesign_verify(wrapper: &Path) -> Result<(), DaemonWrapperError> {
	let mut command = Command::new(CODESIGN_PATH);
	command.args(["--verify", "--strict", "--all-architectures", "--verbose=2"]).arg(wrapper);
	run_bounded_child(command, None).map(|_| ())
}

fn codesign_entitlements(wrapper: &Path) -> Result<Value, DaemonWrapperError> {
	let mut command = Command::new(CODESIGN_PATH);
	command.args(["-d", "--entitlements", ":-", "--xml"]).arg(wrapper);
	let output = run_bounded_child(command, None)?;
	let body = extract_xml(&output.stdout)
		.or_else(|| extract_xml(&output.stderr))
		.ok_or(DaemonWrapperError)?;
	plutil_json_bytes(body.to_vec())
}

fn codesign_details(wrapper: &Path) -> Result<String, DaemonWrapperError> {
	let mut command = Command::new(CODESIGN_PATH);
	command.args(["-d", "--verbose=4"]).arg(wrapper);
	let output = run_bounded_child(command, None)?;
	String::from_utf8(output.stderr).map_err(|_| DaemonWrapperError)
}

fn codesign_requirement(wrapper: &Path) -> Result<String, DaemonWrapperError> {
	let mut command = Command::new(CODESIGN_PATH);
	command.args(["-d", "-r-"]).arg(wrapper);
	let output = run_bounded_child(command, None)?;
	String::from_utf8(output.stdout).map_err(|_| DaemonWrapperError)
}

fn extracted_leaf_certificate(wrapper: &Path) -> Result<Vec<u8>, DaemonWrapperError> {
	let temporary = tempfile::Builder::new()
		.prefix("decodexd-certificates-")
		.tempdir()
		.map_err(|_| DaemonWrapperError)?;
	let prefix = temporary.path().join("certificate");
	let result = (|| {
		let mut command = Command::new(CODESIGN_PATH);
		let mut extract_argument = OsString::from("--extract-certificates=");
		extract_argument.push(&prefix);
		command.arg("-d").arg(extract_argument).arg(wrapper);
		run_bounded_child(command, None)?;

		let mut indexed = BTreeMap::new();
		for entry in fs::read_dir(temporary.path()).map_err(|_| DaemonWrapperError)? {
			if indexed.len() >= MAX_CERTIFICATE_COUNT {
				return Err(DaemonWrapperError);
			}
			let path = entry.map_err(|_| DaemonWrapperError)?.path();
			let name =
				path.file_name().and_then(|value| value.to_str()).ok_or(DaemonWrapperError)?;
			let suffix = name.strip_prefix("certificate").ok_or(DaemonWrapperError)?;
			if suffix.is_empty()
				|| !suffix.bytes().all(|byte| byte.is_ascii_digit())
				|| (suffix != "0" && suffix.starts_with('0'))
			{
				return Err(DaemonWrapperError);
			}
			let index = suffix.parse::<usize>().map_err(|_| DaemonWrapperError)?;
			if indexed.insert(index, path).is_some() {
				return Err(DaemonWrapperError);
			}
		}
		if indexed.is_empty() || indexed.keys().copied().ne(0..indexed.len()) {
			return Err(DaemonWrapperError);
		}
		let mut total = 0_usize;
		let mut leaf = None;
		for (index, path) in indexed {
			let certificate = read_bounded_file(&path, MAX_CERTIFICATE_BYTES, false)?;
			total = total.checked_add(certificate.len()).ok_or(DaemonWrapperError)?;
			if total > MAX_CERTIFICATE_SET_BYTES || !is_bounded_der_certificate(&certificate) {
				return Err(DaemonWrapperError);
			}
			if index == 0 {
				leaf = Some(certificate);
			}
		}
		leaf.ok_or(DaemonWrapperError)
	})();
	let cleanup = temporary.close().map_err(|_| DaemonWrapperError);
	match result {
		Err(primary) => Err(primary),
		Ok(leaf) => {
			cleanup?;
			Ok(leaf)
		},
	}
}

fn extract_xml(body: &[u8]) -> Option<&[u8]> {
	body.windows(5).position(|window| window == b"<?xml").map(|index| &body[index..])
}

fn read_bounded_file(
	path: &Path,
	maximum: usize,
	executable: bool,
) -> Result<Vec<u8>, DaemonWrapperError> {
	let mut options = OpenOptions::new();
	options.read(true).custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
	let file = options.open(path).map_err(|_| DaemonWrapperError)?;
	let metadata = file.metadata().map_err(|_| DaemonWrapperError)?;
	let is_executable = metadata.permissions().mode() & 0o111 != 0;
	if !metadata.is_file()
		|| metadata.nlink() != 1
		|| usize::try_from(metadata.len()).ok().is_none_or(|length| length > maximum)
		|| is_executable != executable
	{
		return Err(DaemonWrapperError);
	}
	let mut body = Vec::new();
	file.take(u64::try_from(maximum).map_err(|_| DaemonWrapperError)? + 1)
		.read_to_end(&mut body)
		.map_err(|_| DaemonWrapperError)?;
	if body.len() > maximum {
		return Err(DaemonWrapperError);
	}
	if u64::try_from(body.len()).map_err(|_| DaemonWrapperError)? != metadata.len() {
		return Err(DaemonWrapperError);
	}
	Ok(body)
}

fn require_directory(path: &Path) -> Result<(), DaemonWrapperError> {
	let metadata = fs::symlink_metadata(path).map_err(|_| DaemonWrapperError)?;
	if !metadata.is_dir() || metadata.file_type().is_symlink() {
		return Err(DaemonWrapperError);
	}
	Ok(())
}

fn exact_names(path: &Path) -> Result<BTreeSet<OsString>, DaemonWrapperError> {
	fs::read_dir(path)
		.map_err(|_| DaemonWrapperError)?
		.map(|entry| entry.map(|value| value.file_name()).map_err(|_| DaemonWrapperError))
		.collect()
}

fn validate_layout(wrapper: &Path) -> Result<(PathBuf, PathBuf, PathBuf), DaemonWrapperError> {
	if !wrapper.is_absolute()
		|| wrapper.file_name().and_then(|value| value.to_str()) != Some(WRAPPER_NAME)
		|| fs::canonicalize(wrapper).map_err(|_| DaemonWrapperError)? != wrapper
	{
		return Err(DaemonWrapperError);
	}
	let contents = wrapper.join("Contents");
	let macos = contents.join("MacOS");
	let signature = contents.join("_CodeSignature");
	for directory in [wrapper, &contents, &macos, &signature] {
		require_directory(directory)?;
	}
	if exact_names(wrapper)? != BTreeSet::from([OsString::from("Contents")])
		|| exact_names(&contents)?
			!= BTreeSet::from([
				OsString::from("Info.plist"),
				OsString::from("MacOS"),
				OsString::from("_CodeSignature"),
				OsString::from("embedded.provisionprofile"),
			]) || exact_names(&macos)? != BTreeSet::from([OsString::from(BUNDLE_EXECUTABLE)])
		|| exact_names(&signature)? != BTreeSet::from([OsString::from("CodeResources")])
	{
		return Err(DaemonWrapperError);
	}
	let executable = macos.join(BUNDLE_EXECUTABLE);
	let info = contents.join("Info.plist");
	let profile = contents.join("embedded.provisionprofile");
	let _ = read_bounded_file(&signature.join("CodeResources"), MAX_PLIST_BYTES, false)?;
	Ok((executable, info, profile))
}

fn sha256(body: &[u8]) -> String {
	let digest = Sha256::digest(body);
	let mut encoded = String::with_capacity(64);
	for byte in digest {
		use fmt::Write as _;
		let _ = write!(encoded, "{byte:02x}");
	}
	encoded
}

fn expected_info_plist() -> Value {
	json!({
		"CFBundleDevelopmentRegion": "en",
		"CFBundleExecutable": BUNDLE_EXECUTABLE,
		"CFBundleIdentifier": BUNDLE_IDENTIFIER,
		"CFBundleInfoDictionaryVersion": "6.0",
		"CFBundleName": "decodexd",
		"CFBundlePackageType": BUNDLE_PACKAGE_TYPE,
		"LSBackgroundOnly": true,
	})
}

fn expected_entitlements() -> Value {
	json!({
		"com.apple.application-identifier": APPLICATION_IDENTIFIER,
		"com.apple.developer.team-identifier": TEAM_IDENTIFIER,
		"keychain-access-groups": [APPLICATION_IDENTIFIER],
	})
}

fn object(value: &Value) -> Result<&Map<String, Value>, DaemonWrapperError> {
	value.as_object().ok_or(DaemonWrapperError)
}

fn exact_string_array(value: Option<&Value>, expected: &str) -> bool {
	matches!(
		value.and_then(Value::as_array).map(Vec::as_slice),
		Some([Value::String(actual)]) if actual == expected
	)
}

fn is_bounded_der_certificate(body: &[u8]) -> bool {
	if body.len() < 4 || body.len() > MAX_CERTIFICATE_BYTES || body[0] != 0x30 {
		return false;
	}
	let first_length = body[1];
	let (header_length, content_length) = if first_length < 0x80 {
		(2, usize::from(first_length))
	} else {
		let length_bytes = usize::from(first_length & 0x7f);
		if length_bytes == 0 || length_bytes > 4 || body.len() < 2 + length_bytes || body[2] == 0 {
			return false;
		}
		let mut content_length = 0_usize;
		for byte in &body[2..2 + length_bytes] {
			let Some(next) = content_length
				.checked_mul(256)
				.and_then(|value| value.checked_add(usize::from(*byte)))
			else {
				return false;
			};
			content_length = next;
		}
		if content_length < 0x80 {
			return false;
		}
		(2 + length_bytes, content_length)
	};
	header_length.checked_add(content_length) == Some(body.len())
		&& content_length > 0
		&& body.get(header_length) == Some(&0x30)
}

struct ProfileIdentity {
	expires_at: String,
	developer_certificates: HashSet<Vec<u8>>,
}

fn profile_contains_leaf(profile: &ProfileIdentity, leaf: &[u8]) -> bool {
	profile.developer_certificates.contains(leaf)
}

fn profile_certificates(
	profile: &Map<String, Value>,
) -> Result<HashSet<Vec<u8>>, DaemonWrapperError> {
	let certificates =
		profile.get("DeveloperCertificates").and_then(Value::as_array).ok_or(DaemonWrapperError)?;
	if certificates.is_empty() || certificates.len() > MAX_CERTIFICATE_COUNT {
		return Err(DaemonWrapperError);
	}
	let mut total = 0_usize;
	let mut certificate_set = HashSet::with_capacity(certificates.len());
	for value in certificates {
		let encoded = value.as_str().ok_or(DaemonWrapperError)?;
		if encoded.len() > MAX_CERTIFICATE_BYTES.saturating_mul(4) / 3 + 4 {
			return Err(DaemonWrapperError);
		}
		let certificate = STANDARD.decode(encoded).map_err(|_| DaemonWrapperError)?;
		total = total.checked_add(certificate.len()).ok_or(DaemonWrapperError)?;
		if total > MAX_CERTIFICATE_SET_BYTES
			|| STANDARD.encode(&certificate) != encoded
			|| !is_bounded_der_certificate(&certificate)
			|| !certificate_set.insert(certificate)
		{
			return Err(DaemonWrapperError);
		}
	}
	Ok(certificate_set)
}

fn validate_profile(value: &Value) -> Result<ProfileIdentity, DaemonWrapperError> {
	let profile = object(value)?;
	let devices = profile.get("ProvisionedDevices").and_then(Value::as_array);
	let entitlements = profile.get("Entitlements").and_then(Value::as_object);
	if !exact_string_array(profile.get("TeamIdentifier"), TEAM_IDENTIFIER)
		|| !exact_string_array(profile.get("ApplicationIdentifierPrefix"), TEAM_IDENTIFIER)
		|| devices.is_none_or(|values| {
			values.is_empty() || values.iter().any(|value| value.as_str().is_none_or(str::is_empty))
		}) || profile.get("ProvisionsAllDevices") == Some(&Value::Bool(true))
		|| entitlements.is_none_or(|values| {
			values.get("com.apple.application-identifier").and_then(Value::as_str)
				!= Some(APPLICATION_IDENTIFIER)
				|| values.get("com.apple.developer.team-identifier").and_then(Value::as_str)
					!= Some(TEAM_IDENTIFIER)
				|| !exact_string_array(values.get("keychain-access-groups"), PROFILE_ACCESS_GROUP)
		}) {
		return Err(DaemonWrapperError);
	}
	let expiry = profile.get("ExpirationDate").and_then(Value::as_str).ok_or(DaemonWrapperError)?;
	let expiry_seconds = parse_utc_timestamp(expiry)?;
	let now =
		SystemTime::now().duration_since(UNIX_EPOCH).map_err(|_| DaemonWrapperError)?.as_secs();
	if expiry_seconds <= i64::try_from(now).map_err(|_| DaemonWrapperError)? {
		return Err(DaemonWrapperError);
	}
	Ok(ProfileIdentity {
		expires_at: expiry.to_owned(),
		developer_certificates: profile_certificates(profile)?,
	})
}

fn parse_utc_timestamp(value: &str) -> Result<i64, DaemonWrapperError> {
	let bytes = value.as_bytes();
	if bytes.len() != 20
		|| bytes[4] != b'-'
		|| bytes[7] != b'-'
		|| bytes[10] != b'T'
		|| bytes[13] != b':'
		|| bytes[16] != b':'
		|| bytes[19] != b'Z'
	{
		return Err(DaemonWrapperError);
	}
	let number = |start: usize, end: usize| -> Result<i64, DaemonWrapperError> {
		value[start..end].parse::<i64>().map_err(|_| DaemonWrapperError)
	};
	let year = number(0, 4)?;
	let month = number(5, 7)?;
	let day = number(8, 10)?;
	let hour = number(11, 13)?;
	let minute = number(14, 16)?;
	let second = number(17, 19)?;
	let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
	let month_days = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
	if !(1..=12).contains(&month)
		|| day < 1
		|| day > month_days[usize::try_from(month - 1).map_err(|_| DaemonWrapperError)?]
		|| hour > 23
		|| minute > 59
		|| second > 59
	{
		return Err(DaemonWrapperError);
	}
	let adjusted_year = year - i64::from(month <= 2);
	let era = adjusted_year.div_euclid(400);
	let year_of_era = adjusted_year - era * 400;
	let adjusted_month = month + if month > 2 { -3 } else { 9 };
	let day_of_year = (153 * adjusted_month + 2) / 5 + day - 1;
	let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
	let days = era * 146_097 + day_of_era - 719_468;
	Ok(days * 86_400 + hour * 3_600 + minute * 60 + second)
}

fn normalize_spaces(value: &str) -> String {
	value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn singleton<'a>(lines: &'a [&str], prefix: &str) -> Result<&'a str, DaemonWrapperError> {
	let values = lines
		.iter()
		.filter_map(|line| line.strip_prefix(prefix).map(str::trim))
		.collect::<Vec<_>>();
	match values.as_slice() {
		[value] if !value.is_empty() => Ok(value),
		_ => Err(DaemonWrapperError),
	}
}

fn signature_identity(
	details: &str,
	requirement: &str,
	leaf_certificate_sha256: &str,
) -> Result<Value, DaemonWrapperError> {
	let lines = details.lines().collect::<Vec<_>>();
	let identifier = singleton(&lines, "Identifier=")?;
	let team = singleton(&lines, "TeamIdentifier=")?;
	let cdhash = singleton(&lines, "CDHash=")?;
	let code_directory = singleton(&lines, "CodeDirectory ")?;
	let authorities = lines
		.iter()
		.filter_map(|line| line.strip_prefix("Authority=").map(str::trim))
		.collect::<Vec<_>>();
	let requirements = requirement
		.lines()
		.filter_map(|line| line.strip_prefix("designated =>").map(normalize_spaces))
		.collect::<Vec<_>>();
	let authority_set = authorities.iter().copied().collect::<HashSet<_>>();
	if identifier != BUNDLE_IDENTIFIER
		|| team != TEAM_IDENTIFIER
		|| !matches!(cdhash.len(), 40 | 64)
		|| !cdhash.bytes().all(|byte| byte.is_ascii_hexdigit())
		|| !code_directory.contains("runtime")
		|| authorities.is_empty()
		|| authorities.iter().any(|authority| authority.is_empty())
		|| authority_set.len() != authorities.len()
		|| requirements.len() != 1
		|| !requirements[0].contains(&format!("identifier \"{BUNDLE_IDENTIFIER}\""))
		|| leaf_certificate_sha256.len() != 64
		|| !leaf_certificate_sha256
			.bytes()
			.all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
	{
		return Err(DaemonWrapperError);
	}
	Ok(json!({
		"identifier": identifier,
		"team_identifier": team,
		"cdhash": cdhash.to_ascii_lowercase(),
		"code_directory": normalize_spaces(code_directory),
		"designated_requirement": requirements[0],
		"certificate_authorities": authorities,
		"leaf_certificate_sha256": leaf_certificate_sha256,
	}))
}

fn push_ascii_json_string(output: &mut Vec<u8>, value: &str) {
	output.push(b'"');
	for character in value.chars() {
		match character {
			'"' => output.extend_from_slice(br#"\""#),
			'\\' => output.extend_from_slice(br#"\\"#),
			'\u{0008}' => output.extend_from_slice(br#"\b"#),
			'\u{000c}' => output.extend_from_slice(br#"\f"#),
			'\n' => output.extend_from_slice(br#"\n"#),
			'\r' => output.extend_from_slice(br#"\r"#),
			'\t' => output.extend_from_slice(br#"\t"#),
			'\u{0020}'..='\u{007e}' => {
				let mut buffer = [0_u8; 4];
				output.extend_from_slice(character.encode_utf8(&mut buffer).as_bytes());
			},
			value if u32::from(value) <= 0xffff => {
				output.extend_from_slice(format!("\\u{:04x}", u32::from(value)).as_bytes());
			},
			value => {
				let scalar = u32::from(value) - 0x1_0000;
				let high = 0xd800 + (scalar >> 10);
				let low = 0xdc00 + (scalar & 0x3ff);
				output.extend_from_slice(format!("\\u{high:04x}\\u{low:04x}").as_bytes());
			},
		}
	}
	output.push(b'"');
}

fn push_canonical_json(output: &mut Vec<u8>, value: &Value) -> Result<(), DaemonWrapperError> {
	match value {
		Value::Null => output.extend_from_slice(b"null"),
		Value::Bool(true) => output.extend_from_slice(b"true"),
		Value::Bool(false) => output.extend_from_slice(b"false"),
		Value::Number(number) if !number.is_f64() => {
			output.extend_from_slice(number.to_string().as_bytes());
		},
		Value::Number(_) => return Err(DaemonWrapperError),
		Value::String(value) => push_ascii_json_string(output, value),
		Value::Array(values) => {
			output.push(b'[');
			for (index, value) in values.iter().enumerate() {
				if index != 0 {
					output.push(b',');
				}
				push_canonical_json(output, value)?;
			}
			output.push(b']');
		},
		Value::Object(values) => {
			output.push(b'{');
			let mut entries = values.iter().collect::<Vec<_>>();
			entries.sort_unstable_by(|(left, _), (right, _)| left.cmp(right));
			for (index, (key, value)) in entries.into_iter().enumerate() {
				if index != 0 {
					output.push(b',');
				}
				push_ascii_json_string(output, key);
				output.push(b':');
				push_canonical_json(output, value)?;
			}
			output.push(b'}');
		},
	}
	Ok(())
}

fn canonical_json(value: &Value) -> Result<Vec<u8>, DaemonWrapperError> {
	let mut output = Vec::new();
	push_canonical_json(&mut output, value)?;
	Ok(output)
}

fn validate_descriptor(descriptor: &DaemonWrapperDescriptor) -> Result<(), DaemonWrapperError> {
	let wrapper = Path::new(&descriptor.wrapper_path);
	let executable = Path::new(&descriptor.executable_path);
	let info = wrapper.join("Contents/Info.plist");
	let profile = wrapper.join("Contents/embedded.provisionprofile");
	if descriptor.schema != DESCRIPTOR_SCHEMA
		|| !wrapper.is_absolute()
		|| wrapper.file_name().and_then(|value| value.to_str()) != Some(WRAPPER_NAME)
		|| fs::canonicalize(wrapper).map_err(|_| DaemonWrapperError)? != wrapper
		|| executable != wrapper.join("Contents/MacOS").join(BUNDLE_EXECUTABLE)
		|| fs::canonicalize(executable).map_err(|_| DaemonWrapperError)? != executable
		|| info.to_str().is_none_or(|value| descriptor.info_plist_path != value)
		|| profile.to_str().is_none_or(|value| descriptor.embedded_profile_path != value)
		|| descriptor.executable_byte_count == 0
		|| descriptor.bundle_identifier != BUNDLE_IDENTIFIER
		|| descriptor.bundle_executable != BUNDLE_EXECUTABLE
		|| descriptor.bundle_package_type != BUNDLE_PACKAGE_TYPE
		|| !descriptor.background_only
		|| descriptor.team_identifier != TEAM_IDENTIFIER
		|| descriptor.application_identifier != APPLICATION_IDENTIFIER
		|| descriptor.profile_channel != PROFILE_CHANNEL
		|| descriptor.keychain_access_groups.len() != 1
		|| descriptor.keychain_access_groups.first().map(String::as_str)
			!= Some(APPLICATION_IDENTIFIER)
		|| [
			&descriptor.executable_sha256,
			&descriptor.info_plist_sha256,
			&descriptor.embedded_profile_sha256,
			&descriptor.signed_entitlements_sha256,
			&descriptor.signature_identity_sha256,
		]
		.into_iter()
		.any(|digest| {
			digest.len() != 64
				|| !digest
					.bytes()
					.all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
		}) {
		return Err(DaemonWrapperError);
	}
	let _ = parse_utc_timestamp(&descriptor.profile_expires_at)?;
	Ok(())
}

/// Return the Python-compatible canonical SHA-256 of one strict descriptor.
pub fn daemon_wrapper_descriptor_sha256(
	descriptor: &DaemonWrapperDescriptor,
) -> Result<String, DaemonWrapperError> {
	validate_descriptor(descriptor)?;
	let value = serde_json::to_value(descriptor).map_err(|_| DaemonWrapperError)?;
	Ok(sha256(&canonical_json(&value)?))
}

/// Inspect the wrapper that contains the current process executable.
pub fn inspect_current_daemon_wrapper() -> Result<DaemonWrapperDescriptor, DaemonWrapperError> {
	let executable = env::current_exe().map_err(|_| DaemonWrapperError)?;
	let executable = fs::canonicalize(&executable).map_err(|_| DaemonWrapperError)?;
	let macos = executable.parent().ok_or(DaemonWrapperError)?;
	let contents = macos.parent().ok_or(DaemonWrapperError)?;
	let wrapper = contents.parent().ok_or(DaemonWrapperError)?;
	if macos.file_name().and_then(|value| value.to_str()) != Some("MacOS")
		|| contents.file_name().and_then(|value| value.to_str()) != Some("Contents")
		|| executable.file_name().and_then(|value| value.to_str()) != Some(BUNDLE_EXECUTABLE)
	{
		return Err(DaemonWrapperError);
	}
	let (layout_executable, info_path, profile_path) = validate_layout(wrapper)?;
	if layout_executable != executable {
		return Err(DaemonWrapperError);
	}
	let executable_body = read_bounded_file(&executable, MAX_EXECUTABLE_BYTES, true)?;
	let info_body = read_bounded_file(&info_path, MAX_PLIST_BYTES, false)?;
	let profile_body = read_bounded_file(&profile_path, MAX_PROFILE_BYTES, false)?;
	if !MACHO_MAGICS.iter().any(|magic| executable_body.starts_with(magic))
		|| info_body != INFO_PLIST_AUTHORITY
		|| plutil_json_path(&info_path)? != expected_info_plist()
	{
		return Err(DaemonWrapperError);
	}
	let decoded_profile = security_profile(&profile_path)?;
	let profile = plutil_json_bytes(decoded_profile)?;
	let profile_identity = validate_profile(&profile)?;
	codesign_verify(wrapper)?;
	let entitlements = codesign_entitlements(wrapper)?;
	if entitlements != expected_entitlements() {
		return Err(DaemonWrapperError);
	}
	let leaf_certificate = extracted_leaf_certificate(wrapper)?;
	if !profile_contains_leaf(&profile_identity, &leaf_certificate) {
		return Err(DaemonWrapperError);
	}
	let identity = signature_identity(
		&codesign_details(wrapper)?,
		&codesign_requirement(wrapper)?,
		&sha256(&leaf_certificate),
	)?;
	let wrapper_path = wrapper.to_str().ok_or(DaemonWrapperError)?.to_owned();
	let executable_path = executable.to_str().ok_or(DaemonWrapperError)?.to_owned();
	let descriptor = DaemonWrapperDescriptor {
		schema: DESCRIPTOR_SCHEMA.to_owned(),
		wrapper_path,
		executable_path,
		executable_sha256: sha256(&executable_body),
		executable_byte_count: u64::try_from(executable_body.len())
			.map_err(|_| DaemonWrapperError)?,
		info_plist_path: info_path.to_str().ok_or(DaemonWrapperError)?.to_owned(),
		info_plist_sha256: sha256(&info_body),
		bundle_identifier: BUNDLE_IDENTIFIER.to_owned(),
		bundle_executable: BUNDLE_EXECUTABLE.to_owned(),
		bundle_package_type: BUNDLE_PACKAGE_TYPE.to_owned(),
		background_only: true,
		embedded_profile_path: profile_path.to_str().ok_or(DaemonWrapperError)?.to_owned(),
		embedded_profile_sha256: sha256(&profile_body),
		team_identifier: TEAM_IDENTIFIER.to_owned(),
		application_identifier: APPLICATION_IDENTIFIER.to_owned(),
		profile_expires_at: profile_identity.expires_at,
		profile_channel: PROFILE_CHANNEL.to_owned(),
		signed_entitlements_sha256: sha256(&canonical_json(&entitlements)?),
		keychain_access_groups: vec![APPLICATION_IDENTIFIER.to_owned()],
		signature_identity_sha256: sha256(&canonical_json(&identity)?),
	};
	validate_descriptor(&descriptor)?;
	Ok(descriptor)
}

/// Re-inspect the current wrapper and require exact canonical descriptor equality.
pub fn verify_current_daemon_wrapper(
	expected: &DaemonWrapperDescriptor,
) -> Result<DaemonWrapperDescriptor, DaemonWrapperError> {
	validate_descriptor(expected)?;
	let current = inspect_current_daemon_wrapper()?;
	let expected_json = serde_json::to_value(expected).map_err(|_| DaemonWrapperError)?;
	let current_json = serde_json::to_value(&current).map_err(|_| DaemonWrapperError)?;
	if canonical_json(&expected_json)? != canonical_json(&current_json)? {
		return Err(DaemonWrapperError);
	}
	Ok(current)
}

/// Require one LaunchAgent document to execute the exact verified wrapper main.
pub(crate) fn verify_launch_agent_daemon_wrapper(
	body: &[u8],
	expected: &DaemonWrapperDescriptor,
) -> Result<(), DaemonWrapperError> {
	validate_descriptor(expected)?;
	if body.is_empty() || body.len() > MAX_PLIST_BYTES {
		return Err(DaemonWrapperError);
	}
	let launch_agent = plutil_json_bytes(body.to_vec())?;
	let arguments = launch_agent
		.as_object()
		.and_then(|document| document.get("ProgramArguments"))
		.and_then(Value::as_array)
		.ok_or(DaemonWrapperError)?;
	match arguments.first().and_then(Value::as_str) {
		Some(executable) if executable == expected.executable_path => Ok(()),
		_ => Err(DaemonWrapperError),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	const DER_CERTIFICATE: &[u8] = b"\x30\x03\x30\x01\x00";
	const DER_CERTIFICATE_BASE64: &str = "MAMwAQA=";

	fn wrapper_fixture(root: &Path) -> PathBuf {
		let wrapper = root.join(WRAPPER_NAME);
		let macos = wrapper.join("Contents/MacOS");
		fs::create_dir_all(&macos).unwrap();
		fs::write(macos.join(BUNDLE_EXECUTABLE), b"fixture").unwrap();
		wrapper
	}

	fn descriptor_fixture(wrapper: &Path) -> DaemonWrapperDescriptor {
		let executable = wrapper.join("Contents/MacOS").join(BUNDLE_EXECUTABLE);
		DaemonWrapperDescriptor {
			schema: DESCRIPTOR_SCHEMA.to_owned(),
			wrapper_path: wrapper.to_str().unwrap().to_owned(),
			executable_path: executable.to_str().unwrap().to_owned(),
			executable_sha256: "0".repeat(64),
			executable_byte_count: 7,
			info_plist_path: wrapper.join("Contents/Info.plist").to_str().unwrap().to_owned(),
			info_plist_sha256: "1".repeat(64),
			bundle_identifier: BUNDLE_IDENTIFIER.to_owned(),
			bundle_executable: BUNDLE_EXECUTABLE.to_owned(),
			bundle_package_type: BUNDLE_PACKAGE_TYPE.to_owned(),
			background_only: true,
			embedded_profile_path: wrapper
				.join("Contents/embedded.provisionprofile")
				.to_str()
				.unwrap()
				.to_owned(),
			embedded_profile_sha256: "2".repeat(64),
			team_identifier: TEAM_IDENTIFIER.to_owned(),
			application_identifier: APPLICATION_IDENTIFIER.to_owned(),
			profile_expires_at: "2099-01-01T00:00:00Z".to_owned(),
			profile_channel: PROFILE_CHANNEL.to_owned(),
			signed_entitlements_sha256: "3".repeat(64),
			keychain_access_groups: vec![APPLICATION_IDENTIFIER.to_owned()],
			signature_identity_sha256: "4".repeat(64),
		}
	}

	fn signature_details() -> &'static str {
		"Identifier=box.acg.decodex.daemon\n\
		 TeamIdentifier=T54QFA7W2S\n\
		 CDHash=ABCDEF0123456789ABCDEF0123456789ABCDEF01\n\
		 CodeDirectory v=20500 size=512 flags=0x10000(runtime) hashes=8\n\
		 Authority=Apple Development: Decodex Test\n\
		 Authority=Apple Worldwide Developer Relations Certification Authority\n"
	}

	#[test]
	fn canonical_json_matches_python_ascii_sorting() {
		let value = json!({"z": "😀", "a": "é", "control": "\n"});
		let expected = br#"{"a":"\u00e9","control":"\n","z":"\ud83d\ude00"}"#;
		assert_eq!(canonical_json(&value).unwrap(), expected);
		assert_eq!(sha256(&canonical_json(&value).unwrap()), sha256(expected));
	}

	#[test]
	fn descriptor_rejects_unknown_fields() {
		let temporary = tempfile::tempdir().unwrap();
		let wrapper = wrapper_fixture(temporary.path());
		let descriptor = descriptor_fixture(&wrapper);
		let mut value = serde_json::to_value(descriptor).unwrap();
		value.as_object_mut().unwrap().insert("unknown".to_owned(), Value::Bool(true));
		assert!(serde_json::from_value::<DaemonWrapperDescriptor>(value).is_err());
	}

	#[test]
	fn descriptor_paths_are_absolute_bound_and_drift_sensitive() {
		let temporary = tempfile::tempdir().unwrap();
		let original_parent = temporary.path().join("original");
		let moved_parent = temporary.path().join("moved");
		let other_parent = temporary.path().join("other");
		fs::create_dir_all(&original_parent).unwrap();
		fs::create_dir_all(&moved_parent).unwrap();
		fs::create_dir_all(&other_parent).unwrap();
		let wrapper = wrapper_fixture(&original_parent);
		let other = wrapper_fixture(&other_parent);
		let original = descriptor_fixture(&wrapper);
		assert!(validate_descriptor(&original).is_ok());

		let moved = moved_parent.join(WRAPPER_NAME);
		fs::rename(&wrapper, &moved).unwrap();
		assert!(validate_descriptor(&original).is_err());
		let moved_descriptor = descriptor_fixture(&moved);
		assert!(validate_descriptor(&moved_descriptor).is_ok());

		let mut cross_bound = moved_descriptor.clone();
		cross_bound.executable_path =
			other.join("Contents/MacOS").join(BUNDLE_EXECUTABLE).to_str().unwrap().to_owned();
		assert!(validate_descriptor(&cross_bound).is_err());

		fs::remove_file(moved.join("Contents/MacOS").join(BUNDLE_EXECUTABLE)).unwrap();
		assert!(validate_descriptor(&moved_descriptor).is_err());
	}

	#[test]
	fn profile_closes_team_group_channel_and_expiry() {
		let valid = json!({
			"TeamIdentifier": [TEAM_IDENTIFIER],
			"ApplicationIdentifierPrefix": [TEAM_IDENTIFIER],
			"ExpirationDate": "2099-01-01T00:00:00Z",
			"ProvisionedDevices": ["device"],
			"DeveloperCertificates": [DER_CERTIFICATE_BASE64],
			"Entitlements": {
				"com.apple.application-identifier": APPLICATION_IDENTIFIER,
				"com.apple.developer.team-identifier": TEAM_IDENTIFIER,
				"keychain-access-groups": [PROFILE_ACCESS_GROUP],
			},
		});
		let profile = validate_profile(&valid).unwrap();
		assert_eq!(profile.expires_at, "2099-01-01T00:00:00Z");
		assert!(profile_contains_leaf(&profile, DER_CERTIFICATE));
		assert!(!profile_contains_leaf(&profile, b"\x30\x03\x30\x01\x01"));

		let mut duplicate_certificates = valid.clone();
		duplicate_certificates["DeveloperCertificates"] =
			json!([DER_CERTIFICATE_BASE64, DER_CERTIFICATE_BASE64]);
		assert!(validate_profile(&duplicate_certificates).is_err());

		let mut missing_certificates = valid.clone();
		missing_certificates.as_object_mut().unwrap().remove("DeveloperCertificates");
		assert!(validate_profile(&missing_certificates).is_err());

		let mut legacy_application_key = valid.clone();
		let entitlements = legacy_application_key["Entitlements"].as_object_mut().unwrap();
		entitlements.remove("com.apple.application-identifier");
		entitlements.insert(
			"application-identifier".to_owned(),
			Value::String(APPLICATION_IDENTIFIER.to_owned()),
		);
		assert!(validate_profile(&legacy_application_key).is_err());

		let mut signed_group_as_profile_allowlist = valid.clone();
		signed_group_as_profile_allowlist["Entitlements"]["keychain-access-groups"] =
			json!([APPLICATION_IDENTIFIER]);
		assert!(validate_profile(&signed_group_as_profile_allowlist).is_err());

		let mut extra_profile_group = valid.clone();
		extra_profile_group["Entitlements"]["keychain-access-groups"] =
			json!([PROFILE_ACCESS_GROUP, "T54QFA7W2S.extra"]);
		assert!(validate_profile(&extra_profile_group).is_err());

		for changed in [
			json!({"TeamIdentifier": ["WRONG"]}),
			json!({
				"TeamIdentifier": [TEAM_IDENTIFIER],
				"ApplicationIdentifierPrefix": [TEAM_IDENTIFIER],
				"ExpirationDate": "2000-01-01T00:00:00Z",
				"ProvisionsAllDevices": true,
				"DeveloperCertificates": [DER_CERTIFICATE_BASE64],
				"Entitlements": {},
			}),
		] {
			assert!(validate_profile(&changed).is_err());
		}
	}

	#[test]
	fn entitlements_and_signature_identity_are_exact() {
		assert_eq!(
			expected_entitlements(),
			json!({
				"com.apple.application-identifier": APPLICATION_IDENTIFIER,
				"com.apple.developer.team-identifier": TEAM_IDENTIFIER,
				"keychain-access-groups": [APPLICATION_IDENTIFIER],
			})
		);
		let requirement =
			format!("designated => anchor apple generic and identifier \"{BUNDLE_IDENTIFIER}\"");
		let leaf_sha256 = sha256(DER_CERTIFICATE);
		let identity = signature_identity(signature_details(), &requirement, &leaf_sha256).unwrap();
		assert_eq!(identity["team_identifier"], TEAM_IDENTIFIER);
		assert_eq!(identity["leaf_certificate_sha256"], leaf_sha256);
		let expected = format!(
			r#"{{"cdhash":"abcdef0123456789abcdef0123456789abcdef01","certificate_authorities":["Apple Development: Decodex Test","Apple Worldwide Developer Relations Certification Authority"],"code_directory":"v=20500 size=512 flags=0x10000(runtime) hashes=8","designated_requirement":"anchor apple generic and identifier \"box.acg.decodex.daemon\"","identifier":"box.acg.decodex.daemon","leaf_certificate_sha256":"{leaf_sha256}","team_identifier":"T54QFA7W2S"}}"#
		);
		assert_eq!(canonical_json(&identity).unwrap(), expected.as_bytes());
		assert!(
			signature_identity(
				&signature_details().replace(TEAM_IDENTIFIER, "WRONG"),
				&requirement,
				&leaf_sha256,
			)
			.is_err()
		);
	}

	#[test]
	fn bounded_tool_failures_are_nonsecret_and_typed() {
		for failure in [
			ToolFailure::Spawn,
			ToolFailure::Pipes,
			ToolFailure::Input,
			ToolFailure::Timeout,
			ToolFailure::Output,
			ToolFailure::Status,
			ToolFailure::Cleanup,
		] {
			assert_eq!(tool_error(failure).to_string(), "daemon wrapper identity is unavailable");
		}
	}

	#[test]
	fn utc_timestamp_rejects_invalid_and_accepts_exact_development_expiry() {
		assert_eq!(parse_utc_timestamp("1970-01-01T00:00:00Z").unwrap(), 0);
		assert!(parse_utc_timestamp("2026-02-30T00:00:00Z").is_err());
		assert!(parse_utc_timestamp("2026-01-01T00:00:00+00:00").is_err());
	}
}
