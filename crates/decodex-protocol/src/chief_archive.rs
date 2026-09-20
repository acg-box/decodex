//! Native archive state is queried again after reconnect; it is not a local shadow flag.
use serde::{Deserialize, Serialize};

/// Current evidence for the selected work's exact native thread.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum ChiefArchiveResult {
	/// The exact thread appears in the active list only.
	Active {
		/// Native identity at the time of inspection.
		thread_id: String,
	},
	/// The exact thread appears in the archived list only.
	Archived {
		/// Native identity required for an explicit restore.
		thread_id: String,
	},
	/// This work has not acquired a native thread yet.
	Unbound,
	/// Native lists contain neither or both memberships; do not infer deletion.
	Unconfirmed,
	/// The provider cannot expose archive membership.
	Unsupported,
	/// Complete membership could not be determined within the inspection bound.
	CapacityExceeded,
	/// The owning process, binding or inspection is unavailable.
	Unavailable,
}
