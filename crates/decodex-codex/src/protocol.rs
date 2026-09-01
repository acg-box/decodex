use std::{
	cmp::Ordering,
	fmt::{Debug, Formatter},
};

use serde::{Serialize, Serializer};
use sha2::{Digest as _, Sha256};
use zeroize::Zeroizing;

use crate::conversation::{ConversationTurnStatus, ExactTurnId};

pub(crate) const MAX_APP_SERVER_FRAME_BYTES: usize = 1_024 * 1_024;

/// Maximum UTF-8 bytes in an executable Codex thread identifier.
pub const MAX_EXACT_THREAD_ID_BYTES: usize = decodex_core::MAX_PROVIDER_THREAD_ID_BYTES;
/// Maximum UTF-8 bytes in a Decodex-owned list search term.
pub const MAX_THREAD_SEARCH_TERM_BYTES: usize = 512;
/// Maximum UTF-8 bytes in a Codex thread title.
pub const MAX_THREAD_TITLE_BYTES: usize = 512;
/// Maximum UTF-8 bytes in a Codex thread working directory.
pub const MAX_THREAD_CWD_BYTES: usize = 4 * 1_024;
/// Maximum UTF-8 bytes in a Codex thread provenance marker.
pub const MAX_THREAD_PROVENANCE_BYTES: usize = 256;
/// Maximum number of exact list results accepted from Codex.
pub const MAX_EXACT_THREAD_LIST_RESULTS: usize = 100;
/// Maximum turns inspected from one exact `thread/read` response.
pub const MAX_EXACT_THREAD_READ_TURNS: usize = 1_024;
/// Maximum items inspected across one exact `thread/read` response.
pub const MAX_EXACT_THREAD_READ_ITEMS: usize = 8_192;
/// Maximum recovered assistant bytes retained from one exact submitted Turn.
pub const MAX_EXACT_TURN_ASSISTANT_BYTES: usize = 256 * 1_024;

/// Exact Codex CLI build identity used as capability-cache authority.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct BuildId(String);
impl BuildId {
	#[doc(hidden)]
	pub fn from_attestation(
		version: &str,
		executable_digest: &[u8; 32],
	) -> Result<Self, &'static str> {
		if version.trim().is_empty() || version.len() > 256 || version.contains(['\r', '\n', '\0'])
		{
			return Err("Codex build identity is invalid");
		}

		let mut digest = Sha256::new();

		digest.update(version.as_bytes());
		digest.update([0]);
		digest.update(executable_digest);

		Ok(Self(format!("sha256:{}", hex_digest(&digest.finalize()))))
	}

	#[cfg(test)]
	pub(crate) fn for_test(value: &str) -> Self {
		Self::from_attestation(value, &[0; 32]).expect("test build identity must be valid")
	}

	/// Return the opaque observed-executable fingerprint.
	pub fn as_str(&self) -> &str {
		&self.0
	}
}

/// Opaque Codex thread identifier.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ThreadId(String);
impl ThreadId {
	#[doc(hidden)]
	pub fn from_protocol(value: &str) -> Self {
		Self(Self::normalize(value))
	}

	pub(crate) fn normalize(value: &str) -> String {
		let bytes = value.as_bytes();
		let is_uuid = bytes.len() == 36
			&& [8, 13, 18, 23].into_iter().all(|index| bytes[index] == b'-')
			&& bytes
				.iter()
				.enumerate()
				.all(|(index, byte)| [8, 13, 18, 23].contains(&index) || byte.is_ascii_hexdigit());

		if is_uuid {
			value.to_owned()
		} else {
			format!("sha256:{}", hex_digest(&Sha256::digest(value.as_bytes())))
		}
	}

	/// Return the opaque exact identifier.
	pub fn as_str(&self) -> &str {
		&self.0
	}
}

/// Exact executable Codex thread identifier.
///
/// Unlike [`ThreadId`], this value is never hashed or normalized. It is the only thread
/// identity in this crate that may be serialized into an exact-ID app-server request.
#[derive(Clone, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ExactThreadId(ZeroizingText);
impl ExactThreadId {
	/// Validate and retain one exact protocol identifier byte-for-byte.
	pub fn new(value: impl Into<String>) -> Result<Self, &'static str> {
		let value = ZeroizingText::new(value.into());

		validate_exact_thread_id(value.as_str())?;

		Ok(Self(value))
	}

	/// Return the exact identifier for an app-server request or equality check.
	pub fn as_str(&self) -> &str {
		self.0.as_str()
	}
}
impl Debug for ExactThreadId {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str("ExactThreadId([REDACTED])")
	}
}

/// Bounded Decodex-owned provenance/title search term for `thread/list`.
#[derive(Clone, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct DecodexThreadSearchTerm(String);
impl DecodexThreadSearchTerm {
	/// Retain an exact Decodex title marker used to read back an already-owned thread.
	pub fn new(value: impl Into<String>) -> Result<Self, &'static str> {
		let value = value.into();

		validate_bounded_text(
			&value,
			MAX_THREAD_SEARCH_TERM_BYTES,
			"Codex thread search term is invalid",
		)?;

		if !value.starts_with("Decodex ") {
			return Err("Codex thread search term is not Decodex-owned");
		}

		Ok(Self(value))
	}

	/// Return the exact bounded term for `thread/list`.
	pub fn as_str(&self) -> &str {
		&self.0
	}
}
impl Debug for DecodexThreadSearchTerm {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str("DecodexThreadSearchTerm([REDACTED])")
	}
}

/// Archived-state predicate for one bounded exact list request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ThreadArchivedFilter {
	/// Return only non-archived threads.
	Current,
	/// Return only archived threads.
	Archived,
}
impl ThreadArchivedFilter {
	/// App-server boolean representation.
	pub const fn as_bool(self) -> bool {
		matches!(self, Self::Archived)
	}
}

/// Closed exact-list filter: Decodex provenance/title plus archived state only.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExactThreadListFilter {
	/// Exact bounded Decodex-owned provenance/title term.
	pub search_term: DecodexThreadSearchTerm,
	/// Required archived-state predicate.
	pub archived: ThreadArchivedFilter,
}

/// Bounded exact Codex thread title fact.
#[derive(Clone, Eq, PartialEq)]
pub struct ThreadTitle(ZeroizingText);
impl ThreadTitle {
	#[doc(hidden)]
	pub fn from_protocol(value: impl Into<String>) -> Result<Self, &'static str> {
		let value = ZeroizingText::new(value.into());

		validate_bounded_text(
			value.as_str(),
			MAX_THREAD_TITLE_BYTES,
			"Codex thread title is invalid",
		)?;

		Ok(Self(value))
	}

	/// Return the exact bounded title.
	pub fn as_str(&self) -> &str {
		self.0.as_str()
	}
}
impl Debug for ThreadTitle {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str("ThreadTitle([REDACTED])")
	}
}

/// Bounded exact Codex thread working-directory fact.
#[derive(Clone, Eq, PartialEq)]
pub struct ThreadCwd(ZeroizingText);
impl ThreadCwd {
	#[doc(hidden)]
	pub fn from_protocol(value: impl Into<String>) -> Result<Self, &'static str> {
		let value = ZeroizingText::new(value.into());

		validate_bounded_text(value.as_str(), MAX_THREAD_CWD_BYTES, "Codex thread cwd is invalid")?;

		if !value.as_str().starts_with('/') {
			return Err("Codex thread cwd is not absolute");
		}

		Ok(Self(value))
	}

	/// Return the exact bounded working directory.
	pub fn as_str(&self) -> &str {
		self.0.as_str()
	}
}
impl Debug for ThreadCwd {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str("ThreadCwd([REDACTED])")
	}
}

/// Bounded Decodex thread-source provenance fact.
#[derive(Clone, Eq, PartialEq)]
pub struct ThreadProvenance(ZeroizingText);
impl ThreadProvenance {
	#[doc(hidden)]
	pub fn from_protocol(value: impl Into<String>) -> Result<Self, &'static str> {
		let value = ZeroizingText::new(value.into());

		validate_bounded_text(
			value.as_str(),
			MAX_THREAD_PROVENANCE_BYTES,
			"Codex thread provenance is invalid",
		)?;

		if !value.as_str().starts_with("decodex.") {
			return Err("Codex thread provenance is not Decodex-owned");
		}

		Ok(Self(value))
	}

	/// Return the exact bounded provenance marker.
	pub fn as_str(&self) -> &str {
		self.0.as_str()
	}
}
impl Debug for ThreadProvenance {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str("ThreadProvenance([REDACTED])")
	}
}

/// Valid non-negative app-server creation timestamp.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ThreadCreatedAt(i64);
impl ThreadCreatedAt {
	#[doc(hidden)]
	pub const fn from_protocol(value: i64) -> Result<Self, &'static str> {
		if value < 0 {
			return Err("Codex thread creation timestamp is invalid");
		}

		Ok(Self(value))
	}

	/// Return Unix seconds reported by Codex.
	pub const fn unix_seconds(self) -> i64 {
		self.0
	}
}

/// Exact bounded facts available from list/read reconciliation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExactThreadFacts {
	/// Exact executable protocol identity.
	pub id: ExactThreadId,
	/// Optional retained Decodex thread-source marker.
	pub provenance: Option<ThreadProvenance>,
	/// App-server creation timestamp in Unix seconds.
	pub created_at: ThreadCreatedAt,
	/// Optional retained title.
	pub title: Option<ThreadTitle>,
	/// Exact bounded working directory.
	pub cwd: ThreadCwd,
	/// Current archived fact.
	pub archived: bool,
}

/// Bounded exact-list response.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExactThreadListResult {
	/// Matching exact thread facts, in app-server order.
	threads: Vec<ExactThreadFacts>,
}
impl ExactThreadListResult {
	#[doc(hidden)]
	pub fn from_protocol(threads: Vec<ExactThreadFacts>) -> Result<Self, &'static str> {
		if threads.len() > MAX_EXACT_THREAD_LIST_RESULTS {
			return Err("Codex exact thread list exceeds the result bound");
		}

		Ok(Self { threads })
	}

	/// Return the bounded exact results in app-server order.
	pub fn threads(&self) -> &[ExactThreadFacts] {
		&self.threads
	}
}

/// Explicit epistemic limit of `thread/read(includeTurns=true)`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LossyThreadHistory {
	/// Visible turns were requested, but the response is not complete-history or replay authority.
	IncludeTurnsReadback,
}

/// One exact submitted Turn correlated by the caller's stable user-message identity.
///
/// This observation is positive readback evidence only. Absence never proves non-submission and
/// never grants retry or replay authority.
#[derive(Clone, Eq, PartialEq)]
pub struct ExactSubmittedTurnReadback {
	provider_turn_id: ExactTurnId,
	status: ConversationTurnStatus,
	assistant_text: String,
	witness_digest: String,
}
impl ExactSubmittedTurnReadback {
	#[doc(hidden)]
	pub fn from_protocol(
		provider_turn_id: ExactTurnId,
		status: ConversationTurnStatus,
		assistant_text: String,
		witness_digest: String,
	) -> Result<Self, &'static str> {
		if assistant_text.len() > MAX_EXACT_TURN_ASSISTANT_BYTES
			|| witness_digest.len() != 64
			|| !witness_digest
				.bytes()
				.all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
		{
			return Err("Codex submitted Turn readback is invalid");
		}
		Ok(Self { provider_turn_id, status, assistant_text, witness_digest })
	}

	/// Exact provider Turn identity observed on the account-bound thread.
	pub fn provider_turn_id(&self) -> &ExactTurnId {
		&self.provider_turn_id
	}

	/// Current exact provider Turn state.
	pub const fn status(&self) -> ConversationTurnStatus {
		self.status
	}

	/// Ordered assistant message text retained by the exact readback.
	pub fn assistant_text(&self) -> &str {
		&self.assistant_text
	}

	/// SHA-256 witness of the bounded correlated observation.
	pub fn witness_digest(&self) -> &str {
		&self.witness_digest
	}
}
impl Debug for ExactSubmittedTurnReadback {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter
			.debug_struct("ExactSubmittedTurnReadback")
			.field("provider_turn_id", &self.provider_turn_id)
			.field("status", &self.status)
			.field("assistant_bytes", &self.assistant_text.len())
			.field("witness_digest", &self.witness_digest)
			.finish()
	}
}

/// Exact readback facts without a replay-authorizing history projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExactThreadReadResult {
	/// Exact bounded thread facts.
	pub facts: ExactThreadFacts,
	/// Mandatory lossy-history marker.
	pub history: LossyThreadHistory,
	/// Exact positive correlation for the requested client user-message identity, when present.
	pub submitted_turn: Option<ExactSubmittedTurnReadback>,
}

/// Closed reason an archive mutation could not be verified.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArchiveUnverifiedReason {
	/// The observed executable does not support `thread/archive`.
	MethodUnsupported,
	/// The mutation response was ambiguous and exact readback did not confirm the desired state.
	AmbiguousMutation,
	/// Exact readback was missing, malformed, mismatched, or contradicted the desired state.
	ReadbackFailed,
	/// The immutable account binding changed during reconciliation.
	AccountBindingChanged,
}

/// Same-process desired-state archive reconciliation outcome.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArchiveReconciliationOutcome {
	/// This call issued archive and exact readback confirmed the archived state.
	Archived,
	/// Exact pre-read showed the thread was already archived; no mutation was issued.
	AlreadyArchived,
	/// Desired state was not safely confirmed.
	Unverified(ArchiveUnverifiedReason),
}

/// Redacted read-only thread-list projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThreadSummary {
	/// Opaque thread identifier.
	pub id: ThreadId,
	/// Whether Codex reports this thread as archived.
	pub archived: bool,
	/// Opaque parent identifier for a run-local collaboration actor.
	pub parent_thread_id: Option<ThreadId>,
}

/// Private owned UTF-8 protected by the workspace's established zeroization authority.
///
/// The constructor is private and all exposed access remains borrowed. `Zeroizing` owns clearing
/// the allocation on drop, including independently cloned owners.
#[derive(Clone, Eq, PartialEq)]
struct ZeroizingText(Zeroizing<String>);
impl ZeroizingText {
	fn new(value: String) -> Self {
		Self(Zeroizing::new(value))
	}

	fn as_str(&self) -> &str {
		self.0.as_str()
	}
}
impl Ord for ZeroizingText {
	fn cmp(&self, other: &Self) -> Ordering {
		self.as_str().cmp(other.as_str())
	}
}
impl PartialOrd for ZeroizingText {
	fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
		Some(self.cmp(other))
	}
}
impl Serialize for ZeroizingText {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: Serializer,
	{
		serializer.serialize_str(self.as_str())
	}
}

fn validate_exact_thread_id(value: &str) -> Result<(), &'static str> {
	const MESSAGE: &str = "Codex thread ID is invalid";

	validate_bounded_text(value, MAX_EXACT_THREAD_ID_BYTES, MESSAGE)?;

	// The private inbound validator deliberately rejects JSON escape syntax before Serde can create
	// untracked scratch strings. Close the executable-ID domain at its public constructor so every
	// accepted ID has the same unescaped JSON representation in requests and readback responses.
	if value.contains(['"', '\\']) {
		return Err(MESSAGE);
	}

	Ok(())
}

fn validate_bounded_text(
	value: &str,
	max_bytes: usize,
	message: &'static str,
) -> Result<(), &'static str> {
	if value.is_empty()
		|| value.len() > max_bytes
		|| value.chars().any(|character| character.is_control())
	{
		return Err(message);
	}

	Ok(())
}

fn hex_digest(bytes: &[u8]) -> String {
	bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
	use std::cmp::Ordering;

	use crate::protocol::{
		DecodexThreadSearchTerm, ExactThreadId, MAX_EXACT_THREAD_ID_BYTES, ThreadCwd, ThreadId,
		ThreadProvenance, ThreadTitle, ZeroizingText,
	};

	#[test]
	fn exact_and_observation_identities_are_distinct_and_exact_debug_is_redacted() {
		let raw = "thread:non-uuid/Case-Sensitive_01";
		let exact = ExactThreadId::new(raw).unwrap();
		let observed = ThreadId::from_protocol(raw);

		assert_eq!(exact.as_str(), raw);
		assert_ne!(observed.as_str(), raw);
		assert_eq!(serde_json::to_string(&exact).unwrap(), format!("\"{raw}\""));
		assert_eq!(format!("{exact:?}"), "ExactThreadId([REDACTED])");
		assert!(!format!("{exact:?}").contains(raw));
	}

	#[test]
	fn exact_identifier_validation_is_bounded_without_normalization() {
		assert!(ExactThreadId::new("").is_err());
		assert!(ExactThreadId::new("line\nbreak").is_err());
		assert!(ExactThreadId::new("thread:\u{1b}escape").is_err());
		assert!(ExactThreadId::new("thread:\"quote").is_err());
		assert!(ExactThreadId::new(r"thread:\backslash").is_err());
		assert!(ExactThreadId::new("x".repeat(MAX_EXACT_THREAD_ID_BYTES + 1)).is_err());

		let boundary = "x".repeat(MAX_EXACT_THREAD_ID_BYTES);

		assert_eq!(ExactThreadId::new(boundary.clone()).unwrap().as_str(), boundary);

		let punctuation = "thread:MiXeD-id/._~:@+$,;=[]{}()!%&'*? #";
		let exact = ExactThreadId::new(punctuation).unwrap();

		assert_eq!(exact.as_str(), punctuation);
		assert_eq!(serde_json::to_string(&exact).unwrap(), format!("\"{punctuation}\""));
	}

	#[test]
	fn exact_fact_text_owners_clone_as_zeroizing_borrowed_values_and_keep_debug_redacted() {
		let raw = "thread:private-exact-id";
		let exact = ExactThreadId::new(raw).unwrap();
		let exact_clone = exact.clone();
		let title = ThreadTitle::from_protocol("Decodex private title").unwrap();
		let cwd = ThreadCwd::from_protocol("/tmp/private-repository").unwrap();
		let provenance = ThreadProvenance::from_protocol("decodex.private").unwrap();

		assert_eq!(exact_clone.as_str(), raw);
		assert_eq!(exact, exact_clone);
		assert_eq!(exact.cmp(&exact_clone), Ordering::Equal);
		assert_eq!(format!("{exact:?}"), "ExactThreadId([REDACTED])");
		assert!(!format!("{exact:?} {title:?} {cwd:?} {provenance:?}").contains("private"));
	}

	#[test]
	fn zeroizing_text_serializes_borrowed_utf8_without_exposing_debug() {
		let owner = ZeroizingText::new("exact-protocol-text".to_owned());

		assert_eq!(owner.as_str(), "exact-protocol-text");
		assert_eq!(serde_json::to_string(&owner).unwrap(), "\"exact-protocol-text\"");
	}

	#[test]
	fn list_and_read_text_facts_are_bounded_and_redacted() {
		let search = DecodexThreadSearchTerm::new("Decodex XY-1317 exact marker").unwrap();
		let title = ThreadTitle::from_protocol("Decodex XY-1317 exact marker").unwrap();
		let cwd = ThreadCwd::from_protocol("/tmp/private-repository").unwrap();
		let provenance = ThreadProvenance::from_protocol("decodex.xy1317.fixture").unwrap();

		assert!(DecodexThreadSearchTerm::new("arbitrary global title").is_err());
		assert_eq!(search.as_str(), "Decodex XY-1317 exact marker");
		assert_eq!(title.as_str(), search.as_str());
		assert_eq!(cwd.as_str(), "/tmp/private-repository");
		assert_eq!(provenance.as_str(), "decodex.xy1317.fixture");
		assert!(
			!format!("{search:?} {title:?} {cwd:?} {provenance:?}").contains("private-repository")
		);
	}
}
