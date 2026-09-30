//! Explicit Markdown transcript transfer. Each chunk belongs to one immutable export.
use serde::{Deserialize, Serialize};
/// Maximum complete Markdown document size.
pub const MAX_TRANSCRIPT_BYTES: usize = decodex_core::MAX_NATIVE_MESSAGE_BYTES;
/// Binary chunks fit the local JSON frame even with escaped bytes.
pub const TRANSCRIPT_CHUNK_BYTES: usize = 32 * 1024;
/// Exact conversation and export continuation.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentTranscriptRequest {
	/// Local task owner.
	pub work_id: crate::EntityId,
	/// Native conversation.
	pub thread_id: crate::EntityId,
	/// Byte offset in the immutable document.
	pub offset: u32,
	/// Token returned by the first chunk; required for continuation.
	pub token: Option<crate::EntityId>,
}
/// Complete-document chunks or an explicit read failure.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum AgentTranscriptResult {
	/// One chunk of a fully hydrated document.
	Available {
		/// Echo of the request.
		request: AgentTranscriptRequest,
		/// Account used to hydrate the document.
		account_id: crate::EntityId,
		/// Opaque export identity.
		token: crate::EntityId,
		/// Full document length.
		total_bytes: u32,
		/// Document bytes at the requested offset.
		bytes: Vec<u8>,
	},
	/// The document exceeds the supported export size.
	CapacityExceeded,
	/// Complete persisted history cannot be read from this source.
	Unavailable,
}
