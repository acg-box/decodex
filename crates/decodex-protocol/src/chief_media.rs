//! Exact native attachment reads. Paths and URLs are resolved only by the service.
use serde::{Deserialize, Serialize};

/// Maximum decoded attachment size accepted by the native media reader.
pub const MAX_CHIEF_MEDIA_BYTES: usize = 6 * 1024 * 1024;
/// Binary chunk size; its worst-case JSON representation fits the local wire bound.
pub const CHIEF_MEDIA_CHUNK_BYTES: usize = 32 * 1024;

/// One exact native attachment and continuation offset.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChiefMediaRequest {
	/// Current task identity.
	pub work_id: crate::EntityId,
	/// Expected native thread binding.
	pub thread_id: crate::EntityId,
	/// Exact containing turn.
	pub turn_id: crate::EntityId,
	/// Exact containing item.
	pub item_id: crate::EntityId,
	/// Original native content-array index, or zero for a standalone image item.
	pub index: u32,
	/// Decoded byte offset. Zero starts a fresh read.
	pub offset: u32,
	/// Required content and source fingerprint for every nonzero offset.
	pub fingerprint: Option<crate::EntityId>,
}

/// Attachment bytes or an explicit unavailable/unsupported result.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ChiefMediaResult {
	/// A bounded byte chunk from the exact source.
	Available {
		/// Echo of the exact requested attachment and offset.
		request: Box<ChiefMediaRequest>,
		/// Account that owns this read.
		account_id: crate::EntityId,
		/// SHA-256 of the source binding, media type and complete bytes.
		fingerprint: crate::EntityId,
		/// Content type for rendering; never an executable application payload.
		mime_type: String,
		/// Complete decoded content length.
		total_bytes: u32,
		/// Bytes starting at the requested offset.
		bytes: Vec<u8>,
	},
	/// The native source is not supported for content reads.
	Unsupported,
	/// The attachment exceeds the bounded reader capacity.
	CapacityExceeded,
	/// The item is absent, invalid, changed, or no longer owned by the source.
	Unavailable,
}
