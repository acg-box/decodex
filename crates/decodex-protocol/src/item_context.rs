//! A bounded item-history window with native directional continuations.
use serde::{Deserialize, Serialize};

use crate::{AgentTimelineContent, EntityId, WireText};

/// Exact known message and an optional native continuation within its turn.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct ItemContextRequest {
	/// Owning local conversation.
	pub work_id: EntityId,
	/// Exact native conversation.
	pub thread_id: EntityId,
	/// Turn in which the known item is visible.
	pub turn_id: WireText,
	/// Known native item. The first window includes this item.
	pub item_id: WireText,
	/// Native opaque continuation, not an offset or a locally reconstructed cursor.
	pub cursor: Option<WireText>,
	/// True reads earlier items; false reads later items.
	pub before: bool,
}

/// Read-only nearby text. These entries have no fabricated timeline positions.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ItemContextResult {
	/// Exact native item window in chronological order.
	Available {
		/// Echoed known item and page.
		request: ItemContextRequest,
		/// Retained visible item bodies and public activity facts.
		items: Vec<AgentTimelineContent>,
		/// Native continuation toward earlier items of this turn.
		previous_cursor: Option<WireText>,
		/// Native continuation toward later items of this turn.
		next_cursor: Option<WireText>,
	},
	/// This native server does not provide item anchors.
	Unsupported,
	/// The item, its source or the requested page is unavailable.
	Unavailable,
}
