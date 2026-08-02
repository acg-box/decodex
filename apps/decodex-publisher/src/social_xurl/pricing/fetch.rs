use std::{
	fs,
	os::unix::{
		fs::{MetadataExt as _, PermissionsExt as _},
		process::CommandExt as _,
	},
	path::Path,
	process::{Child, Command, Stdio},
	sync::mpsc,
	thread,
	time::{Duration, Instant},
};

use wait_timeout::ChildExt as _;

use super::{MAX_SOURCE_BYTES, OFFICIAL_PRICING_SOURCE};

const CURL_PATH: &str = "/usr/bin/curl";
const FETCH_DEADLINE: Duration = Duration::from_secs(10);
const MAX_STDERR_BYTES: usize = 16 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct PricingFetchFailure {
	code: &'static str,
	ordinary_https_get_count: u64,
}

impl PricingFetchFailure {
	fn before_get(code: &'static str) -> Self {
		Self { code, ordinary_https_get_count: 0 }
	}

	fn after_get(code: &'static str) -> Self {
		Self { code, ordinary_https_get_count: 1 }
	}

	pub(super) fn code(self) -> &'static str {
		self.code
	}

	pub(super) fn ordinary_https_get_count(self) -> u64 {
		self.ordinary_https_get_count
	}

	#[cfg(test)]
	pub(super) fn network_for_test() -> Self {
		Self::after_get("x_pricing_network_unavailable")
	}
}

pub(super) fn fetch_official() -> Result<Vec<u8>, PricingFetchFailure> {
	validate_curl()
		.map_err(|_| PricingFetchFailure::before_get("x_pricing_fetch_runtime_invalid"))?;
	let deadline = Instant::now()
		.checked_add(FETCH_DEADLINE)
		.ok_or_else(|| PricingFetchFailure::before_get("x_pricing_deadline_exceeded"))?;
	let mut command = Command::new(CURL_PATH);
	command
		.args(curl_arguments())
		.current_dir("/")
		.env_clear()
		.env("LANG", "C")
		.env("LC_ALL", "C")
		.env("PATH", "/usr/bin:/bin")
		.stdin(Stdio::null())
		.stdout(Stdio::piped())
		.stderr(Stdio::piped())
		.process_group(0);
	let mut child = command
		.spawn()
		.map_err(|_| PricingFetchFailure::before_get("x_pricing_network_unavailable"))?;
	let stdout = child
		.stdout
		.take()
		.ok_or_else(|| terminate_with(&mut child, "x_pricing_fetch_runtime_invalid"))?;
	let stderr = child
		.stderr
		.take()
		.ok_or_else(|| terminate_with(&mut child, "x_pricing_fetch_runtime_invalid"))?;
	let (stdout_receiver, stdout_reader) = spawn_bounded_reader(stdout, MAX_SOURCE_BYTES as usize);
	let (stderr_receiver, stderr_reader) = spawn_bounded_reader(stderr, MAX_STDERR_BYTES);

	let wait = match remaining(deadline) {
		Ok(wait) => wait,
		Err(error) => {
			terminate(&mut child);
			return Err(error);
		},
	};
	let status = match child.wait_timeout(wait) {
		Ok(Some(status)) => status,
		Ok(None) => {
			terminate(&mut child);
			return Err(PricingFetchFailure::after_get("x_pricing_deadline_exceeded"));
		},
		Err(_) => {
			terminate(&mut child);
			return Err(PricingFetchFailure::after_get("x_pricing_network_unavailable"));
		},
	};
	kill_process_group(child.id());
	let stdout = receive_bounded_reader(stdout_receiver, stdout_reader, deadline)?;
	let stderr = receive_bounded_reader(stderr_receiver, stderr_reader, deadline)?;
	if stderr.len() > MAX_STDERR_BYTES {
		return Err(PricingFetchFailure::after_get("x_pricing_fetch_output_oversize"));
	}
	if !status.success() {
		return Err(PricingFetchFailure::after_get(match status.code() {
			Some(47) => "x_pricing_redirect_rejected",
			Some(63) => "x_pricing_source_oversize",
			Some(28) => "x_pricing_deadline_exceeded",
			Some(35 | 51 | 58 | 59 | 60 | 77 | 80 | 82 | 83 | 90 | 91) => "x_pricing_tls_invalid",
			_ => "x_pricing_network_unavailable",
		}));
	}
	if stdout.len() > MAX_SOURCE_BYTES as usize {
		return Err(PricingFetchFailure::after_get("x_pricing_source_oversize"));
	}
	if stdout.is_empty() {
		return Err(PricingFetchFailure::after_get("x_pricing_source_empty"));
	}
	Ok(stdout)
}

pub(super) fn curl_arguments() -> Vec<&'static str> {
	vec![
		"--disable",
		"--silent",
		"--show-error",
		"--fail",
		"--location",
		"--max-redirs",
		"0",
		"--proto",
		"=https",
		"--proto-redir",
		"=https",
		"--connect-timeout",
		"10",
		"--max-time",
		"10",
		"--max-filesize",
		"1048576",
		"--header",
		"Accept: text/markdown",
		"--user-agent",
		"decodex-publisher-x-pricing/1",
		"--request",
		"GET",
		OFFICIAL_PRICING_SOURCE,
	]
}

fn validate_curl() -> std::io::Result<()> {
	let metadata = fs::symlink_metadata(Path::new(CURL_PATH))?;
	if metadata.file_type().is_symlink()
		|| !metadata.is_file()
		|| metadata.uid() != 0
		|| metadata.nlink() != 1
		|| metadata.permissions().mode() & 0o022 != 0
		|| metadata.permissions().mode() & 0o100 == 0
	{
		return Err(std::io::Error::other("system curl metadata is not trusted"));
	}
	Ok(())
}

type ReaderResult = std::io::Result<Vec<u8>>;

fn spawn_bounded_reader(
	reader: impl std::io::Read + Send + 'static,
	max_bytes: usize,
) -> (mpsc::Receiver<ReaderResult>, thread::JoinHandle<()>) {
	let (sender, receiver) = mpsc::sync_channel(1);
	let handle = thread::spawn(move || {
		let _ = sender.send(drain_bounded(reader, max_bytes));
	});
	(receiver, handle)
}

fn receive_bounded_reader(
	receiver: mpsc::Receiver<ReaderResult>,
	handle: thread::JoinHandle<()>,
	deadline: Instant,
) -> Result<Vec<u8>, PricingFetchFailure> {
	let output = receiver
		.recv_timeout(remaining(deadline)?)
		.map_err(|_| PricingFetchFailure::after_get("x_pricing_deadline_exceeded"))?
		.map_err(|_| PricingFetchFailure::after_get("x_pricing_source_read_invalid"))?;
	handle.join().map_err(|_| PricingFetchFailure::after_get("x_pricing_source_read_invalid"))?;
	Ok(output)
}

fn drain_bounded(mut reader: impl std::io::Read, max_bytes: usize) -> std::io::Result<Vec<u8>> {
	let mut retained = Vec::new();
	let mut buffer = [0_u8; 8192];
	loop {
		let read = reader.read(&mut buffer)?;
		if read == 0 {
			break;
		}
		let remaining = max_bytes.saturating_add(1).saturating_sub(retained.len());
		retained.extend_from_slice(&buffer[..read.min(remaining)]);
	}
	Ok(retained)
}

fn remaining(deadline: Instant) -> Result<Duration, PricingFetchFailure> {
	deadline
		.checked_duration_since(Instant::now())
		.filter(|remaining| !remaining.is_zero())
		.ok_or_else(|| PricingFetchFailure::after_get("x_pricing_deadline_exceeded"))
}

fn terminate_with(child: &mut Child, code: &'static str) -> PricingFetchFailure {
	terminate(child);
	PricingFetchFailure::after_get(code)
}

fn terminate(child: &mut Child) {
	kill_process_group(child.id());
	let _ = child.kill();
	let _ = child.wait();
}

fn kill_process_group(child_id: u32) {
	if let Ok(process_group) = i32::try_from(child_id) {
		unsafe {
			libc::kill(-process_group, libc::SIGKILL);
		}
	}
}

#[cfg(test)]
mod tests {
	use super::{MAX_SOURCE_BYTES, OFFICIAL_PRICING_SOURCE, curl_arguments, drain_bounded};

	#[test]
	fn curl_contract_is_one_credential_free_official_https_get() {
		let arguments = curl_arguments();
		assert_eq!(arguments.iter().filter(|argument| **argument == "GET").count(), 1);
		assert_eq!(
			arguments.iter().filter(|argument| **argument == OFFICIAL_PRICING_SOURCE).count(),
			1
		);
		assert!(OFFICIAL_PRICING_SOURCE.starts_with("https://"));
		assert!(arguments.windows(2).any(|pair| pair == ["--max-redirs", "0"]));
		assert!(arguments.windows(2).any(|pair| pair == ["--max-time", "10"]));
		assert!(arguments.windows(2).any(|pair| pair == ["--max-filesize", "1048576"]));
		let folded = arguments.join(" ").to_ascii_lowercase();
		for forbidden in ["oauth", "token", "authorization:", "/2/", "xurl"] {
			assert!(!folded.contains(forbidden), "{forbidden}");
		}
	}

	#[test]
	fn reader_retains_only_the_bounded_overflow_sentinel() {
		let source = vec![b'x'; MAX_SOURCE_BYTES as usize + 8192];
		let retained =
			drain_bounded(source.as_slice(), MAX_SOURCE_BYTES as usize).expect("bounded read");
		assert_eq!(retained.len(), MAX_SOURCE_BYTES as usize + 1);
	}
}
