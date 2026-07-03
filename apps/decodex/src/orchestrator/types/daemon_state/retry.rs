use crate::orchestrator::types::{
	HashMap, Instant, IssueDispatchMode, RECOVERABLE_WORKTREE_SKIP_TTL, RetryKind,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RetryEntryLifecycle {
	Active,
	ReviewRepair,
	Closeout,
}
impl RetryEntryLifecycle {
	pub(crate) const fn for_dispatch_mode(dispatch_mode: IssueDispatchMode) -> Self {
		match dispatch_mode {
			IssueDispatchMode::ReviewRepair => Self::ReviewRepair,
			IssueDispatchMode::Closeout => Self::Closeout,
			IssueDispatchMode::Normal | IssueDispatchMode::Program | IssueDispatchMode::Retry =>
				Self::Active,
		}
	}
}

#[derive(Clone, Debug)]
pub(crate) struct RetryEntry {
	pub(crate) issue_id: String,
	#[allow(dead_code)]
	#[cfg(test)]
	pub(crate) retry_project_slug: String,
	pub(crate) continuation_initial_issue_state: Option<String>,
	pub(crate) lifecycle: RetryEntryLifecycle,
	pub(crate) dispatch_mode: IssueDispatchMode,
	pub(crate) kind: RetryKind,
	pub(crate) attempt: u32,
	pub(crate) ready_at: Instant,
}

#[derive(Default)]
pub(crate) struct RetryQueue {
	pub(crate) entries: HashMap<String, RetryEntry>,
}
impl RetryQueue {
	pub(crate) fn is_empty(&self) -> bool {
		self.entries.is_empty()
	}

	pub(crate) fn upsert(&mut self, entry: RetryEntry) {
		self.entries.insert(entry.issue_id.clone(), entry);
	}

	pub(crate) fn release(&mut self, issue_id: &str) {
		self.entries.remove(issue_id);
	}

	pub(crate) fn next_entry(&self) -> Option<&RetryEntry> {
		self.entries.values().min_by(|left, right| {
			left.ready_at.cmp(&right.ready_at).then_with(|| left.issue_id.cmp(&right.issue_id))
		})
	}

	pub(crate) fn ordered_entries(&self) -> Vec<RetryEntry> {
		let mut entries = self.entries.values().cloned().collect::<Vec<_>>();

		entries.sort_by(|left, right| {
			left.ready_at.cmp(&right.ready_at).then_with(|| left.issue_id.cmp(&right.issue_id))
		});

		entries
	}
}

#[derive(Default)]
pub(crate) struct RecoverableWorktreeSkipCache {
	pub(crate) entries: HashMap<String, Instant>,
}
impl RecoverableWorktreeSkipCache {
	pub(crate) fn is_suppressed(&mut self, issue_identifier: &str, now: Instant) -> bool {
		self.retain_active(now);

		self.entries.get(&issue_identifier.to_ascii_uppercase()).is_some_and(|until| *until > now)
	}

	pub(crate) fn remember(&mut self, issue_identifier: &str, now: Instant) {
		self.retain_active(now);
		self.entries
			.insert(issue_identifier.to_ascii_uppercase(), now + RECOVERABLE_WORKTREE_SKIP_TTL);
	}

	pub(crate) fn retain_active(&mut self, now: Instant) {
		self.entries.retain(|_, until| *until > now);
	}
}
