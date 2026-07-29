//! Crate-private read-only WorkItem board queries.

use decodex_core::{ProjectId, WorkItemId, WorkItemState};
use decodex_postgres::{PostgresStore, StoreError, StoredWorkItem};

/// Read-only access to canonical WorkItem board pages.
pub(crate) struct WorkItemBoardQuery {
	store: PostgresStore,
}

impl WorkItemBoardQuery {
	/// Retain the canonical PostgreSQL product-state store.
	pub(crate) fn new(store: PostgresStore) -> Self {
		Self { store }
	}

	/// Read one deterministic Project lane/page after an optional WorkItem cursor.
	///
	/// The limit is forwarded unchanged so the store enforces its canonical bound.
	pub(crate) async fn page(
		&self,
		project_id: &ProjectId,
		state: Option<WorkItemState>,
		after: Option<&WorkItemId>,
		limit: usize,
	) -> Result<Vec<StoredWorkItem>, StoreError> {
		self.store.query_work_items(project_id, state, after, limit).await
	}
}
