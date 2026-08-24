//! Single daemon-owned coordinator for stable shared Codex auth observation and cutover.

use std::{
	sync::{Arc, Mutex},
};

#[cfg(target_os = "macos")]
use std::{
	collections::{HashMap, HashSet},
	ffi::{OsStr, OsString, c_void},
	mem::MaybeUninit,
	os::unix::ffi::OsStringExt as _,
	path::{Path, PathBuf},
};

use crate::{
	auth_projection::{
		CodexAuthProjectionError, SharedCodexAuthFileStamp, SharedCodexAuthSnapshot,
		SharedCodexAuthVersion, project_shared_codex_auth_cas, read_shared_codex_auth_snapshot,
		read_shared_codex_auth_stamp,
	},
	host_credentials::CredentialSecretBundle,
};

const REQUIRED_STABLE_POLLS: u8 = 2;

/// Conservative process observation. Any uncertainty keeps the shared writer handoff closed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CodexLiveness {
	Quiescent,
	MayBeRunning,
}

pub(crate) trait CodexLivenessPort: Send + Sync {
	fn observe(&self) -> CodexLiveness;
}

pub(crate) trait SharedAuthFilePort: Send + Sync {
	fn stamp(&self) -> Result<SharedCodexAuthFileStamp, CodexAuthProjectionError>;

	fn read(
		&self,
		expected: &SharedCodexAuthFileStamp,
	) -> Result<SharedCodexAuthSnapshot, CodexAuthProjectionError>;

	fn project(
		&self,
		bundle: &CredentialSecretBundle,
		provider_account_id: &str,
		expected: &SharedCodexAuthVersion,
	) -> Result<(), CodexAuthProjectionError>;
}

struct ProductionSharedAuthFile;
impl SharedAuthFilePort for ProductionSharedAuthFile {
	fn stamp(&self) -> Result<SharedCodexAuthFileStamp, CodexAuthProjectionError> {
		read_shared_codex_auth_stamp()
	}

	fn read(
		&self,
		expected: &SharedCodexAuthFileStamp,
	) -> Result<SharedCodexAuthSnapshot, CodexAuthProjectionError> {
		read_shared_codex_auth_snapshot(expected)
	}

	fn project(
		&self,
		bundle: &CredentialSecretBundle,
		provider_account_id: &str,
		expected: &SharedCodexAuthVersion,
	) -> Result<(), CodexAuthProjectionError> {
		project_shared_codex_auth_cas(bundle, provider_account_id, expected)
	}
}

#[derive(Default)]
struct StableReadState {
	candidate: Option<SharedCodexAuthFileStamp>,
	consecutive: u8,
	handled: Option<SharedCodexAuthFileStamp>,
}

pub(crate) enum StableSharedAuthPoll {
	Changed(Box<SharedCodexAuthSnapshot>),
	Unchanged,
	Waiting,
	Unavailable,
}

pub(crate) enum StableSharedAuthRead {
	Ready(Box<SharedCodexAuthSnapshot>),
	Waiting,
	Unavailable,
}

/// One coordinator instance owns all shared-auth polling and projection decisions in `decodexd`.
pub(crate) struct SharedAuthCoordinator {
	liveness: Arc<dyn CodexLivenessPort>,
	file: Arc<dyn SharedAuthFilePort>,
	state: Mutex<StableReadState>,
}

impl SharedAuthCoordinator {
	pub(crate) fn production() -> Self {
		Self {
			liveness: Arc::new(ProductionCodexLiveness),
			file: Arc::new(ProductionSharedAuthFile),
			state: Mutex::new(StableReadState::default()),
		}
	}

	#[cfg(test)]
	pub(crate) fn with_ports(
		liveness: Arc<dyn CodexLivenessPort>,
		file: Arc<dyn SharedAuthFilePort>,
	) -> Self {
		Self { liveness, file, state: Mutex::new(StableReadState::default()) }
	}

	pub(crate) fn liveness(&self) -> CodexLiveness {
		self.liveness.observe()
	}

	pub(crate) fn poll_stable_change(&self) -> StableSharedAuthPoll {
		let stamp = match self.file.stamp() {
			Ok(stamp) => stamp,
			Err(_) => return StableSharedAuthPoll::Unavailable,
		};
		let mut state = match self.state.lock() {
			Ok(state) => state,
			Err(_) => return StableSharedAuthPoll::Unavailable,
		};
		if state.candidate.as_ref() != Some(&stamp) {
			state.candidate = Some(stamp);
			state.consecutive = 1;
			state.handled = None;
			return StableSharedAuthPoll::Waiting;
		}
		state.consecutive = state.consecutive.saturating_add(1);
		if state.consecutive < REQUIRED_STABLE_POLLS {
			return StableSharedAuthPoll::Waiting;
		}
		if state.handled.as_ref() == Some(&stamp) {
			return StableSharedAuthPoll::Unchanged;
		}
		drop(state);
		match self.file.read(&stamp) {
			Ok(snapshot) => {
				if let Ok(mut state) = self.state.lock()
					&& state.candidate.as_ref() == Some(&stamp)
				{
					state.handled = Some(stamp);
				}
				StableSharedAuthPoll::Changed(Box::new(snapshot))
			},
			Err(_) => StableSharedAuthPoll::Unavailable,
		}
	}

	pub(crate) fn read_current_stable(&self) -> StableSharedAuthRead {
		let stamp = match self.file.stamp() {
			Ok(stamp) => stamp,
			Err(_) => return StableSharedAuthRead::Unavailable,
		};
		let mut state = match self.state.lock() {
			Ok(state) => state,
			Err(_) => return StableSharedAuthRead::Unavailable,
		};
		if state.candidate.as_ref() != Some(&stamp) {
			state.candidate = Some(stamp);
			state.consecutive = 1;
			state.handled = None;
			return StableSharedAuthRead::Waiting;
		}
		state.consecutive = state.consecutive.saturating_add(1);
		if state.consecutive < REQUIRED_STABLE_POLLS {
			return StableSharedAuthRead::Waiting;
		}
		drop(state);
		match self.file.read(&stamp) {
			Ok(snapshot) => StableSharedAuthRead::Ready(Box::new(snapshot)),
			Err(_) => StableSharedAuthRead::Unavailable,
		}
	}

	pub(crate) fn project_if_quiescent(
		&self,
		bundle: &CredentialSecretBundle,
		provider_account_id: &str,
		expected: &SharedCodexAuthVersion,
	) -> Result<(), CodexAuthProjectionError> {
		if self.liveness() != CodexLiveness::Quiescent {
			return Err(CodexAuthProjectionError::SourceChanged);
		}
		self.file.project(bundle, provider_account_id, expected)
	}
}

struct ProductionCodexLiveness;

#[cfg(target_os = "macos")]
impl CodexLivenessPort for ProductionCodexLiveness {
	fn observe(&self) -> CodexLiveness {
		observe_macos_codex_liveness()
	}
}

#[cfg(not(target_os = "macos"))]
impl CodexLivenessPort for ProductionCodexLiveness {
	fn observe(&self) -> CodexLiveness {
		CodexLiveness::MayBeRunning
	}
}

#[cfg(target_os = "macos")]
fn observe_macos_codex_liveness() -> CodexLiveness {
	// Apple SDK `sys/proc_info.h` defines `PROC_ALL_PIDS` as 1. libc exposes the
	// functions but not this selector, so keep the one SDK-named value local.
	const PROC_ALL_PIDS: u32 = 1;
	let required = unsafe { libc::proc_listpids(PROC_ALL_PIDS, 0, std::ptr::null_mut(), 0) };
	let pid_size = std::mem::size_of::<libc::pid_t>();
	let Ok(required) = usize::try_from(required) else {
		return CodexLiveness::MayBeRunning;
	};
	if required == 0 || pid_size == 0 {
		return CodexLiveness::MayBeRunning;
	}
	let capacity = required / pid_size + 64;
	let mut pids = vec![0 as libc::pid_t; capacity];
	let Ok(buffer_bytes) = i32::try_from(pids.len().saturating_mul(pid_size)) else {
		return CodexLiveness::MayBeRunning;
	};
	let returned = unsafe {
		libc::proc_listpids(PROC_ALL_PIDS, 0, pids.as_mut_ptr().cast::<c_void>(), buffer_bytes)
	};
	let Ok(returned) = usize::try_from(returned) else {
		return CodexLiveness::MayBeRunning;
	};
	if returned == 0 || returned >= pids.len().saturating_mul(pid_size) {
		return CodexLiveness::MayBeRunning;
	}
	pids.truncate(returned / pid_size);
	let observations = pids
		.into_iter()
		.filter(|pid| *pid > 0)
		.map(observe_macos_process)
		.collect::<Vec<_>>();
	let Ok(own_pid) = libc::pid_t::try_from(std::process::id()) else {
		return CodexLiveness::MayBeRunning;
	};
	classify_macos_codex_liveness(own_pid, &observations)
}

#[cfg(target_os = "macos")]
#[derive(Clone, Debug, Eq, PartialEq)]
enum MacosProcessField<T> {
	Value(T),
	Unavailable,
	Vanished,
}

#[cfg(target_os = "macos")]
#[derive(Clone, Debug, Eq, PartialEq)]
struct MacosProcessObservation {
	pid: libc::pid_t,
	parent_pid: MacosProcessField<libc::pid_t>,
	name: MacosProcessField<OsString>,
	path: MacosProcessField<PathBuf>,
}

#[cfg(target_os = "macos")]
fn observe_macos_process(pid: libc::pid_t) -> MacosProcessObservation {
	let name = observe_macos_process_name(pid);
	let path = match &name {
		MacosProcessField::Value(name) if !process_name_looks_like_codex(name) => {
			MacosProcessField::Unavailable
		},
		MacosProcessField::Vanished => MacosProcessField::Vanished,
		MacosProcessField::Value(_) | MacosProcessField::Unavailable => {
			observe_macos_process_path(pid)
		},
	};
	MacosProcessObservation { pid, parent_pid: observe_macos_parent_pid(pid), name, path }
}

#[cfg(target_os = "macos")]
fn observe_macos_process_name(pid: libc::pid_t) -> MacosProcessField<OsString> {
	let mut bytes = vec![0_u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
	let length = unsafe {
		libc::proc_name(
			pid,
			bytes.as_mut_ptr().cast::<c_void>(),
			libc::PROC_PIDPATHINFO_MAXSIZE as u32,
		)
	};
	if length == 0 {
		return if std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
			MacosProcessField::Vanished
		} else {
			MacosProcessField::Unavailable
		};
	}
	let Ok(length) = usize::try_from(length) else {
		return MacosProcessField::Unavailable;
	};
	if length > bytes.len() {
		return MacosProcessField::Unavailable;
	}
	let length = bytes[..length].iter().position(|byte| *byte == 0).unwrap_or(length);
	if length == 0 {
		return MacosProcessField::Unavailable;
	}
	bytes.truncate(length);
	MacosProcessField::Value(OsString::from_vec(bytes))
}

#[cfg(target_os = "macos")]
fn observe_macos_process_path(pid: libc::pid_t) -> MacosProcessField<PathBuf> {
	let mut bytes = vec![0_u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
	let length = unsafe {
		libc::proc_pidpath(
			pid,
			bytes.as_mut_ptr().cast::<c_void>(),
			libc::PROC_PIDPATHINFO_MAXSIZE as u32,
		)
	};
	if length == 0 {
		return if std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
			MacosProcessField::Vanished
		} else {
			MacosProcessField::Unavailable
		};
	}
	let Ok(length) = usize::try_from(length) else {
		return MacosProcessField::Unavailable;
	};
	if length > bytes.len() {
		return MacosProcessField::Unavailable;
	}
	let length = bytes[..length].iter().position(|byte| *byte == 0).unwrap_or(length);
	if length == 0 {
		return MacosProcessField::Unavailable;
	}
	bytes.truncate(length);
	MacosProcessField::Value(PathBuf::from(OsString::from_vec(bytes)))
}

#[cfg(target_os = "macos")]
fn observe_macos_parent_pid(pid: libc::pid_t) -> MacosProcessField<libc::pid_t> {
	let mut info = MaybeUninit::<libc::proc_bsdinfo>::zeroed();
	let Ok(size) = i32::try_from(std::mem::size_of::<libc::proc_bsdinfo>()) else {
		return MacosProcessField::Unavailable;
	};
	let returned = unsafe {
		libc::proc_pidinfo(
			pid,
			libc::PROC_PIDTBSDINFO,
			0,
			info.as_mut_ptr().cast::<c_void>(),
			size,
		)
	};
	if returned == 0 {
		return if std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
			MacosProcessField::Vanished
		} else {
			MacosProcessField::Unavailable
		};
	}
	if returned != size {
		return MacosProcessField::Unavailable;
	}
	let info = unsafe { info.assume_init() };
	libc::pid_t::try_from(info.pbi_ppid)
		.map(MacosProcessField::Value)
		.unwrap_or(MacosProcessField::Unavailable)
}

#[cfg(target_os = "macos")]
fn classify_macos_codex_liveness(
	own_pid: libc::pid_t,
	observations: &[MacosProcessObservation],
) -> CodexLiveness {
	let parents = observations
		.iter()
		.filter_map(|observation| match observation.parent_pid {
			MacosProcessField::Value(parent) => Some((observation.pid, parent)),
			MacosProcessField::Unavailable | MacosProcessField::Vanished => None,
		})
		.collect::<HashMap<_, _>>();
	for observation in observations {
		if observation.pid == own_pid || !macos_process_looks_like_external_codex(observation) {
			continue;
		}
		// Decodex-owned app-server children use the daemon's attested external-token
		// path and cannot own the normal shared auth writer. Counting them would make
		// the coordinator permanently wait on itself.
		if process_descends_from(observation.pid, own_pid, &parents) {
			continue;
		}
		return CodexLiveness::MayBeRunning;
	}
	CodexLiveness::Quiescent
}

#[cfg(target_os = "macos")]
fn macos_process_looks_like_external_codex(observation: &MacosProcessObservation) -> bool {
	match &observation.path {
		MacosProcessField::Value(path) => path_looks_like_codex(path),
		MacosProcessField::Vanished => false,
		MacosProcessField::Unavailable => match &observation.name {
			MacosProcessField::Value(name) => process_name_looks_like_codex(name),
			MacosProcessField::Unavailable | MacosProcessField::Vanished => false,
		},
	}
}

#[cfg(target_os = "macos")]
fn process_descends_from(
	pid: libc::pid_t,
	ancestor: libc::pid_t,
	parents: &HashMap<libc::pid_t, libc::pid_t>,
) -> bool {
	let mut current = pid;
	let mut visited = HashSet::new();
	while current > 1 && visited.insert(current) {
		let Some(parent) = parents.get(&current).copied() else {
			return false;
		};
		if parent == ancestor {
			return true;
		}
		current = parent;
	}
	false
}

#[cfg(target_os = "macos")]
fn process_name_looks_like_codex(name: &OsStr) -> bool {
	name == OsStr::new("ChatGPT")
		|| name == OsStr::new("Codex")
		|| name == OsStr::new("codex")
		|| name == OsStr::new("verified-codex-image")
}

#[cfg(target_os = "macos")]
fn path_looks_like_codex(path: &Path) -> bool {
	let executable = path.file_name();
	path.ends_with("ChatGPT.app/Contents/MacOS/ChatGPT")
		|| path.ends_with("ChatGPT.app/Contents/Resources/codex")
		|| path.ends_with("Codex.app/Contents/MacOS/Codex")
		|| path.ends_with("Codex.app/Contents/Resources/codex")
		|| executable == Some(OsStr::new("codex"))
		|| executable == Some(OsStr::new("Codex"))
		|| executable == Some(OsStr::new("verified-codex-image"))
}

#[cfg(test)]
mod tests {
	use std::{
		collections::VecDeque,
		sync::atomic::{AtomicUsize, Ordering},
	};

	use super::*;

	struct FixedLiveness(CodexLiveness);
	impl CodexLivenessPort for FixedLiveness {
		fn observe(&self) -> CodexLiveness {
			self.0
		}
	}

	struct ScriptedFile {
		stamps: Mutex<VecDeque<Result<SharedCodexAuthFileStamp, CodexAuthProjectionError>>>,
		reads: AtomicUsize,
		failed_reads: AtomicUsize,
	}

	impl SharedAuthFilePort for ScriptedFile {
		fn stamp(&self) -> Result<SharedCodexAuthFileStamp, CodexAuthProjectionError> {
			self.stamps
				.lock()
				.expect("script lock")
				.pop_front()
				.unwrap_or(Ok(SharedCodexAuthFileStamp::Absent))
		}

		fn read(
			&self,
			expected: &SharedCodexAuthFileStamp,
		) -> Result<SharedCodexAuthSnapshot, CodexAuthProjectionError> {
			self.reads.fetch_add(1, Ordering::Relaxed);
			if self
				.failed_reads
				.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |remaining| {
					remaining.checked_sub(1)
				})
				.is_ok()
			{
				return Err(CodexAuthProjectionError::Unavailable);
			}
			Ok(SharedCodexAuthSnapshot::Unmanaged {
				version: SharedCodexAuthVersion { stamp: expected.clone(), sha256: None },
			})
		}

		fn project(
			&self,
			_bundle: &CredentialSecretBundle,
			_provider_account_id: &str,
			_expected: &SharedCodexAuthVersion,
		) -> Result<(), CodexAuthProjectionError> {
			Ok(())
		}
	}

	#[test]
	fn stable_poll_requires_two_equal_metadata_observations_and_coalesces_reads() {
		let file = Arc::new(ScriptedFile {
			stamps: Mutex::new(VecDeque::from([
				Ok(SharedCodexAuthFileStamp::Absent),
				Ok(SharedCodexAuthFileStamp::Absent),
				Ok(SharedCodexAuthFileStamp::Absent),
			])),
			reads: AtomicUsize::new(0),
			failed_reads: AtomicUsize::new(0),
		});
		let coordinator = SharedAuthCoordinator::with_ports(
			Arc::new(FixedLiveness(CodexLiveness::Quiescent)),
			file.clone(),
		);
		assert!(matches!(coordinator.poll_stable_change(), StableSharedAuthPoll::Waiting));
		assert!(matches!(coordinator.poll_stable_change(), StableSharedAuthPoll::Changed(_)));
		assert!(matches!(coordinator.poll_stable_change(), StableSharedAuthPoll::Unchanged));
		assert_eq!(file.reads.load(Ordering::Relaxed), 1);
	}

	#[test]
	fn unavailable_stable_read_is_retried_without_a_metadata_change() {
		let file = Arc::new(ScriptedFile {
			stamps: Mutex::new(VecDeque::from([
				Ok(SharedCodexAuthFileStamp::Absent),
				Ok(SharedCodexAuthFileStamp::Absent),
				Ok(SharedCodexAuthFileStamp::Absent),
			])),
			reads: AtomicUsize::new(0),
			failed_reads: AtomicUsize::new(1),
		});
		let coordinator = SharedAuthCoordinator::with_ports(
			Arc::new(FixedLiveness(CodexLiveness::Quiescent)),
			file.clone(),
		);
		assert!(matches!(coordinator.poll_stable_change(), StableSharedAuthPoll::Waiting));
		assert!(matches!(coordinator.poll_stable_change(), StableSharedAuthPoll::Unavailable));
		assert!(matches!(coordinator.poll_stable_change(), StableSharedAuthPoll::Changed(_)));
		assert_eq!(file.reads.load(Ordering::Relaxed), 2);
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn codex_path_matcher_covers_current_and_legacy_apps_without_generic_helpers() {
		assert!(process_name_looks_like_codex(OsStr::new("ChatGPT")));
		assert!(process_name_looks_like_codex(OsStr::new("codex")));
		assert!(!process_name_looks_like_codex(OsStr::new("launchd")));
		assert!(path_looks_like_codex(Path::new(
			"/Applications/ChatGPT.app/Contents/MacOS/ChatGPT",
		)));
		assert!(path_looks_like_codex(Path::new(
			"/Applications/ChatGPT.app/Contents/Resources/codex",
		)));
		assert!(path_looks_like_codex(Path::new("/Applications/Codex.app/Contents/MacOS/Codex",)));
		assert!(path_looks_like_codex(Path::new(
			"/Applications/Codex.app/Contents/Resources/codex",
		)));
		assert!(!path_looks_like_codex(Path::new(
			"/Applications/ChatGPT.app/Contents/Frameworks/ChatGPT Helper.app/Contents/MacOS/ChatGPT Helper",
		)));
	}

	#[cfg(target_os = "macos")]
	fn process(
		pid: libc::pid_t,
		parent_pid: MacosProcessField<libc::pid_t>,
		name: MacosProcessField<&str>,
		path: MacosProcessField<&str>,
	) -> MacosProcessObservation {
		MacosProcessObservation {
			pid,
			parent_pid,
			name: match name {
				MacosProcessField::Value(name) => {
					MacosProcessField::Value(OsString::from(name))
				},
				MacosProcessField::Unavailable => MacosProcessField::Unavailable,
				MacosProcessField::Vanished => MacosProcessField::Vanished,
			},
			path: match path {
				MacosProcessField::Value(path) => {
					MacosProcessField::Value(PathBuf::from(path))
				},
				MacosProcessField::Unavailable => MacosProcessField::Unavailable,
				MacosProcessField::Vanished => MacosProcessField::Vanished,
			},
		}
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn unrelated_inaccessible_process_does_not_prevent_quiescence() {
		let observations = [process(
			20,
			MacosProcessField::Value(1),
			MacosProcessField::Value("launchd"),
			MacosProcessField::Unavailable,
		)];
		assert_eq!(classify_macos_codex_liveness(10, &observations), CodexLiveness::Quiescent);
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn named_codex_with_inaccessible_path_fails_closed() {
		let observations = [process(
			20,
			MacosProcessField::Value(1),
			MacosProcessField::Value("codex"),
			MacosProcessField::Unavailable,
		)];
		assert_eq!(classify_macos_codex_liveness(10, &observations), CodexLiveness::MayBeRunning);
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn no_candidate_or_vanished_candidate_is_quiescent() {
		let observations = [process(
			20,
			MacosProcessField::Vanished,
			MacosProcessField::Value("codex"),
			MacosProcessField::Vanished,
		)];
		assert_eq!(classify_macos_codex_liveness(10, &[]), CodexLiveness::Quiescent);
		assert_eq!(classify_macos_codex_liveness(10, &observations), CodexLiveness::Quiescent);
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn matching_chatgpt_parent_or_codex_child_blocks_cutover() {
		let observations = [
			process(
				20,
				MacosProcessField::Value(1),
				MacosProcessField::Value("ChatGPT"),
				MacosProcessField::Value("/Applications/ChatGPT.app/Contents/MacOS/ChatGPT"),
			),
			process(
				21,
				MacosProcessField::Value(20),
				MacosProcessField::Value("codex"),
				MacosProcessField::Value(
					"/Applications/ChatGPT.app/Contents/Resources/codex",
				),
			),
		];
		assert_eq!(classify_macos_codex_liveness(10, &observations), CodexLiveness::MayBeRunning);
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn daemon_owned_codex_descendant_does_not_block_its_coordinator() {
		let observations = [
			process(
				11,
				MacosProcessField::Value(10),
				MacosProcessField::Value("helper"),
				MacosProcessField::Unavailable,
			),
			process(
				12,
				MacosProcessField::Value(11),
				MacosProcessField::Value("codex"),
				MacosProcessField::Value(
					"/Applications/ChatGPT.app/Contents/Resources/codex",
				),
			),
		];
		assert_eq!(classify_macos_codex_liveness(10, &observations), CodexLiveness::Quiescent);
	}
}
