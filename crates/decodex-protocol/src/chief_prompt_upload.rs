//! Complete native input transfer. Uploading data never authorizes inference.
use crate::{EntityId, IdempotencyKey, Sha256Digest, WireText};
use serde::{Deserialize, Serialize};

/// Exact source and content identity shared by every chunk and status read.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PromptInputUpload {
	/// Local task owner.
	pub work_id: EntityId,
	/// Native thread whose history was edited.
	pub thread_id: WireText,
	/// Durable applied-edit receipt.
	pub edit_receipt_id: i64,
	/// Client transfer identity; separate from a submission command identity.
	pub upload_id: IdempotencyKey,
	/// Digest of the complete compact JSON input array.
	pub sha256: Sha256Digest,
	/// Complete UTF-8 byte count, excluding the local wire envelope.
	pub total_bytes: u64,
}
impl PromptInputUpload {
	/// Check transfer bounds before any allocation or storage operation.
	pub fn is_valid(&self) -> bool {
		!self.thread_id.as_str().is_empty()
			&& self.edit_receipt_id > 0
			&& self.total_bytes > 0
			&& self.total_bytes <= decodex_core::MAX_NATIVE_MESSAGE_BYTES as u64
	}
}

/// Read-only transfer status. Complete bytes are not a submitted model turn.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum PromptInputUploadStatus {
	/// Durable chunks exist or a new transfer can start at zero.
	Receiving {
		/// Exact requested source.
		upload: PromptInputUpload,
		/// Durably accepted contiguous bytes. Finalization is a separate command.
		received_bytes: u64,
	},
	/// Complete immutable data is available for a later explicit send.
	Ready {
		/// Exact requested source.
		upload: PromptInputUpload,
		/// Immutable content record; not a submission receipt.
		input_id: i64,
	},
	/// No current source-bound evidence is available.
	Unavailable {
		/// Exact requested source.
		upload: PromptInputUpload,
	},
}
impl PromptInputUploadStatus {
	/// Borrow the complete source binding to reject crossed replies.
	pub fn upload(&self) -> &PromptInputUpload {
		match self {
			Self::Receiving { upload, .. }
			| Self::Ready { upload, .. }
			| Self::Unavailable { upload } => upload,
		}
	}

	/// Validate source and phase-specific bounds.
	pub fn is_valid(&self) -> bool {
		self.upload().is_valid()
			&& match self {
				Self::Receiving { upload, received_bytes } => *received_bytes <= upload.total_bytes,
				Self::Ready { input_id, .. } => *input_id > 0,
				Self::Unavailable { .. } => true,
			}
	}
}
