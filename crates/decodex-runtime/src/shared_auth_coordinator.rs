//! Single daemon-owned coordinator for stable shared Codex auth observation and cutover.
#![cfg_attr(all(feature = "process-acceptance-fixture", debug_assertions), allow(dead_code))]

use std::sync::{Arc, Mutex};

#[cfg(target_os = "macos")]
use std::{
	collections::{HashMap, HashSet},
	ffi::{OsStr, OsString, c_void},
	fs,
	mem::{MaybeUninit, size_of},
	os::unix::ffi::{OsStrExt as _, OsStringExt as _},
	path::{Path, PathBuf},
};

#[cfg(target_os = "macos")]
use zeroize::Zeroizing;

use crate::{
	auth_projection::{
		CodexAuthProjectionError, SharedCodexAuthFileStamp, SharedCodexAuthSnapshot,
		SharedCodexAuthVersion, project_shared_codex_auth_cas, read_shared_codex_auth_snapshot,
		read_shared_codex_auth_stamp,
	},
	host_credentials::CredentialSecretBundle,
};

const REQUIRED_STABLE_POLLS: u8 = 2;
pub(crate) const MAX_CODEX_AUTH_OWNER_BLOCKERS: usize = 8;

#[cfg(target_os = "macos")]
const MAX_PROCESS_ENVIRONMENT_BYTES: usize = 2 * 1024 * 1024;

#[cfg(target_os = "macos")]
const MAX_PROCESS_HOME_BYTES: usize = 4 * 1024;

#[cfg(target_os = "macos")]
const MAX_CODEX_HOME_INSPECTIONS: usize = 16;

/// Conservative process observation. Any uncertainty keeps the shared writer handoff closed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CodexLiveness {
	Quiescent,
	MayBeRunning,
}

/// Credential-negative process identity shown when a Route waits for shared-auth ownership.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CodexAuthOwnerKind {
	Chatgpt,
	Codex,
}

/// Whether one blocking process is proved to use the normal shared Codex home.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CodexAuthHomeEvidence {
	Shared,
	Unknown,
}

/// One bounded same-UID process that can still write the normal shared Codex auth file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CodexAuthOwnerBlocker {
	pub(crate) pid: u32,
	pub(crate) kind: CodexAuthOwnerKind,
	pub(crate) auth_home: CodexAuthHomeEvidence,
}

/// Structured liveness readback. `Unavailable` remains fail-closed but is distinguishable from a
/// concrete process blocker.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum CodexLivenessObservation {
	Quiescent,
	Blocked {
		blockers: Vec<CodexAuthOwnerBlocker>,
		omitted: u16,
	},
	Unavailable,
}

impl CodexLivenessObservation {
	pub(crate) const fn state(&self) -> CodexLiveness {
		match self {
			Self::Quiescent => CodexLiveness::Quiescent,
			Self::Blocked { .. } | Self::Unavailable => CodexLiveness::MayBeRunning,
		}
	}

	#[cfg(test)]
	pub(crate) fn from_state(state: CodexLiveness) -> Self {
		match state {
			CodexLiveness::Quiescent => Self::Quiescent,
			CodexLiveness::MayBeRunning => Self::Unavailable,
		}
	}
}

pub(crate) trait CodexLivenessPort: Send + Sync {
	fn observe(&self) -> CodexLivenessObservation;
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
		self.liveness.observe().state()
	}

	pub(crate) fn liveness_observation(&self) -> CodexLivenessObservation {
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

	/// Read one exact source version without waiting for the passive two-poll coalescer.
	pub(crate) fn read_current_exact(
		&self,
	) -> Result<Box<SharedCodexAuthSnapshot>, CodexAuthProjectionError> {
		let stamp = self.file.stamp()?;
		self.file.read(&stamp).map(Box::new)
	}

	/// Replace one proved same-account source without requiring external Codex quiescence.
	/// The Account Service must prove the provider and credential lineage before this call.
	pub(crate) fn project_exact_source(
		&self,
		bundle: &CredentialSecretBundle,
		provider_account_id: &str,
		expected: &SharedCodexAuthVersion,
	) -> Result<(), CodexAuthProjectionError> {
		self.file.project(bundle, provider_account_id, expected)
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
	fn observe(&self) -> CodexLivenessObservation {
		#[cfg(all(feature = "process-acceptance-fixture", debug_assertions))]
		{
			if crate::account_service::process_acceptance_fixture_endpoint().is_some() {
				CodexLivenessObservation::Quiescent
			} else {
				observe_macos_codex_liveness()
			}
		}
		#[cfg(not(all(feature = "process-acceptance-fixture", debug_assertions)))]
		{
			observe_macos_codex_liveness()
		}
	}
}

#[cfg(not(target_os = "macos"))]
impl CodexLivenessPort for ProductionCodexLiveness {
	fn observe(&self) -> CodexLivenessObservation {
		CodexLivenessObservation::Unavailable
	}
}

#[cfg(target_os = "macos")]
fn observe_macos_codex_liveness() -> CodexLivenessObservation {
	// Apple SDK `sys/proc_info.h` defines `PROC_ALL_PIDS` as 1. libc exposes the
	// functions but not this selector, so keep the one SDK-named value local.
	const PROC_ALL_PIDS: u32 = 1;
	let Some(shared_codex_home) = std::env::var_os("HOME")
		.filter(|home| !home.is_empty())
		.map(PathBuf::from)
		.map(|home| home.join(".codex"))
	else {
		return CodexLivenessObservation::Unavailable;
	};
	let required = unsafe { libc::proc_listpids(PROC_ALL_PIDS, 0, std::ptr::null_mut(), 0) };
	let pid_size = std::mem::size_of::<libc::pid_t>();
	let Ok(required) = usize::try_from(required) else {
		return CodexLivenessObservation::Unavailable;
	};
	if required == 0 || pid_size == 0 {
		return CodexLivenessObservation::Unavailable;
	}
	let capacity = required / pid_size + 64;
	let mut pids = vec![0 as libc::pid_t; capacity];
	let Ok(buffer_bytes) = i32::try_from(pids.len().saturating_mul(pid_size)) else {
		return CodexLivenessObservation::Unavailable;
	};
	let returned = unsafe {
		libc::proc_listpids(PROC_ALL_PIDS, 0, pids.as_mut_ptr().cast::<c_void>(), buffer_bytes)
	};
	let Ok(returned) = usize::try_from(returned) else {
		return CodexLivenessObservation::Unavailable;
	};
	if returned == 0 || returned >= pids.len().saturating_mul(pid_size) {
		return CodexLivenessObservation::Unavailable;
	}
	pids.truncate(returned / pid_size);
	let Ok(own_pid) = libc::pid_t::try_from(std::process::id()) else {
		return CodexLivenessObservation::Unavailable;
	};
	let mut observations = pids
		.into_iter()
		.filter(|pid| *pid > 0)
		.map(observe_macos_process)
		.collect::<Vec<_>>();
	enrich_macos_codex_home_evidence(own_pid, &shared_codex_home, &mut observations);
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
	auth_home: MacosProcessField<MacosCodexHomeRelation>,
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
	let mut observation = MacosProcessObservation {
		pid,
		parent_pid: observe_macos_parent_pid(pid),
		name,
		path,
		auth_home: MacosProcessField::Unavailable,
	};
	if matches!(
		&observation.path,
		MacosProcessField::Value(path) if path_is_official_shared_codex(path)
	) {
		// Official desktop and bundled app-server identities are strict blockers. Only a
		// standalone Codex executable is eligible for best-effort isolated-home proof.
		observation.auth_home = MacosProcessField::Value(MacosCodexHomeRelation::Shared);
	}
	observation
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
		libc::proc_pidinfo(pid, libc::PROC_PIDTBSDINFO, 0, info.as_mut_ptr().cast::<c_void>(), size)
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
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MacosCodexHomeRelation {
	Shared,
	Isolated,
}

#[cfg(target_os = "macos")]
struct MacosProcessAuthEnvironment {
	codex_home: Option<OsString>,
	home: Option<OsString>,
}

#[cfg(target_os = "macos")]
fn observe_macos_codex_home(
	pid: libc::pid_t,
	shared_codex_home: &Path,
) -> MacosProcessField<MacosCodexHomeRelation> {
	let environment = match read_macos_process_auth_environment(pid) {
		MacosProcessField::Value(environment) => environment,
		MacosProcessField::Unavailable => return MacosProcessField::Unavailable,
		MacosProcessField::Vanished => return MacosProcessField::Vanished,
	};
	let process_codex_home = environment
		.codex_home
		.filter(|value| !value.is_empty())
		.map(PathBuf::from)
		.or_else(|| {
			environment.home.filter(|value| !value.is_empty()).map(PathBuf::from).map(|home| {
				home.join(".codex")
			})
		});
	let Some(process_codex_home) = process_codex_home else {
		return MacosProcessField::Unavailable;
	};
	classify_macos_codex_home(&process_codex_home, shared_codex_home)
}

#[cfg(target_os = "macos")]
fn classify_macos_codex_home(
	process_codex_home: &Path,
	shared_codex_home: &Path,
) -> MacosProcessField<MacosCodexHomeRelation> {
	if !process_codex_home.is_absolute()
		|| !shared_codex_home.is_absolute()
		|| process_codex_home.as_os_str().as_bytes().len() > MAX_PROCESS_HOME_BYTES
		|| shared_codex_home.as_os_str().as_bytes().len() > MAX_PROCESS_HOME_BYTES
	{
		return MacosProcessField::Unavailable;
	}
	if process_codex_home == shared_codex_home {
		return MacosProcessField::Value(MacosCodexHomeRelation::Shared);
	}
	let (Ok(process_codex_home), Ok(shared_codex_home)) =
		(fs::canonicalize(process_codex_home), fs::canonicalize(shared_codex_home))
	else {
		return MacosProcessField::Unavailable;
	};
	MacosProcessField::Value(if process_codex_home == shared_codex_home {
		MacosCodexHomeRelation::Shared
	} else {
		MacosCodexHomeRelation::Isolated
	})
}

#[cfg(target_os = "macos")]
fn read_macos_process_auth_environment(
	pid: libc::pid_t,
) -> MacosProcessField<MacosProcessAuthEnvironment> {
	// KERN_PROCARGS2 is a same-UID best-effort seam. Some macOS builds return argv without
	// environment entries. That decodes as no HOME evidence and therefore remains fail-closed.
	let mut argmax_mib = [libc::CTL_KERN, libc::KERN_ARGMAX];
	let mut argmax: libc::c_int = 0;
	let mut argmax_length = size_of::<libc::c_int>();
	let measured = unsafe {
		libc::sysctl(
			argmax_mib.as_mut_ptr(),
			argmax_mib.len() as libc::c_uint,
			std::ptr::addr_of_mut!(argmax).cast::<c_void>(),
			&mut argmax_length,
			std::ptr::null_mut(),
			0,
		)
	};
	if measured == -1 {
		return MacosProcessField::Unavailable;
	}
	let Ok(length) = usize::try_from(argmax) else {
		return MacosProcessField::Unavailable;
	};
	if argmax_length != size_of::<libc::c_int>()
		|| length < size_of::<libc::c_int>()
		|| length > MAX_PROCESS_ENVIRONMENT_BYTES
	{
		return MacosProcessField::Unavailable;
	}
	let mut bytes = Zeroizing::new(vec![0_u8; length]);
	let mut returned = length;
	let mut procargs_mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
	let read = unsafe {
		libc::sysctl(
			procargs_mib.as_mut_ptr(),
			procargs_mib.len() as libc::c_uint,
			bytes.as_mut_ptr().cast::<c_void>(),
			&mut returned,
			std::ptr::null_mut(),
			0,
		)
	};
	if read == -1 {
		return macos_process_read_failure();
	}
	if returned < size_of::<libc::c_int>() || returned > bytes.len() {
		return MacosProcessField::Unavailable;
	}
	parse_macos_process_auth_environment(&bytes[..returned])
		.map(MacosProcessField::Value)
		.unwrap_or(MacosProcessField::Unavailable)
}

#[cfg(target_os = "macos")]
fn macos_process_read_failure<T>() -> MacosProcessField<T> {
	if std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
		MacosProcessField::Vanished
	} else {
		MacosProcessField::Unavailable
	}
}

#[cfg(target_os = "macos")]
fn parse_macos_process_auth_environment(bytes: &[u8]) -> Option<MacosProcessAuthEnvironment> {
	let argc = libc::c_int::from_ne_bytes(bytes.get(..size_of::<libc::c_int>())?.try_into().ok()?);
	let argc = usize::try_from(argc).ok()?;
	if argc > 4_096 {
		return None;
	}
	let mut remaining = &bytes[size_of::<libc::c_int>()..];
	remaining = skip_macos_process_entry(remaining)?;
	remaining = skip_nul_padding(remaining);
	for _ in 0..argc {
		remaining = skip_macos_process_entry(remaining)?;
		remaining = skip_nul_padding(remaining);
	}

	let mut codex_home = None;
	let mut home = None;
	while !remaining.is_empty() {
		let end = remaining.iter().position(|byte| *byte == 0).unwrap_or(remaining.len());
		let entry = &remaining[..end];
		if entry.is_empty() {
			break;
		}
		if let Some(value) = entry.strip_prefix(b"CODEX_HOME=") {
			if codex_home.is_some() || value.len() > MAX_PROCESS_HOME_BYTES {
				return None;
			}
			codex_home = Some(OsString::from_vec(value.to_vec()));
		} else if let Some(value) = entry.strip_prefix(b"HOME=") {
			if home.is_some() || value.len() > MAX_PROCESS_HOME_BYTES {
				return None;
			}
			home = Some(OsString::from_vec(value.to_vec()));
		}
		remaining = if end == remaining.len() { &[] } else { &remaining[end + 1..] };
		remaining = skip_nul_padding(remaining);
	}
	Some(MacosProcessAuthEnvironment { codex_home, home })
}

#[cfg(target_os = "macos")]
fn skip_macos_process_entry(bytes: &[u8]) -> Option<&[u8]> {
	let end = bytes.iter().position(|byte| *byte == 0)?;
	Some(&bytes[end + 1..])
}

#[cfg(target_os = "macos")]
fn skip_nul_padding(mut bytes: &[u8]) -> &[u8] {
	while bytes.first() == Some(&0) {
		bytes = &bytes[1..];
	}
	bytes
}

#[cfg(target_os = "macos")]
fn enrich_macos_codex_home_evidence(
	own_pid: libc::pid_t,
	shared_codex_home: &Path,
	observations: &mut [MacosProcessObservation],
) {
	let parents = observations
		.iter()
		.filter_map(|observation| match observation.parent_pid {
			MacosProcessField::Value(parent) => Some((observation.pid, parent)),
			MacosProcessField::Unavailable | MacosProcessField::Vanished => None,
		})
		.collect::<HashMap<_, _>>();
	let mut inspected = 0_usize;
	for observation in observations {
		if observation.pid == own_pid
			|| !macos_process_looks_like_external_codex(observation)
			|| process_descends_from(observation.pid, own_pid, &parents)
			|| matches!(
				observation.auth_home,
				MacosProcessField::Value(MacosCodexHomeRelation::Shared)
					| MacosProcessField::Vanished
			)
		{
			continue;
		}
		if inspected >= MAX_CODEX_HOME_INSPECTIONS {
			continue;
		}
		inspected += 1;
		observation.auth_home = observe_macos_codex_home(observation.pid, shared_codex_home);
	}
}

#[cfg(target_os = "macos")]
fn classify_macos_codex_liveness(
	own_pid: libc::pid_t,
	observations: &[MacosProcessObservation],
) -> CodexLivenessObservation {
	let parents = observations
		.iter()
		.filter_map(|observation| match observation.parent_pid {
			MacosProcessField::Value(parent) => Some((observation.pid, parent)),
			MacosProcessField::Unavailable | MacosProcessField::Vanished => None,
		})
		.collect::<HashMap<_, _>>();
	let mut blockers = Vec::new();
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
		let auth_home = match observation.auth_home {
			MacosProcessField::Value(MacosCodexHomeRelation::Isolated)
			| MacosProcessField::Vanished => continue,
			MacosProcessField::Value(MacosCodexHomeRelation::Shared) => {
				CodexAuthHomeEvidence::Shared
			},
			MacosProcessField::Unavailable => CodexAuthHomeEvidence::Unknown,
		};
		let Ok(pid) = u32::try_from(observation.pid) else {
			continue;
		};
		let blocker = CodexAuthOwnerBlocker {
			pid,
			kind: macos_process_kind(observation),
			auth_home,
		};
		blockers.push(blocker);
	}
	blockers.sort_by_key(|blocker| blocker.pid);
	if blockers.is_empty() {
		CodexLivenessObservation::Quiescent
	} else {
		let omitted = u16::try_from(blockers.len().saturating_sub(MAX_CODEX_AUTH_OWNER_BLOCKERS))
			.unwrap_or(u16::MAX);
		blockers.truncate(MAX_CODEX_AUTH_OWNER_BLOCKERS);
		CodexLivenessObservation::Blocked { blockers, omitted }
	}
}

#[cfg(target_os = "macos")]
fn macos_process_kind(observation: &MacosProcessObservation) -> CodexAuthOwnerKind {
	let chatgpt_path = matches!(
		&observation.path,
		MacosProcessField::Value(path)
			if path.ends_with("ChatGPT.app/Contents/MacOS/ChatGPT")
	);
	let chatgpt_name = matches!(
		&observation.name,
		MacosProcessField::Value(name) if name == OsStr::new("ChatGPT")
	);
	if chatgpt_path || chatgpt_name {
		CodexAuthOwnerKind::Chatgpt
	} else {
		CodexAuthOwnerKind::Codex
	}
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
	path_is_official_shared_codex(path)
		|| executable == Some(OsStr::new("codex"))
		|| executable == Some(OsStr::new("Codex"))
		|| executable == Some(OsStr::new("verified-codex-image"))
}

#[cfg(target_os = "macos")]
fn path_is_official_shared_codex(path: &Path) -> bool {
	path.ends_with("ChatGPT.app/Contents/MacOS/ChatGPT")
		|| path.ends_with("ChatGPT.app/Contents/Resources/codex")
		|| path.ends_with("Codex.app/Contents/MacOS/Codex")
		|| path.ends_with("Codex.app/Contents/Resources/codex")
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
		fn observe(&self) -> CodexLivenessObservation {
			CodexLivenessObservation::from_state(self.0)
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
		auth_home: MacosProcessField<MacosCodexHomeRelation>,
	) -> MacosProcessObservation {
		MacosProcessObservation {
			pid,
			parent_pid,
			name: match name {
				MacosProcessField::Value(name) => MacosProcessField::Value(OsString::from(name)),
				MacosProcessField::Unavailable => MacosProcessField::Unavailable,
				MacosProcessField::Vanished => MacosProcessField::Vanished,
			},
			path: match path {
				MacosProcessField::Value(path) => MacosProcessField::Value(PathBuf::from(path)),
				MacosProcessField::Unavailable => MacosProcessField::Unavailable,
				MacosProcessField::Vanished => MacosProcessField::Vanished,
			},
			auth_home,
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
			MacosProcessField::Unavailable,
		)];
		assert_eq!(
			classify_macos_codex_liveness(10, &observations),
			CodexLivenessObservation::Quiescent
		);
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn named_codex_with_inaccessible_path_fails_closed() {
		let observations = [process(
			20,
			MacosProcessField::Value(1),
			MacosProcessField::Value("codex"),
			MacosProcessField::Unavailable,
			MacosProcessField::Unavailable,
		)];
		assert_eq!(
			classify_macos_codex_liveness(10, &observations),
			CodexLivenessObservation::Blocked {
				blockers: vec![CodexAuthOwnerBlocker {
					pid: 20,
					kind: CodexAuthOwnerKind::Codex,
					auth_home: CodexAuthHomeEvidence::Unknown,
				}],
				omitted: 0,
			}
		);
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn no_candidate_or_vanished_candidate_is_quiescent() {
		let observations = [process(
			20,
			MacosProcessField::Vanished,
			MacosProcessField::Value("codex"),
			MacosProcessField::Vanished,
			MacosProcessField::Vanished,
		)];
		assert_eq!(
			classify_macos_codex_liveness(10, &[]),
			CodexLivenessObservation::Quiescent
		);
		assert_eq!(
			classify_macos_codex_liveness(10, &observations),
			CodexLivenessObservation::Quiescent
		);
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
				MacosProcessField::Value(MacosCodexHomeRelation::Shared),
			),
			process(
				21,
				MacosProcessField::Value(20),
				MacosProcessField::Value("codex"),
				MacosProcessField::Value("/Applications/ChatGPT.app/Contents/Resources/codex"),
				MacosProcessField::Value(MacosCodexHomeRelation::Shared),
			),
		];
		let observation = classify_macos_codex_liveness(10, &observations);
		assert_eq!(observation.state(), CodexLiveness::MayBeRunning);
		let CodexLivenessObservation::Blocked { blockers, omitted: 0 } = observation else {
			panic!("matching shared-home processes must be reported")
		};
		assert_eq!(blockers.len(), 2);
		assert_eq!(blockers[0].kind, CodexAuthOwnerKind::Chatgpt);
		assert_eq!(blockers[1].kind, CodexAuthOwnerKind::Codex);
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
				MacosProcessField::Unavailable,
			),
			process(
				12,
				MacosProcessField::Value(11),
				MacosProcessField::Value("codex"),
				MacosProcessField::Value("/Applications/ChatGPT.app/Contents/Resources/codex"),
				MacosProcessField::Value(MacosCodexHomeRelation::Shared),
			),
		];
		assert_eq!(
			classify_macos_codex_liveness(10, &observations),
			CodexLivenessObservation::Quiescent
		);
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn isolated_codex_home_does_not_block_shared_auth_cutover() {
		let observations = [process(
			20,
			MacosProcessField::Value(1),
			MacosProcessField::Value("codex"),
			MacosProcessField::Value("/usr/local/bin/codex"),
			MacosProcessField::Value(MacosCodexHomeRelation::Isolated),
		)];
		assert_eq!(
			classify_macos_codex_liveness(10, &observations),
			CodexLivenessObservation::Quiescent
		);
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn blocker_snapshot_is_pid_ordered_and_bounded() {
		let observations = (20..30)
			.rev()
			.map(|pid| {
				process(
					pid,
					MacosProcessField::Value(1),
					MacosProcessField::Value("codex"),
					MacosProcessField::Value("/Applications/ChatGPT.app/Contents/Resources/codex"),
					MacosProcessField::Value(MacosCodexHomeRelation::Shared),
				)
			})
			.collect::<Vec<_>>();
		let CodexLivenessObservation::Blocked { blockers, omitted } =
			classify_macos_codex_liveness(10, &observations)
		else {
			panic!("shared-home Codex processes must block")
		};
		assert_eq!(
			blockers.iter().map(|blocker| blocker.pid).collect::<Vec<_>>(),
			(20_u32..28).collect::<Vec<_>>()
		);
		assert_eq!(omitted, 2);
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn process_environment_parser_reads_only_home_authority_fields() {
		let mut bytes = Vec::new();
		bytes.extend_from_slice(&2_i32.to_ne_bytes());
		bytes.extend_from_slice(b"/usr/local/bin/codex\0\0");
		bytes.extend_from_slice(b"codex\0app-server\0");
		bytes.extend_from_slice(b"TOKEN=must-not-escape\0");
		bytes.extend_from_slice(b"HOME=/Users/test\0");
		bytes.extend_from_slice(b"CODEX_HOME=/tmp/isolated-codex\0");
		let parsed = parse_macos_process_auth_environment(&bytes).expect("parse environment");
		assert_eq!(parsed.home.as_deref(), Some(OsStr::new("/Users/test")));
		assert_eq!(parsed.codex_home.as_deref(), Some(OsStr::new("/tmp/isolated-codex")));
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn codex_home_relation_proves_shared_symlinks_and_distinct_isolated_homes() {
		let root = tempfile::tempdir().expect("temporary root");
		let shared = root.path().join("shared-codex");
		let isolated = root.path().join("isolated-codex");
		let linked = root.path().join("linked-codex");
		fs::create_dir(&shared).expect("shared home");
		fs::create_dir(&isolated).expect("isolated home");
		std::os::unix::fs::symlink(&shared, &linked).expect("linked home");

		assert_eq!(
			classify_macos_codex_home(&linked, &shared),
			MacosProcessField::Value(MacosCodexHomeRelation::Shared)
		);
		assert_eq!(
			classify_macos_codex_home(&isolated, &shared),
			MacosProcessField::Value(MacosCodexHomeRelation::Isolated)
		);
		assert_eq!(
			classify_macos_codex_home(&root.path().join("missing"), &shared),
			MacosProcessField::Unavailable
		);
	}
}
