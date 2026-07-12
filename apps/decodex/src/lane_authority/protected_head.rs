//! Signed host-local anchor for the private authority event chain.

use ed25519_dalek::{Signature, Signer as _, SigningKey, Verifier as _, VerifyingKey};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt as _;
use std::{
	fs::{self, File, OpenOptions},
	io::{Read as _, Write as _},
	os::fd::AsRawFd as _,
	path::{Path, PathBuf},
};

use minicbor::{Decode, Encode};
use sha2::{Digest as _, Sha256};

use super::AuthorityEvent;
use crate::prelude::{Result, eyre};

const PROTECTED_HEAD_DOMAIN: &[u8] = b"decodex.authority-protected-head/1";
const AUTHORITY_DATABASE_DOMAIN: &[u8] = b"decodex.authority-database/1";
const AUTHORITY_DIR: &str = "authority";
const HOST_ID_FILE: &str = "host-id";
const PUBLIC_KEY_FILE: &str = "host-authority-public-key";
const PROTECTED_HEAD_FILE: &str = "protected-head.cbor";
const PROTECTED_HEAD_LOCK: &str = ".protected-head.lock";

#[derive(Clone, Debug, Decode, Encode, Eq, PartialEq)]
#[cbor(map)]
struct ProtectedAuthorityHeadBody {
	#[n(0)]
	host_id: String,
	#[n(1)]
	key_id: String,
	#[n(2)]
	generation: u64,
	#[n(3)]
	sequence: u64,
	#[n(4)]
	event_hash: Vec<u8>,
	#[n(5)]
	database_digest: Vec<u8>,
}

#[derive(Clone, Debug, Decode, Encode, Eq, PartialEq)]
#[cbor(map)]
pub(crate) struct ProtectedAuthorityHead {
	#[n(0)]
	body: ProtectedAuthorityHeadBody,
	#[n(1)]
	public_key: Vec<u8>,
	#[n(2)]
	signature: Vec<u8>,
}
impl ProtectedAuthorityHead {
	pub(crate) fn generation(&self) -> u64 {
		self.body.generation
	}
	pub(crate) fn sequence(&self) -> u64 {
		self.body.sequence
	}
	pub(crate) fn event_hash(&self) -> &[u8] {
		&self.body.event_hash
	}

	fn signed_bytes(body: &ProtectedAuthorityHeadBody) -> Result<Vec<u8>> {
		let mut bytes = PROTECTED_HEAD_DOMAIN.to_vec();
		bytes.extend(minicbor::to_vec(body)?);
		Ok(bytes)
	}
}

#[derive(Clone)]
pub(crate) struct Ed25519HostAuthorityKey {
	key_id: String,
	signing_key: SigningKey,
}
impl Ed25519HostAuthorityKey {
	pub(crate) fn from_seed(key_id: impl Into<String>, seed: [u8; 32]) -> Self {
		Self { key_id: key_id.into(), signing_key: SigningKey::from_bytes(&seed) }
	}

	pub(crate) fn public_key(&self) -> [u8; 32] {
		self.signing_key.verifying_key().to_bytes()
	}

	pub(crate) fn sign(
		&self,
		host_id: &str,
		generation: u64,
		sequence: u64,
		event_hash: &[u8],
		database_digest: &[u8],
	) -> Result<ProtectedAuthorityHead> {
		if host_id.trim().is_empty() || event_hash.len() != 32 || database_digest.len() != 32 {
			eyre::bail!("protected_authority_head_invalid");
		}
		let body = ProtectedAuthorityHeadBody {
			host_id: host_id.to_owned(),
			key_id: self.key_id.clone(),
			generation,
			sequence,
			event_hash: event_hash.to_vec(),
			database_digest: database_digest.to_vec(),
		};
		let signature = self.signing_key.sign(&ProtectedAuthorityHead::signed_bytes(&body)?);
		Ok(ProtectedAuthorityHead {
			body,
			public_key: self.public_key().to_vec(),
			signature: signature.to_bytes().to_vec(),
		})
	}
}

pub(crate) fn load_or_create_host_authority_key(host_id: &str) -> Result<Ed25519HostAuthorityKey> {
	if host_id.trim().is_empty() {
		eyre::bail!("host_authority_identity_empty");
	}
	load_or_create_platform_key(host_id)
}

fn load_host_authority_key(host_id: &str) -> Result<Ed25519HostAuthorityKey> {
	if host_id.trim().is_empty() {
		eyre::bail!("host_authority_identity_empty");
	}
	load_platform_key(host_id)
}

#[cfg(target_os = "macos")]
fn load_or_create_platform_key(host_id: &str) -> Result<Ed25519HostAuthorityKey> {
	use std::{fs::File, io::Read as _};

	use security_framework::passwords::{
		PasswordOptions, generic_password, set_generic_password_options,
	};
	use security_framework_sys::base::errSecItemNotFound;

	const SERVICE: &str = "space.decodex.host-authority";
	let options = || {
		let mut options = PasswordOptions::new_generic_password(SERVICE, host_id);
		options.set_access_synchronized(Some(false));
		options.use_protected_keychain();
		options
	};
	let seed = match generic_password(options()) {
		Ok(seed) => seed,
		Err(error) if error.code() == errSecItemNotFound => {
			let mut seed = [0_u8; 32];
			File::open("/dev/urandom")?.read_exact(&mut seed)?;
			set_generic_password_options(&seed, options())?;
			let stored = generic_password(options())?;
			if stored != seed {
				eyre::bail!("host_authority_key_create_readback_mismatch");
			}
			stored
		},
		Err(error) => return Err(eyre::eyre!("host_authority_key_read_failed:{error}")),
	};
	key_from_seed_bytes(seed)
}

#[cfg(target_os = "macos")]
fn load_platform_key(host_id: &str) -> Result<Ed25519HostAuthorityKey> {
	use security_framework::passwords::{PasswordOptions, generic_password};

	let mut options =
		PasswordOptions::new_generic_password("space.decodex.host-authority", host_id);
	options.set_access_synchronized(Some(false));
	options.use_protected_keychain();
	let seed = generic_password(options)
		.map_err(|error| eyre::eyre!("host_authority_key_read_failed:{error}"))?;
	key_from_seed_bytes(seed)
}

fn key_from_seed_bytes(seed: Vec<u8>) -> Result<Ed25519HostAuthorityKey> {
	let seed: [u8; 32] =
		seed.try_into().map_err(|_| eyre::eyre!("host_authority_key_seed_length_invalid"))?;
	let public_key = SigningKey::from_bytes(&seed).verifying_key().to_bytes();
	let key_digest = Sha256::digest(public_key);
	let key_id = format!(
		"ed25519:{}",
		key_digest.iter().map(|byte| format!("{byte:02x}")).collect::<String>()
	);
	Ok(Ed25519HostAuthorityKey::from_seed(key_id, seed))
}

#[cfg(not(target_os = "macos"))]
fn load_or_create_platform_key(_host_id: &str) -> Result<Ed25519HostAuthorityKey> {
	eyre::bail!("host_authority_key_protector_unsupported")
}

#[cfg(not(target_os = "macos"))]
fn load_platform_key(_host_id: &str) -> Result<Ed25519HostAuthorityKey> {
	eyre::bail!("host_authority_key_protector_unsupported")
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProtectedHeadDisposition {
	Current,
	Advanced,
}

pub(crate) struct AuthorityAnchor {
	authority_dir: PathBuf,
	host_id: String,
	key: Ed25519HostAuthorityKey,
}
impl AuthorityAnchor {
	pub(crate) fn initialize(
		runtime_root: &Path,
		generation: u64,
		genesis_hash: &[u8],
		events: &[AuthorityEvent],
	) -> Result<Self> {
		let authority_dir = runtime_root.join(AUTHORITY_DIR);
		fs::create_dir(&authority_dir)?;
		let host_id = create_host_id(&authority_dir)?;
		let key = load_or_create_host_authority_key(&host_id)?;
		Self::initialize_with_key(runtime_root, generation, genesis_hash, events, host_id, key)
	}

	fn initialize_with_key(
		runtime_root: &Path,
		generation: u64,
		genesis_hash: &[u8],
		events: &[AuthorityEvent],
		host_id: String,
		key: Ed25519HostAuthorityKey,
	) -> Result<Self> {
		let authority_dir = runtime_root.join(AUTHORITY_DIR);
		write_create_once(&authority_dir.join(PUBLIC_KEY_FILE), &key.public_key())?;
		let digest = authority_database_digest(generation, genesis_hash, events)?;
		let event_hash = events.last().map_or(genesis_hash, |event| event.event_hash.as_slice());
		let head =
			key.sign(&host_id, generation, u64::try_from(events.len())?, event_hash, &digest)?;
		write_create_once(&authority_dir.join(PROTECTED_HEAD_FILE), &minicbor::to_vec(head)?)?;
		fsync_dir(&authority_dir)?;
		Ok(Self { authority_dir, host_id, key })
	}

	pub(crate) fn open(
		runtime_root: &Path,
		generation: u64,
		genesis_hash: &[u8],
		events: &[AuthorityEvent],
	) -> Result<Self> {
		let authority_dir = runtime_root.join(AUTHORITY_DIR);
		let host_id = read_nonempty_utf8(&authority_dir.join(HOST_ID_FILE))?;
		let key = load_host_authority_key(&host_id)?;
		Self::open_with_key(runtime_root, generation, genesis_hash, events, host_id, key)
	}

	fn open_with_key(
		runtime_root: &Path,
		generation: u64,
		genesis_hash: &[u8],
		events: &[AuthorityEvent],
		host_id: String,
		key: Ed25519HostAuthorityKey,
	) -> Result<Self> {
		let authority_dir = runtime_root.join(AUTHORITY_DIR);
		let pinned_public_key = fs::read(authority_dir.join(PUBLIC_KEY_FILE))?;
		if pinned_public_key != key.public_key() {
			eyre::bail!("host_authority_public_key_pin_mismatch");
		}
		let anchor = Self { authority_dir, host_id, key };
		anchor.reconcile(generation, genesis_hash, events)?;
		Ok(anchor)
	}

	#[cfg(test)]
	pub(crate) fn initialize_for_test(
		runtime_root: &Path,
		generation: u64,
		genesis_hash: &[u8],
		events: &[AuthorityEvent],
	) -> Result<Self> {
		let authority_dir = runtime_root.join(AUTHORITY_DIR);
		fs::create_dir(&authority_dir)?;
		let host_id = create_host_id(&authority_dir)?;
		let key = Ed25519HostAuthorityKey::from_seed("test-key", [11_u8; 32]);
		Self::initialize_with_key(runtime_root, generation, genesis_hash, events, host_id, key)
	}

	#[cfg(test)]
	pub(crate) fn open_for_test(
		runtime_root: &Path,
		generation: u64,
		genesis_hash: &[u8],
		events: &[AuthorityEvent],
	) -> Result<Self> {
		let authority_dir = runtime_root.join(AUTHORITY_DIR);
		let host_id = read_nonempty_utf8(&authority_dir.join(HOST_ID_FILE))?;
		let key = Ed25519HostAuthorityKey::from_seed("test-key", [11_u8; 32]);
		Self::open_with_key(runtime_root, generation, genesis_hash, events, host_id, key)
	}

	pub(crate) fn reconcile(
		&self,
		generation: u64,
		genesis_hash: &[u8],
		events: &[AuthorityEvent],
	) -> Result<ProtectedHeadDisposition> {
		let _lock = AuthorityHeadLock::acquire(&self.authority_dir.join(PROTECTED_HEAD_LOCK))?;
		let path = self.authority_dir.join(PROTECTED_HEAD_FILE);
		let protected: ProtectedAuthorityHead = minicbor::decode(&fs::read(&path)?)?;
		let digest = authority_database_digest(generation, genesis_hash, events)?;
		let (disposition, head) = reconcile_protected_head(
			&self.host_id,
			&self.key,
			&protected,
			generation,
			genesis_hash,
			events,
			&digest,
		)?;
		if disposition == ProtectedHeadDisposition::Advanced {
			atomic_replace(&path, &minicbor::to_vec(head)?)?;
		}
		Ok(disposition)
	}
}

fn authority_database_digest(
	generation: u64,
	genesis_hash: &[u8],
	events: &[AuthorityEvent],
) -> Result<[u8; 32]> {
	let mut hasher = Sha256::new();
	hasher.update(AUTHORITY_DATABASE_DOMAIN);
	hasher.update(generation.to_be_bytes());
	hasher.update(genesis_hash);
	for event in events {
		hasher.update(event.canonical_bytes()?);
	}
	Ok(hasher.finalize().into())
}

fn create_host_id(authority_dir: &Path) -> Result<String> {
	let path = authority_dir.join(HOST_ID_FILE);
	let mut random = [0_u8; 32];
	File::open("/dev/urandom")?.read_exact(&mut random)?;
	let host_id =
		Sha256::digest(random).iter().map(|byte| format!("{byte:02x}")).collect::<String>();
	write_create_once(&path, host_id.as_bytes())?;
	Ok(host_id)
}

fn read_nonempty_utf8(path: &Path) -> Result<String> {
	let value = String::from_utf8(fs::read(path)?)?;
	if value.trim().is_empty() {
		eyre::bail!("authority_identity_file_empty");
	}
	Ok(value)
}

fn write_create_once(path: &Path, bytes: &[u8]) -> Result<()> {
	let mut options = OpenOptions::new();
	options.create_new(true).write(true);
	#[cfg(unix)]
	options.mode(0o600);
	let mut file = options.open(path)?;
	file.write_all(bytes)?;
	file.sync_all()?;
	Ok(())
}

fn atomic_replace(path: &Path, bytes: &[u8]) -> Result<()> {
	let temp = path.with_extension(format!("tmp.{}", std::process::id()));
	let mut options = OpenOptions::new();
	options.create_new(true).write(true);
	#[cfg(unix)]
	options.mode(0o600);
	let mut file = options.open(&temp)?;
	file.write_all(bytes)?;
	file.sync_all()?;
	drop(file);
	if let Err(error) = fs::rename(&temp, path) {
		let _ = fs::remove_file(&temp);
		return Err(error.into());
	}
	fsync_dir(path.parent().ok_or_else(|| eyre::eyre!("authority_head_parent_missing"))?)
}

fn fsync_dir(path: &Path) -> Result<()> {
	OpenOptions::new().read(true).open(path)?.sync_all()?;
	Ok(())
}

struct AuthorityHeadLock(File);
impl AuthorityHeadLock {
	fn acquire(path: &Path) -> Result<Self> {
		let file = OpenOptions::new().create(true).read(true).write(true).open(path)?;
		if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } != 0 {
			return Err(std::io::Error::last_os_error().into());
		}
		Ok(Self(file))
	}
}
impl Drop for AuthorityHeadLock {
	fn drop(&mut self) {
		unsafe {
			libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
		}
	}
}

#[cfg(test)]
pub(crate) fn protected_head_sequence_for_test(runtime_root: &Path) -> Result<u64> {
	let head: ProtectedAuthorityHead =
		minicbor::decode(&fs::read(runtime_root.join(AUTHORITY_DIR).join(PROTECTED_HEAD_FILE))?)?;
	Ok(head.sequence())
}

pub(crate) fn reconcile_protected_head(
	host_id: &str,
	key: &Ed25519HostAuthorityKey,
	protected: &ProtectedAuthorityHead,
	generation: u64,
	genesis_hash: &[u8],
	events: &[AuthorityEvent],
	database_digest: &[u8],
) -> Result<(ProtectedHeadDisposition, ProtectedAuthorityHead)> {
	verify_signature(host_id, key, protected)?;
	if protected.generation() != generation {
		eyre::bail!("protected_authority_head_generation_mismatch");
	}
	let database_sequence = u64::try_from(events.len())?;
	let database_hash = events.last().map_or(genesis_hash, |event| event.event_hash.as_slice());
	if protected.sequence() > database_sequence {
		eyre::bail!("protected_authority_head_ahead_of_database");
	}
	let anchored_hash = if protected.sequence() == 0 {
		genesis_hash
	} else {
		events
			.get(usize::try_from(protected.sequence() - 1)?)
			.map(|event| event.event_hash.as_slice())
			.ok_or_else(|| eyre::eyre!("protected_authority_head_sequence_missing"))?
	};
	if protected.event_hash() != anchored_hash {
		eyre::bail!("protected_authority_head_hash_mismatch");
	}
	if protected.sequence() == database_sequence {
		if protected.body.database_digest != database_digest {
			eyre::bail!("protected_authority_head_database_digest_mismatch");
		}
		return Ok((ProtectedHeadDisposition::Current, protected.clone()));
	}
	let advanced =
		key.sign(host_id, generation, database_sequence, database_hash, database_digest)?;
	Ok((ProtectedHeadDisposition::Advanced, advanced))
}

fn verify_signature(
	host_id: &str,
	key: &Ed25519HostAuthorityKey,
	protected: &ProtectedAuthorityHead,
) -> Result<()> {
	if protected.body.host_id != host_id
		|| protected.body.key_id != key.key_id
		|| protected.public_key != key.public_key()
	{
		eyre::bail!("protected_authority_head_identity_mismatch");
	}
	let public_key: [u8; 32] = protected
		.public_key
		.as_slice()
		.try_into()
		.map_err(|_| eyre::eyre!("protected_authority_head_public_key_invalid"))?;
	let signature = Signature::from_slice(&protected.signature)
		.map_err(|_| eyre::eyre!("protected_authority_head_signature_invalid"))?;
	VerifyingKey::from_bytes(&public_key)?
		.verify(&ProtectedAuthorityHead::signed_bytes(&protected.body)?, &signature)
		.map_err(|_| eyre::eyre!("protected_authority_head_signature_invalid"))
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::lane_authority::{
		AuthorityDecision, AuthorityEventDraft, AuthorityEventType, AuthorityReasonCode,
	};

	#[test]
	fn lane_authority_v2_c1_tel_09_signed_head_freezes_tamper_and_ahead_state() {
		let key = Ed25519HostAuthorityKey::from_seed("key-1", [3_u8; 32]);
		let genesis = [5_u8; 32];
		let first = AuthorityEvent::append(1, 1, &genesis, draft("event-1")).expect("event");
		let digest = [8_u8; 32];
		let genesis_head = key.sign("host-1", 1, 0, &genesis, &digest).expect("head");
		let (disposition, advanced) = reconcile_protected_head(
			"host-1",
			&key,
			&genesis_head,
			1,
			&genesis,
			std::slice::from_ref(&first),
			&digest,
		)
		.expect("advance");
		assert_eq!(disposition, ProtectedHeadDisposition::Advanced);
		let mut rewritten = advanced.clone();
		rewritten.body.event_hash[0] ^= 1;
		assert!(
			reconcile_protected_head(
				"host-1",
				&key,
				&rewritten,
				1,
				&genesis,
				std::slice::from_ref(&first),
				&digest,
			)
			.is_err()
		);
		assert!(
			reconcile_protected_head("host-1", &key, &advanced, 1, &genesis, &[], &digest).is_err()
		);
	}

	#[test]
	fn lane_authority_v2_c5_tel_10_recovers_only_database_ahead_suffix() {
		let key = Ed25519HostAuthorityKey::from_seed("key-1", [7_u8; 32]);
		let genesis = [2_u8; 32];
		let first = AuthorityEvent::append(4, 1, &genesis, draft("event-1")).expect("first");
		let second =
			AuthorityEvent::append(4, 2, &first.event_hash, draft("event-2")).expect("second");
		let old = key.sign("host-1", 4, 1, &first.event_hash, &[6_u8; 32]).expect("old");
		let (disposition, recovered) = reconcile_protected_head(
			"host-1",
			&key,
			&old,
			4,
			&genesis,
			&[first, second],
			&[9_u8; 32],
		)
		.expect("recover");
		assert_eq!(disposition, ProtectedHeadDisposition::Advanced);
		assert_eq!(recovered.sequence(), 2);
	}

	#[test]
	fn lane_authority_v2_c5_tel_10_runtime_anchor_recovers_post_commit_crash_once() {
		let temp = tempfile::tempdir().expect("tempdir");
		let genesis = [2_u8; 32];
		AuthorityAnchor::initialize_for_test(temp.path(), 4, &genesis, &[]).expect("initialize");
		let first = AuthorityEvent::append(4, 1, &genesis, draft("event-1")).expect("first");
		AuthorityAnchor::open_for_test(temp.path(), 4, &genesis, std::slice::from_ref(&first))
			.expect("recover database-ahead suffix");
		assert_eq!(protected_head_sequence_for_test(temp.path()).expect("head"), 1);
		let reopened =
			AuthorityAnchor::open_for_test(temp.path(), 4, &genesis, std::slice::from_ref(&first))
				.expect("idempotent reopen");
		assert_eq!(
			reopened.reconcile(4, &genesis, std::slice::from_ref(&first)).expect("current"),
			ProtectedHeadDisposition::Current
		);
	}

	fn draft(event_id: &str) -> AuthorityEventDraft {
		AuthorityEventDraft {
			event_id: event_id.to_owned(),
			event_type: AuthorityEventType::TransitionCommitted,
			transition_id: String::from("transition-1"),
			correlation_id: String::from("correlation-1"),
			causation_id: String::from("cause-1"),
			project_key: Some(String::from("project-1")),
			tracker_issue_id: Some(String::from("issue-1")),
			project_binding_fingerprint: Some(String::from("binding-1")),
			invocation_identity_fingerprint: String::from("invocation-1"),
			observed_facts_fingerprint: String::from("facts-1"),
			decision: AuthorityDecision::Committed,
			reason_codes: vec![AuthorityReasonCode::BindingMatched],
			operation_id: None,
			effect_id: None,
			receipt_ref: None,
			runtime_version: String::from("0.2.0"),
			recorded_at_unix_micros: 1,
			boot_id_fingerprint: String::from("boot-1"),
			monotonic_nanos: 1,
		}
	}
}
