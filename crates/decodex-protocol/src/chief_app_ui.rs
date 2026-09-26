//! Exact native App UI document reads; executable content stays in the isolated view.
use serde::{Deserialize, Serialize};

/// Maximum decoded widget document size accepted by the native widget reader.
pub const MAX_CHIEF_APP_UI_BYTES: usize = 6 * 1024 * 1024;
/// Binary chunk size; its worst-case JSON representation fits the local wire bound.
pub const CHIEF_APP_UI_CHUNK_BYTES: usize = 32 * 1024;

/// One exact native widget document and continuation offset.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChiefAppUiRequest {
	/// Current task identity.
	pub work_id: crate::EntityId,
	/// Expected native thread binding.
	pub thread_id: crate::EntityId,
	/// Exact containing turn.
	pub turn_id: crate::EntityId,
	/// Exact containing item.
	pub item_id: crate::EntityId,
	/// Decoded byte offset. Zero starts a fresh read.
	pub offset: u32,
	/// Required content and source fingerprint for every nonzero offset.
	pub fingerprint: Option<crate::EntityId>,
}

/// Widget document bytes or an explicit unavailable/unsupported result.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ChiefAppUiResult {
	/// A bounded byte chunk from the exact source.
	Available {
		/// Echo of the exact requested widget document and offset.
		request: Box<ChiefAppUiRequest>,
		/// Account that owns this read.
		account_id: crate::EntityId,
		/// Opaque current account/process/history identity for lightweight validity reads.
		source_fingerprint: crate::EntityId,
		/// SHA-256 of the source binding and complete document bytes.
		fingerprint: crate::EntityId,
		/// Complete decoded content length.
		total_bytes: u32,
		/// Bytes starting at the requested offset.
		bytes: Vec<u8>,
	},
	/// The native source is not supported for content reads.
	Unsupported,
	/// The widget document exceeds the bounded reader capacity.
	CapacityExceeded,
	/// The item is absent, invalid, changed, or no longer owned by the source.
	Unavailable,
}
