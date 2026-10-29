//! Read-only reverse lookup of existing native resource associations.
use serde::{Deserialize, Serialize};

use crate::{EntityId, WireText};

/// Exact registered resource and native page selection.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct ResourceOwnersRequest {
	/// Local conversation from which the resource was selected.
	pub work_id: EntityId,
	/// Existing native resource type.
	pub attachment_type: WireText,
	/// Existing native resource identity; never derived from a filename.
	pub identity_key: WireText,
	/// None reads both archived and current conversations.
	pub archived: Option<bool>,
	/// Native continuation for this exact resource and archive filter.
	pub cursor: Option<WireText>,
}

/// A native association with optional known Decodex navigation and account facts.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct ResourceOwner {
	/// Exact native conversation.
	pub thread_id: WireText,
	/// Native archive membership.
	pub archived: bool,
	/// Existing local conversation, if known. Lookup does not create one.
	pub work_id: Option<EntityId>,
	/// Retained local title, or a bounded native reference.
	pub title: String,
	/// Retained account binding, not the account used to run this query.
	pub account_id: Option<EntityId>,
}

/// One source-bound native page. Missing support is distinct from an empty page.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ResourceOwnersResult {
	/// The native store returned this exact resource page.
	Available {
		/// Echoed resource and filter.
		request: ResourceOwnersRequest,
		/// Exact admitted process observation.
		source: EntityId,
		/// Current matching associations.
		owners: Vec<ResourceOwner>,
		/// Native continuation; absence completes this lookup.
		next_cursor: Option<WireText>,
	},
	/// This native store does not support reverse lookup.
	Unsupported,
	/// The requested page or its source could not be established.
	Unavailable,
}
