//! Presentation-neutral, bounded ConversationHistory paging for one GPUI view.

#[path = "history_pager/page_cache.rs"]
mod page_cache;

use std::{
	collections::VecDeque,
	path::{Path, PathBuf},
	sync::{Arc, Mutex, MutexGuard, TryLockError},
	time::{SystemTime, UNIX_EPOCH},
};

use tokio::sync::Notify;

use decodex_protocol::{
	CURRENT_VERSION, ConversationHistoryPage, ConversationHistoryResult, EntityId,
	HistoryCursorToken, HistoryQueryError, MAX_HISTORY_PAGE_SIZE, QueryEnvelope, QueryId,
	QueryPayload, QueryResultEnvelope, QueryResultPayload, ServerId,
};

use self::page_cache::{
	CacheAuthority, CacheDiagnostic, CacheFailure, CacheHit, CacheLookup, CachePublishResult,
	CacheRequest, CommittedCachePublication, HistoryPageCache, PreparedCachePublication,
};

const MAX_CANCELLED_REQUESTS: usize = 8;
const PRODUCTION_MAX_PAGE_BYTES: usize = 256 * 1_024;
const PRODUCTION_MAX_WINDOW_BYTES: usize = 1_024 * 1_024;
const PRODUCTION_MAX_WINDOW_ITEMS: usize = 32;
const PRODUCTION_MAX_WINDOW_PAGES: usize = 4;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct HistoryPagerLimits {
	max_page_bytes: usize,
	max_window_bytes: usize,
	max_window_items: usize,
	max_window_pages: usize,
}

impl HistoryPagerLimits {
	const fn production() -> Self {
		Self {
			max_page_bytes: PRODUCTION_MAX_PAGE_BYTES,
			max_window_bytes: PRODUCTION_MAX_WINDOW_BYTES,
			max_window_items: PRODUCTION_MAX_WINDOW_ITEMS,
			max_window_pages: PRODUCTION_MAX_WINDOW_PAGES,
		}
	}
}

/// Current presentation-neutral state for one Conversation view.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct HistorySnapshot {
	pub(crate) conversation_id: Option<EntityId>,
	pub(crate) view_generation: u64,
	pub(crate) load: HistoryLoadState,
	pub(crate) visible: Option<ConversationHistoryPage>,
	pub(crate) visible_source: Option<HistoryPageSource>,
	pub(crate) next_cursor: Option<HistoryCursorToken>,
	pub(crate) cursor: HistoryCursorObservation,
	pub(crate) cache_diagnostic: Option<HistoryCacheDiagnostic>,
	pub(crate) retained_pages: usize,
	pub(crate) retained_items: usize,
	pub(crate) retained_bytes: usize,
	pub(crate) can_show_previous: bool,
	pub(crate) can_show_next: bool,
	pub(crate) can_retry: bool,
	pub(crate) last_stale_cancellation: Option<HistoryStaleCancellation>,
}

/// Finite loading state. None of these states asserts that product history is complete.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HistoryLoadState {
	Inactive,
	InitialLoading,
	RefreshingVisible,
	PrefetchingAdjacent,
	Visible,
	RetryableUnavailable(HistoryRetryReason),
	ClosedUnavailable(HistoryClosedReason),
}

/// Origin of the visible bounded page.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HistoryPageSource {
	FreshServer,
	CachedUnverified,
}

/// Bounded local-cache observation with no product-content or credential meaning.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HistoryCacheDiagnostic {
	Unavailable,
}

#[cfg(test)]
const MAX_CACHE_PROBE_EVENTS: usize = 2;

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HistoryCacheProbeEvent {
	LookupStarted,
	PublicationStarted,
}

/// Latest page-level continuation observation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HistoryCursorObservation {
	Unknown,
	ContinuationAvailable,
	NoContinuationObserved,
}

/// Retryable failures that do not prove product absence or completion.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HistoryRetryReason {
	SessionUnavailable,
	ResourceExhausted,
	ProductStateUnavailable,
	IntegrityUnavailable,
}

/// Closed request-local failures. Opening a fresh view may still succeed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HistoryClosedReason {
	InvalidRequest,
	LocalBounds,
	MalformedContinuation,
	ProtocolMismatch,
	RequestIdentityExhausted,
}

/// One bounded stale request cancellation observation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct HistoryStaleCancellation {
	pub(crate) request_sequence: u64,
	pub(crate) reason: HistoryStaleReason,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HistoryStaleReason {
	ConversationChanged,
	NavigationChanged,
	SessionReplaced,
	ViewCancelled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HistoryNavigationResult {
	Moved,
	BoundaryUnknown,
	Inactive,
	GenerationExhausted,
}

/// Cloneable view controller. It owns no task, transport, or product authority.
#[derive(Clone)]
pub(crate) struct HistoryPager {
	inner: Arc<HistoryPagerInner>,
}

struct HistoryPagerInner {
	state: Mutex<PagerState>,
	page_cache: Mutex<PageCacheOwner>,
	// Publication lock order is page_cache -> commit gate -> state.
	cache_publication_commit_gate: Mutex<()>,
	cache_schema_generation: Option<u32>,
	#[cfg(test)]
	cache_probe_events: Mutex<Vec<HistoryCacheProbeEvent>>,
	notify: Notify,
}

enum PageCacheOwner {
	Dormant { parent: PathBuf, cache_schema_generation: u32 },
	Enabled(HistoryPageCache),
	Disabled,
}

impl PageCacheOwner {
	fn dormant(parent: &Path, cache_schema_generation: u32) -> Self {
		Self::Dormant { parent: parent.to_path_buf(), cache_schema_generation }
	}

	fn ensure_open(&mut self) -> bool {
		let dormant = match self {
			Self::Dormant { parent, cache_schema_generation } => {
				Some((parent.clone(), *cache_schema_generation))
			},
			Self::Enabled(_) => return true,
			Self::Disabled => return false,
		};
		let (parent, cache_schema_generation) =
			dormant.expect("dormant cache owner was just observed");
		match HistoryPageCache::open(&parent, cache_schema_generation) {
			Ok(cache) => {
				*self = Self::Enabled(cache);
				true
			},
			Err(failure) => {
				self.disable(failure);
				false
			},
		}
	}

	fn read_lookup(&mut self, identity: &CacheOperationIdentity) -> PageCacheLookupRead {
		let request = match identity.cache_request() {
			Ok(request) => request,
			Err(failure) => {
				self.disable(failure);
				return PageCacheLookupRead::Failure;
			},
		};
		let Some(now_unix_seconds) = current_unix_seconds() else {
			return PageCacheLookupRead::Failure;
		};
		if !self.ensure_open() {
			return PageCacheLookupRead::Failure;
		}
		let lookup = match self {
			Self::Enabled(cache) => cache.lookup(&request, now_unix_seconds),
			Self::Dormant { .. } | Self::Disabled => return PageCacheLookupRead::Failure,
		};

		match lookup {
			CacheLookup::Hit(hit) => PageCacheLookupRead::Hit(hit),
			CacheLookup::Miss(CacheDiagnostic::NotFound | CacheDiagnostic::Ineligible) => {
				PageCacheLookupRead::Miss
			},
			CacheLookup::Miss(diagnostic) => {
				self.disable(CacheFailure::new(diagnostic));
				PageCacheLookupRead::Failure
			},
			CacheLookup::Failure(failure) => {
				self.disable(failure);
				PageCacheLookupRead::Failure
			},
		}
	}

	fn complete_lookup(&mut self, lookup: PageCacheLookupRead) -> PageCacheLookupResult {
		match lookup {
			PageCacheLookupRead::Hit(hit) => {
				let recency_result = match self {
					Self::Enabled(cache) => cache.record_hit_recency(&hit),
					Self::Dormant { .. } | Self::Disabled => return PageCacheLookupResult::Failure,
				};
				if let Err(failure) = recency_result {
					self.disable(failure);
					return PageCacheLookupResult::Failure;
				}

				PageCacheLookupResult::Hit(hit.into_page())
			},
			PageCacheLookupRead::Miss => PageCacheLookupResult::Miss,
			PageCacheLookupRead::Failure => PageCacheLookupResult::Failure,
		}
	}

	fn prepare_publication(
		&mut self,
		publication: &CachePublication,
	) -> Result<PreparedCachePublication, ()> {
		let request = match publication.identity.cache_request() {
			Ok(request) => request,
			Err(failure) => {
				self.disable(failure);
				return Err(());
			},
		};
		let Some(admitted_at_unix_seconds) = publication.admitted_at_unix_seconds else {
			return Err(());
		};
		if !self.ensure_open() {
			return Err(());
		}
		let result = match self {
			Self::Enabled(cache) => {
				cache.prepare_publication(&request, &publication.page, admitted_at_unix_seconds)
			},
			Self::Dormant { .. } | Self::Disabled => return Err(()),
		};

		match result {
			Ok(prepared) => Ok(prepared),
			Err(failure) => {
				self.disable(failure);
				Err(())
			},
		}
	}

	fn commit_publication(&mut self, prepared: PreparedCachePublication) -> PageCacheCommitResult {
		let mut cache = match std::mem::replace(self, Self::Disabled) {
			Self::Enabled(cache) => cache,
			owner => {
				*self = owner;
				unreachable!("prepared publication requires an enabled page cache owner");
			},
		};

		match cache.commit_publication(prepared) {
			Ok(committed) => {
				*self = Self::Enabled(cache);
				PageCacheCommitResult::Committed(committed)
			},
			Err((prepared, failure)) => {
				self.disable(failure.clone());
				PageCacheCommitResult::Failed(cache, prepared, failure)
			},
		}
	}

	fn discard_stale(&mut self, prepared: PreparedCachePublication) {
		let result = match self {
			Self::Enabled(cache) => cache.discard_prepared_publication(prepared),
			Self::Dormant { .. } | Self::Disabled => return,
		};
		if let Err(failure) = result {
			self.disable(failure);
		}
	}

	fn discard_failed(
		&mut self,
		cache: HistoryPageCache,
		prepared: PreparedCachePublication,
		failure: CacheFailure,
	) {
		let cleanup_failure = cache.discard_prepared_publication(prepared).err();

		self.disable(cleanup_failure.unwrap_or(failure));
	}

	fn finish_publication(
		&mut self,
		committed: CommittedCachePublication,
	) -> PageCachePublishResult {
		let result = match self {
			Self::Enabled(cache) => cache.finish_publication(committed),
			Self::Dormant { .. } | Self::Disabled => return PageCachePublishResult::Failure,
		};

		match result {
			Ok(CachePublishResult::Published | CachePublishResult::Reinitialized) => {
				PageCachePublishResult::Stored
			},
			Err(failure) => {
				self.disable(failure);
				PageCachePublishResult::Failure
			},
		}
	}

	fn disable(&mut self, _failure: CacheFailure) {
		*self = Self::Disabled;
	}
}

impl HistoryPager {
	pub(crate) fn production(parent: &Path, cache_schema_generation: u32) -> Self {
		Self::with_page_cache(
			HistoryPagerLimits::production(),
			PageCacheOwner::dormant(parent, cache_schema_generation),
			Some(cache_schema_generation),
		)
	}

	fn new(limits: HistoryPagerLimits) -> Self {
		Self::with_page_cache(limits, PageCacheOwner::Disabled, None)
	}

	fn with_page_cache(
		limits: HistoryPagerLimits,
		page_cache: PageCacheOwner,
		cache_schema_generation: Option<u32>,
	) -> Self {
		Self {
			inner: Arc::new(HistoryPagerInner {
				state: Mutex::new(PagerState::new(limits)),
				page_cache: Mutex::new(page_cache),
				cache_publication_commit_gate: Mutex::new(()),
				cache_schema_generation,
				#[cfg(test)]
				cache_probe_events: Mutex::new(Vec::new()),
				notify: Notify::new(),
			}),
		}
	}

	/// Start a fresh view and cancel every result bound to the previous view.
	pub(crate) fn open(&self, conversation_id: EntityId) -> Result<(), HistoryClosedReason> {
		let _commit_gate = self.lock_cache_publication_commit_gate();
		let mut state = self.lock();
		let generation =
			state.next_view_generation().ok_or(HistoryClosedReason::RequestIdentityExhausted)?;

		state.cancel_in_flight(HistoryStaleReason::ConversationChanged);
		state.active = Some(ActiveView::new(conversation_id, generation));
		drop(state);
		self.inner.notify.notify_one();

		Ok(())
	}

	/// Reload the first bounded page only when the terminal event belongs to the open view.
	pub(crate) fn reload_if_open(
		&self,
		conversation_id: &EntityId,
	) -> Result<bool, HistoryClosedReason> {
		let _commit_gate = self.lock_cache_publication_commit_gate();
		let mut state = self.lock();
		if !state.active.as_ref().is_some_and(|active| &active.conversation_id == conversation_id) {
			return Ok(false);
		}
		let generation =
			state.next_view_generation().ok_or(HistoryClosedReason::RequestIdentityExhausted)?;
		state.cancel_in_flight(HistoryStaleReason::ConversationChanged);
		state
			.active
			.as_mut()
			.expect("open Conversation was just observed")
			.refresh_initial(generation);
		drop(state);
		self.inner.notify.notify_one();
		Ok(true)
	}

	/// Move to the next retained page or request the exact observed continuation.
	pub(crate) fn show_next(&self) -> HistoryNavigationResult {
		let _commit_gate = self.lock_cache_publication_commit_gate();
		let mut state = self.lock();
		let Some(active) = state.active.as_ref() else {
			return HistoryNavigationResult::Inactive;
		};
		if matches!(active.unavailable, Some(HistoryAvailability::Closed(_))) {
			return HistoryNavigationResult::BoundaryUnknown;
		}
		let Some(visible_index) = active.visible_index else {
			return HistoryNavigationResult::BoundaryUnknown;
		};
		let retained_next =
			visible_index.checked_add(1).filter(|index| *index < active.pages.len());
		let continuation = active.pages[visible_index].page.next_cursor.clone();

		if retained_next.is_none() && continuation.is_none() {
			return HistoryNavigationResult::BoundaryUnknown;
		}
		let conversation_id = active.conversation_id.clone();
		let Some(generation) = state.next_view_generation() else {
			state.set_closed(HistoryClosedReason::RequestIdentityExhausted);

			return HistoryNavigationResult::GenerationExhausted;
		};

		state.cancel_in_flight(HistoryStaleReason::NavigationChanged);
		let active = state.active.as_mut().expect("active view was just observed");

		active.generation = generation;
		active.unavailable = None;
		active.retry_request = None;
		active.clear_cache_presentation();
		active.cache_lookup_armed = None;
		active.cache_publication_fence = None;
		if let Some(next_index) = retained_next {
			active.visible_index = Some(next_index);
			active.pending = None;
			active.enqueue_adjacent_prefetch();
		} else if let Some(after) = continuation {
			let request = PageRequest::new(
				generation,
				PageKey::new(conversation_id, Some(after)),
				RequestPurpose::Visible,
			);

			active.pending = Some(request.clone());
			active.cache_lookup_armed = Some(request);
		}
		drop(state);
		self.inner.notify.notify_one();

		HistoryNavigationResult::Moved
	}

	/// Move to the previous retained page. Evicted history remains unknown.
	pub(crate) fn show_previous(&self) -> HistoryNavigationResult {
		let _commit_gate = self.lock_cache_publication_commit_gate();
		let mut state = self.lock();
		let Some(active) = state.active.as_ref() else {
			return HistoryNavigationResult::Inactive;
		};
		if matches!(active.unavailable, Some(HistoryAvailability::Closed(_))) {
			return HistoryNavigationResult::BoundaryUnknown;
		}
		let Some(previous_index) = active.visible_index.and_then(|index| index.checked_sub(1))
		else {
			return HistoryNavigationResult::BoundaryUnknown;
		};
		let Some(generation) = state.next_view_generation() else {
			state.set_closed(HistoryClosedReason::RequestIdentityExhausted);

			return HistoryNavigationResult::GenerationExhausted;
		};

		state.cancel_in_flight(HistoryStaleReason::NavigationChanged);
		let active = state.active.as_mut().expect("active view was just observed");

		active.generation = generation;
		active.visible_index = Some(previous_index);
		active.pending = None;
		active.unavailable = None;
		active.retry_request = None;
		active.clear_cache_presentation();
		active.cache_lookup_armed = None;
		active.cache_publication_fence = None;
		active.enqueue_adjacent_prefetch();
		drop(state);
		self.inner.notify.notify_one();

		HistoryNavigationResult::Moved
	}

	/// Retry only the exact request retained by the latest retryable failure.
	pub(crate) fn retry(&self) -> bool {
		let mut state = self.lock();
		let Some(active) = state.active.as_mut() else {
			return false;
		};
		let Some(request) = active.retry_request.take() else {
			return false;
		};

		active.pending = Some(request);
		active.cache_lookup_armed = None;
		active.unavailable = None;
		drop(state);
		self.inner.notify.notify_one();

		true
	}

	/// Cancel the current view without claiming that its Conversation is absent.
	pub(crate) fn cancel(&self) {
		let _commit_gate = self.lock_cache_publication_commit_gate();
		let mut state = self.lock();

		state.cancel_in_flight(HistoryStaleReason::ViewCancelled);
		state.active = None;
		drop(state);
		self.inner.notify.notify_one();
	}

	pub(crate) fn snapshot(&self) -> HistorySnapshot {
		self.lock().snapshot()
	}

	pub(crate) fn dispatch_is_current(&self, dispatch: &HistoryDispatch) -> bool {
		self.lock().matches_dispatch(dispatch)
	}

	/// Atomically transfer one current request into the transport-send phase.
	pub(crate) fn begin_send(&self, dispatch: &HistoryDispatch) -> Option<HistorySendToken> {
		let mut state = self.lock();

		if state.send_in_flight.is_some() || !state.matches_dispatch(dispatch) {
			return None;
		}

		let token = HistorySendToken::from_dispatch(dispatch);

		state.send_in_flight = Some(token.clone());

		Some(token)
	}

	/// Release one exact transport-send phase without restoring superseded authority.
	pub(crate) fn finish_send(&self, token: &HistorySendToken) -> bool {
		let mut state = self.lock();

		if state.send_in_flight.as_ref() != Some(token) {
			return false;
		}

		state.send_in_flight = None;
		let remains_current = state
			.active
			.as_ref()
			.and_then(|active| active.in_flight.as_ref())
			.is_some_and(|in_flight| in_flight.matches_send_token(token));
		drop(state);
		self.inner.notify.notify_one();

		remains_current
	}

	/// Perform caller-owned cache lookup only after one exact query was sent successfully.
	pub(crate) fn lookup_sent_request(&self, token: &HistorySendToken) {
		let Some(cache_schema_generation) = self.inner.cache_schema_generation else {
			return;
		};
		let identity = {
			let mut state = self.lock();
			let Some(identity) = state.take_sent_cache_lookup(token, cache_schema_generation)
			else {
				return;
			};

			identity
		};
		let Some(mut page_cache) = self.try_lock_page_cache() else {
			return;
		};
		#[cfg(test)]
		self.record_cache_probe_event(HistoryCacheProbeEvent::LookupStarted);
		let lookup = page_cache.read_lookup(&identity);
		let mut state = self.lock();

		if self.inner.cache_schema_generation != Some(identity.cache_schema_generation)
			|| !state.matches_cache_lookup(&identity)
		{
			return;
		}

		let result = page_cache.complete_lookup(lookup);
		let limits = state.limits;
		let active = state.active.as_mut().expect("cache lookup matched an active view");
		let changed = match (identity.request.purpose, result) {
			(
				RequestPurpose::Initial | RequestPurpose::Visible,
				PageCacheLookupResult::Hit(page),
			) => {
				active.admit_provisional_page(identity.request.clone(), page, limits);
				true
			},
			(_, PageCacheLookupResult::Failure) => {
				active.provisional = None;
				active.cache_diagnostic = Some(HistoryCacheDiagnostic::Unavailable);
				true
			},
			(_, PageCacheLookupResult::Hit(_) | PageCacheLookupResult::Miss) => false,
		};
		drop(state);
		drop(page_cache);
		if changed {
			self.inner.notify.notify_one();
		}
	}

	/// Bind future dispatch to one retained-session generation and stable server.
	pub(crate) fn bind_session(&self, generation: u64, server_id: ServerId) {
		let _commit_gate = self.lock_cache_publication_commit_gate();
		let mut state = self.lock();
		let binding = SessionBinding { generation, server_id };

		if state.session.as_ref() != Some(&binding) {
			state.cancel_in_flight(HistoryStaleReason::SessionReplaced);
			if let Some(active) = state.active.as_mut() {
				active.invalidate_session_authority(None);
			}
			state.session = Some(binding);
		}
		drop(state);
		self.inner.notify.notify_one();
	}

	/// Invalidate every page and cursor observation when the owning session ends.
	pub(crate) fn session_ended(&self, generation: u64) {
		let _commit_gate = self.lock_cache_publication_commit_gate();
		let mut state = self.lock();

		if state.session.as_ref().is_some_and(|session| session.generation == generation) {
			state.cancel_in_flight(HistoryStaleReason::SessionReplaced);
			state.session = None;
			if let Some(active) = state.active.as_mut() {
				active.invalidate_session_authority(Some(HistoryAvailability::Retryable(
					HistoryRetryReason::SessionUnavailable,
				)));
			}
		}
		drop(state);
		self.inner.notify.notify_one();
	}

	/// Wait for and reserve one exact outbound request. No request queue is retained.
	pub(crate) async fn next_dispatch(
		&self,
		session_generation: u64,
		server_id: &ServerId,
	) -> HistoryDispatch {
		loop {
			let notified = self.inner.notify.notified();

			if let Some(dispatch) = self.try_take_dispatch(session_generation, server_id) {
				return dispatch;
			}

			notified.await;
		}
	}

	fn try_take_dispatch(
		&self,
		session_generation: u64,
		server_id: &ServerId,
	) -> Option<HistoryDispatch> {
		let _commit_gate = self.lock_cache_publication_commit_gate();
		let mut state = self.lock();
		let expected =
			SessionBinding { generation: session_generation, server_id: server_id.clone() };

		if state.session.as_ref() != Some(&expected) {
			return None;
		}
		if state.send_in_flight.is_some() {
			return None;
		}
		if state.active.as_ref()?.in_flight.is_some() {
			return None;
		}

		let request = state.active.as_mut()?.pending.take()?;
		let request_sequence = match state.next_request_sequence.checked_add(1) {
			Some(sequence) => {
				state.next_request_sequence = sequence;
				sequence
			},
			None => {
				state.set_closed(HistoryClosedReason::RequestIdentityExhausted);

				return None;
			},
		};
		let query_id = QueryId::new(format!(
			"gpui-history/{session_generation}/{}/{request_sequence}",
			request.view_generation
		))
		.expect("bounded numeric query identity");
		let envelope = QueryEnvelope {
			version: CURRENT_VERSION,
			query_id: query_id.clone(),
			payload: QueryPayload::GetConversationHistory {
				conversation_id: request.key.conversation_id.clone(),
				after: request.key.after.clone(),
				page_size: MAX_HISTORY_PAGE_SIZE,
			},
		};
		let dispatch = HistoryDispatch {
			envelope,
			session_generation,
			server_id: server_id.clone(),
			request_sequence,
			request: request.clone(),
		};
		let active = state.active.as_mut().expect("active view owns pending request");

		active.in_flight = Some(InFlightRequest::from_dispatch(&dispatch));
		active.unavailable = None;

		Some(dispatch)
	}

	/// Route one query result through the exact request/session/server/cursor binding.
	pub(crate) fn route_result(
		&self,
		session_generation: u64,
		server_id: &ServerId,
		result: QueryResultEnvelope,
	) -> HistoryRouteOutcome {
		let commit_gate = self.lock_cache_publication_commit_gate();
		let mut state = self.lock();
		let Some(active_query) =
			state.active.as_ref().and_then(|active| active.in_flight.as_ref()).map(|request| {
				(request.query_id.clone(), request.session_generation, request.server_id.clone())
			})
		else {
			return if result.version == CURRENT_VERSION
				&& result.server_id == *server_id
				&& state.is_cancelled_query(&result.query_id, session_generation, &result.server_id)
			{
				HistoryRouteOutcome::Stale
			} else {
				HistoryRouteOutcome::Unmatched
			};
		};

		if active_query.0 != result.query_id {
			return if result.version == CURRENT_VERSION
				&& result.server_id == *server_id
				&& state.is_cancelled_query(&result.query_id, session_generation, &result.server_id)
			{
				HistoryRouteOutcome::Stale
			} else {
				HistoryRouteOutcome::Unmatched
			};
		}
		if result.version != CURRENT_VERSION
			|| result.server_id != *server_id
			|| active_query.1 != session_generation
			|| active_query.2 != *server_id
		{
			state.set_closed(HistoryClosedReason::ProtocolMismatch);

			return HistoryRouteOutcome::ProtocolMismatch;
		}

		let in_flight = state
			.active
			.as_mut()
			.and_then(|active| active.in_flight.take())
			.expect("active query was just observed");
		let cache_identity = self
			.inner
			.cache_schema_generation
			.map(|generation| CacheOperationIdentity::new(&in_flight, generation));
		let active = state.active.as_mut().expect("in-flight request has an active view");

		active.cache_lookup_armed = None;
		active.cache_publication_fence = None;
		active.clear_provisional_cache_page();
		let QueryResultPayload::ConversationHistory(history) = result.payload else {
			state.set_closed(HistoryClosedReason::ProtocolMismatch);

			return HistoryRouteOutcome::ProtocolMismatch;
		};

		match history {
			ConversationHistoryResult::Unavailable { error } => {
				let availability = history_availability(error);
				let active = state.active.as_mut().expect("in-flight request has an active view");

				active.unavailable = Some(availability);
				active.retry_request = matches!(availability, HistoryAvailability::Retryable(_))
					.then_some(in_flight.request);

				HistoryRouteOutcome::Unavailable
			},
			ConversationHistoryResult::Page(page) => {
				let limits = state.limits;
				let request = in_flight.request;
				let active = state.active.as_mut().expect("in-flight request has an active view");
				let publication_page = page.clone();

				if let Err(reason) = active.admit_live_page(request.clone(), page, limits) {
					active.unavailable = Some(HistoryAvailability::Closed(reason));
					active.retry_request = None;

					return HistoryRouteOutcome::Closed;
				}

				active.unavailable = None;
				active.retry_request = None;
				active.cache_publication_fence = cache_identity.clone();
				if request.purpose != RequestPurpose::Prefetch {
					active.enqueue_adjacent_prefetch();
				}

				let notify_follow_on = active.pending.is_some();
				let publication = cache_identity.map(|identity| CachePublication {
					identity,
					page: publication_page,
					admitted_at_unix_seconds: current_unix_seconds(),
				});
				drop(state);
				drop(commit_gate);
				if notify_follow_on {
					self.inner.notify.notify_one();
				}
				if let Some(publication) = publication {
					self.publish_fresh_page(publication);
				}

				HistoryRouteOutcome::Fresh
			},
		}
	}

	fn publish_fresh_page(&self, publication: CachePublication) {
		let identity = publication.identity.clone();
		let Some(mut page_cache) = self.try_lock_page_cache() else {
			self.complete_cache_publication(&identity, PageCachePublishResult::Skipped);

			return;
		};
		#[cfg(test)]
		self.record_cache_probe_event(HistoryCacheProbeEvent::PublicationStarted);
		let prepared = match page_cache.prepare_publication(&publication) {
			Ok(prepared) => prepared,
			Err(()) => {
				let commit_gate = self.lock_cache_publication_commit_gate();
				let mut state = self.lock();
				let remains_current = self.inner.cache_schema_generation
					== Some(identity.cache_schema_generation)
					&& state.matches_cache_publication(&identity);

				if remains_current {
					let active = state.active.as_mut().expect("publication matched an active view");

					active.cache_publication_fence = None;
					active.cache_diagnostic = Some(HistoryCacheDiagnostic::Unavailable);
				}
				drop(state);
				drop(commit_gate);
				drop(page_cache);

				return;
			},
		};

		let commit_gate = self.lock_cache_publication_commit_gate();
		let state = self.lock();
		if self.inner.cache_schema_generation != Some(identity.cache_schema_generation)
			|| !state.matches_cache_publication(&identity)
		{
			drop(state);
			drop(commit_gate);
			page_cache.discard_stale(prepared);

			return;
		}
		drop(state);
		let commit_result = page_cache.commit_publication(prepared);
		drop(commit_gate);

		let publish_result = match commit_result {
			PageCacheCommitResult::Committed(committed) => page_cache.finish_publication(committed),
			PageCacheCommitResult::Failed(cache, prepared, failure) => {
				page_cache.discard_failed(cache, prepared, failure);
				PageCachePublishResult::Failure
			},
		};
		drop(page_cache);
		self.complete_cache_publication(&identity, publish_result);
	}

	fn complete_cache_publication(
		&self,
		identity: &CacheOperationIdentity,
		result: PageCachePublishResult,
	) {
		let _commit_gate = self.lock_cache_publication_commit_gate();
		let mut state = self.lock();

		if self.inner.cache_schema_generation != Some(identity.cache_schema_generation)
			|| !state.matches_cache_publication(identity)
		{
			return;
		}
		let active = state.active.as_mut().expect("publication matched an active view");

		active.cache_publication_fence = None;
		match result {
			PageCachePublishResult::Stored => active.cache_diagnostic = None,
			PageCachePublishResult::Failure => {
				active.cache_diagnostic = Some(HistoryCacheDiagnostic::Unavailable)
			},
			PageCachePublishResult::Skipped => {},
		}
	}

	fn lock(&self) -> MutexGuard<'_, PagerState> {
		self.inner.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
	}

	fn try_lock_page_cache(&self) -> Option<MutexGuard<'_, PageCacheOwner>> {
		match self.inner.page_cache.try_lock() {
			Ok(cache) => Some(cache),
			Err(TryLockError::WouldBlock) => None,
			Err(TryLockError::Poisoned(error)) => Some(error.into_inner()),
		}
	}

	fn lock_cache_publication_commit_gate(&self) -> MutexGuard<'_, ()> {
		self.inner
			.cache_publication_commit_gate
			.lock()
			.unwrap_or_else(std::sync::PoisonError::into_inner)
	}

	#[cfg(test)]
	pub(crate) fn cache_probe_events(&self) -> Vec<HistoryCacheProbeEvent> {
		self.inner
			.cache_probe_events
			.lock()
			.unwrap_or_else(std::sync::PoisonError::into_inner)
			.clone()
	}

	#[cfg(test)]
	fn record_cache_probe_event(&self, event: HistoryCacheProbeEvent) {
		let mut events =
			self.inner.cache_probe_events.lock().unwrap_or_else(std::sync::PoisonError::into_inner);

		if events.len() < MAX_CACHE_PROBE_EVENTS {
			events.push(event);
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CacheOperationIdentity {
	view_generation: u64,
	session_generation: u64,
	server_id: ServerId,
	protocol_major: u16,
	protocol_minor: u16,
	cache_schema_generation: u32,
	query_id: QueryId,
	request_sequence: u64,
	request: PageRequest,
}

impl CacheOperationIdentity {
	fn new(in_flight: &InFlightRequest, cache_schema_generation: u32) -> Self {
		Self {
			view_generation: in_flight.request.view_generation,
			session_generation: in_flight.session_generation,
			server_id: in_flight.server_id.clone(),
			protocol_major: CURRENT_VERSION.major,
			protocol_minor: CURRENT_VERSION.minor,
			cache_schema_generation,
			query_id: in_flight.query_id.clone(),
			request_sequence: in_flight.request_sequence,
			request: in_flight.request.clone(),
		}
	}

	fn cache_request(&self) -> Result<CacheRequest, CacheFailure> {
		let authority = CacheAuthority::new(
			self.server_id.clone(),
			self.protocol_major,
			self.protocol_minor,
			self.cache_schema_generation,
		)?;

		match self.request.key.after.clone() {
			Some(after) => {
				CacheRequest::after(&authority, self.request.key.conversation_id.clone(), after)
			},
			None => CacheRequest::head(&authority, self.request.key.conversation_id.clone()),
		}
	}
}

struct CachePublication {
	identity: CacheOperationIdentity,
	page: ConversationHistoryPage,
	admitted_at_unix_seconds: Option<i64>,
}

enum PageCacheLookupRead {
	Hit(CacheHit),
	Miss,
	Failure,
}

enum PageCacheLookupResult {
	Hit(ConversationHistoryPage),
	Miss,
	Failure,
}

enum PageCacheCommitResult {
	Committed(CommittedCachePublication),
	Failed(HistoryPageCache, PreparedCachePublication, CacheFailure),
}

enum PageCachePublishResult {
	Stored,
	Skipped,
	Failure,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct HistoryDispatch {
	envelope: QueryEnvelope,
	session_generation: u64,
	server_id: ServerId,
	request_sequence: u64,
	request: PageRequest,
}

impl HistoryDispatch {
	pub(crate) const fn envelope(&self) -> &QueryEnvelope {
		&self.envelope
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct HistorySendToken {
	query_id: QueryId,
	session_generation: u64,
	server_id: ServerId,
	request_sequence: u64,
}

impl HistorySendToken {
	fn from_dispatch(dispatch: &HistoryDispatch) -> Self {
		Self {
			query_id: dispatch.envelope.query_id.clone(),
			session_generation: dispatch.session_generation,
			server_id: dispatch.server_id.clone(),
			request_sequence: dispatch.request_sequence,
		}
	}
}

pub(crate) enum HistoryRouteOutcome {
	Fresh,
	Unavailable,
	Closed,
	Stale,
	Unmatched,
	ProtocolMismatch,
}

struct PagerState {
	limits: HistoryPagerLimits,
	next_view_generation: u64,
	next_request_sequence: u64,
	session: Option<SessionBinding>,
	send_in_flight: Option<HistorySendToken>,
	active: Option<ActiveView>,
	cancelled: VecDeque<CancelledRequest>,
	last_stale_cancellation: Option<HistoryStaleCancellation>,
}

impl PagerState {
	fn new(limits: HistoryPagerLimits) -> Self {
		Self {
			limits,
			next_view_generation: 0,
			next_request_sequence: 0,
			session: None,
			send_in_flight: None,
			active: None,
			cancelled: VecDeque::new(),
			last_stale_cancellation: None,
		}
	}

	fn next_view_generation(&mut self) -> Option<u64> {
		let next = self.next_view_generation.checked_add(1)?;

		self.next_view_generation = next;

		Some(next)
	}

	fn cancel_in_flight(&mut self, reason: HistoryStaleReason) {
		let in_flight = self.active.as_mut().and_then(|active| active.in_flight.take());
		let Some(in_flight) = in_flight else {
			return;
		};

		self.last_stale_cancellation =
			Some(HistoryStaleCancellation { request_sequence: in_flight.request_sequence, reason });
		self.cancelled.push_back(CancelledRequest {
			query_id: in_flight.query_id,
			session_generation: in_flight.session_generation,
			server_id: in_flight.server_id,
		});
		while self.cancelled.len() > MAX_CANCELLED_REQUESTS {
			self.cancelled.pop_front();
		}
	}

	fn matches_dispatch(&self, dispatch: &HistoryDispatch) -> bool {
		self.active
			.as_ref()
			.and_then(|active| active.in_flight.as_ref())
			.is_some_and(|in_flight| in_flight.matches(dispatch))
	}

	fn is_cancelled_query(
		&self,
		query_id: &QueryId,
		session_generation: u64,
		server_id: &ServerId,
	) -> bool {
		self.cancelled.iter().any(|request| {
			&request.query_id == query_id
				&& request.session_generation == session_generation
				&& &request.server_id == server_id
		})
	}

	fn set_closed(&mut self, reason: HistoryClosedReason) {
		if let Some(active) = self.active.as_mut() {
			active.clear_cache_presentation();
			active.cache_lookup_armed = None;
			active.cache_publication_fence = None;
			active.unavailable = Some(HistoryAvailability::Closed(reason));
			active.pending = None;
			active.in_flight = None;
			active.retry_request = None;
		}
	}

	fn take_sent_cache_lookup(
		&mut self,
		token: &HistorySendToken,
		cache_schema_generation: u32,
	) -> Option<CacheOperationIdentity> {
		let session = self.session.as_ref()?;
		let active = self.active.as_mut()?;
		let in_flight = active.in_flight.as_ref()?;

		if !in_flight.matches_send_token(token)
			|| active.cache_lookup_armed.as_ref() != Some(&in_flight.request)
			|| active.generation != in_flight.request.view_generation
			|| session.generation != in_flight.session_generation
			|| session.server_id != in_flight.server_id
		{
			return None;
		}
		let identity = CacheOperationIdentity::new(in_flight, cache_schema_generation);

		active.cache_lookup_armed = None;

		Some(identity)
	}

	fn matches_cache_identity(&self, identity: &CacheOperationIdentity) -> bool {
		if CURRENT_VERSION.major != identity.protocol_major
			|| CURRENT_VERSION.minor != identity.protocol_minor
		{
			return false;
		}
		let Some(session) = self.session.as_ref() else {
			return false;
		};
		let Some(active) = self.active.as_ref() else {
			return false;
		};

		session.generation == identity.session_generation
			&& session.server_id == identity.server_id
			&& active.generation == identity.view_generation
			&& active.conversation_id == identity.request.key.conversation_id
	}

	fn matches_cache_lookup(&self, identity: &CacheOperationIdentity) -> bool {
		self.matches_cache_identity(identity)
			&& self
				.active
				.as_ref()
				.and_then(|active| active.in_flight.as_ref())
				.is_some_and(|in_flight| in_flight.matches_cache_identity(identity))
	}

	fn matches_cache_publication(&self, identity: &CacheOperationIdentity) -> bool {
		self.matches_cache_identity(identity)
			&& self
				.active
				.as_ref()
				.is_some_and(|active| active.cache_publication_fence.as_ref() == Some(identity))
	}

	fn snapshot(&self) -> HistorySnapshot {
		let Some(active) = self.active.as_ref() else {
			return HistorySnapshot {
				conversation_id: None,
				view_generation: self.next_view_generation,
				load: HistoryLoadState::Inactive,
				visible: None,
				visible_source: None,
				next_cursor: None,
				cursor: HistoryCursorObservation::Unknown,
				cache_diagnostic: None,
				retained_pages: 0,
				retained_items: 0,
				retained_bytes: 0,
				can_show_previous: false,
				can_show_next: false,
				can_retry: false,
				last_stale_cancellation: self.last_stale_cancellation,
			};
		};
		let fresh_visible = active.merged_visible_page();
		let provisional = active
			.provisional
			.as_ref()
			.filter(|page| page.request.purpose != RequestPurpose::Prefetch);
		let (visible, visible_source, cursor) = if let Some(page) = provisional {
			(
				Some(page.page.clone()),
				Some(HistoryPageSource::CachedUnverified),
				HistoryCursorObservation::Unknown,
			)
		} else if let Some(page) = fresh_visible {
			(
				Some(page.clone()),
				Some(HistoryPageSource::FreshServer),
				if page.next_cursor.is_some() {
					HistoryCursorObservation::ContinuationAvailable
				} else {
					HistoryCursorObservation::NoContinuationObserved
				},
			)
		} else {
			(None, None, HistoryCursorObservation::Unknown)
		};
		let current_request = active
			.in_flight
			.as_ref()
			.map(|request| request.request.purpose)
			.or_else(|| active.pending.as_ref().map(|request| request.purpose));
		let load = match active.unavailable {
			Some(HistoryAvailability::Retryable(reason)) => {
				HistoryLoadState::RetryableUnavailable(reason)
			},
			Some(HistoryAvailability::Closed(reason)) => {
				HistoryLoadState::ClosedUnavailable(reason)
			},
			None => match (visible.is_some(), current_request) {
				(false, Some(_)) => HistoryLoadState::InitialLoading,
				(true, Some(RequestPurpose::Prefetch)) => HistoryLoadState::PrefetchingAdjacent,
				(true, Some(_)) => HistoryLoadState::RefreshingVisible,
				(true, None) => HistoryLoadState::Visible,
				(false, None) => HistoryLoadState::InitialLoading,
			},
		};
		let retained_pages = active.pages.len() + usize::from(active.provisional.is_some());
		let retained_items = active.pages.iter().map(|page| page.page.items.len()).sum::<usize>()
			+ active.provisional.as_ref().map_or(0, |page| page.page.items.len());
		let retained_bytes = active.pages.iter().map(|page| page.byte_length).sum::<usize>()
			+ active.provisional.as_ref().map_or(0, |page| page.byte_length);
		let can_show_previous = active.visible_index.is_some_and(|index| index > 0);
		let can_show_next = active.visible_index.is_some_and(|index| {
			index.checked_add(1).is_some_and(|next| next < active.pages.len())
				|| active.pages[index].page.next_cursor.is_some()
		});
		let next_cursor = visible.as_ref().and_then(|page| page.next_cursor.clone());

		HistorySnapshot {
			conversation_id: Some(active.conversation_id.clone()),
			view_generation: active.generation,
			load,
			visible,
			visible_source,
			next_cursor,
			cursor,
			cache_diagnostic: active.cache_diagnostic,
			retained_pages,
			retained_items,
			retained_bytes,
			can_show_previous,
			can_show_next,
			can_retry: active.retry_request.is_some(),
			last_stale_cancellation: self.last_stale_cancellation,
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SessionBinding {
	generation: u64,
	server_id: ServerId,
}

struct ActiveView {
	conversation_id: EntityId,
	generation: u64,
	pages: VecDeque<RetainedPage>,
	visible_index: Option<usize>,
	pending: Option<PageRequest>,
	in_flight: Option<InFlightRequest>,
	retry_request: Option<PageRequest>,
	unavailable: Option<HistoryAvailability>,
	cache_lookup_armed: Option<PageRequest>,
	cache_publication_fence: Option<CacheOperationIdentity>,
	provisional: Option<ProvisionalPage>,
	cache_diagnostic: Option<HistoryCacheDiagnostic>,
}

impl ActiveView {
	fn new(conversation_id: EntityId, generation: u64) -> Self {
		let initial = PageRequest::new(
			generation,
			PageKey::initial(conversation_id.clone()),
			RequestPurpose::Initial,
		);

		Self {
			conversation_id,
			generation,
			pages: VecDeque::new(),
			visible_index: None,
			pending: Some(initial.clone()),
			in_flight: None,
			retry_request: None,
			unavailable: None,
			cache_lookup_armed: Some(initial),
			cache_publication_fence: None,
			provisional: None,
			cache_diagnostic: None,
		}
	}

	fn refresh_initial(&mut self, generation: u64) {
		self.generation = generation;
		self.clear_cache_presentation();
		self.pending = Some(PageRequest::new(
			generation,
			PageKey::initial(self.conversation_id.clone()),
			RequestPurpose::Visible,
		));
		self.in_flight = None;
		self.retry_request = None;
		self.unavailable = None;
		self.cache_lookup_armed = None;
		self.cache_publication_fence = None;
	}

	fn invalidate_session_authority(&mut self, unavailable: Option<HistoryAvailability>) {
		self.clear_cache_presentation();
		self.pages.clear();
		self.visible_index = None;
		let request = PageRequest::new(
			self.generation,
			PageKey::initial(self.conversation_id.clone()),
			RequestPurpose::Initial,
		);

		self.pending = Some(request.clone());
		self.in_flight = None;
		self.retry_request = None;
		self.unavailable = unavailable;
		self.cache_lookup_armed = Some(request);
		self.cache_publication_fence = None;
	}

	fn admit_provisional_page(
		&mut self,
		request: PageRequest,
		page: ConversationHistoryPage,
		limits: HistoryPagerLimits,
	) {
		match validated_page_byte_length(&page, limits) {
			Ok(byte_length) => {
				let retained_items =
					self.pages.iter().map(|retained| retained.page.items.len()).sum::<usize>();
				let retained_bytes =
					self.pages.iter().map(|retained| retained.byte_length).sum::<usize>();

				if self.pages.len().saturating_add(1) > limits.max_window_pages
					|| retained_items.saturating_add(page.items.len()) > limits.max_window_items
					|| retained_bytes.saturating_add(byte_length) > limits.max_window_bytes
				{
					self.provisional = None;
					self.cache_diagnostic = Some(HistoryCacheDiagnostic::Unavailable);
				} else {
					self.provisional = Some(ProvisionalPage { request, page, byte_length });
					self.cache_diagnostic = None;
				}
			},
			Err(_) => {
				self.provisional = None;
				self.cache_diagnostic = Some(HistoryCacheDiagnostic::Unavailable);
			},
		}
	}

	fn clear_cache_presentation(&mut self) {
		self.clear_provisional_cache_page();
		self.cache_diagnostic = None;
	}

	fn clear_provisional_cache_page(&mut self) {
		self.provisional = None;
	}

	fn admit_live_page(
		&mut self,
		request: PageRequest,
		mut page: ConversationHistoryPage,
		limits: HistoryPagerLimits,
	) -> Result<(), HistoryClosedReason> {
		if let Some(next_cursor) = page.next_cursor.as_ref()
			&& (request.key.after.as_ref() == Some(next_cursor)
				|| self
					.pages
					.iter()
					.any(|retained| retained.key.after.as_ref() == Some(next_cursor)))
		{
			return Err(HistoryClosedReason::MalformedContinuation);
		}
		self.deduplicate_page(&request.key, &mut page)?;

		let byte_length = validated_page_byte_length(&page, limits)?;

		let existing = self.pages.iter().position(|retained| retained.key == request.key);
		let index = if let Some(index) = existing {
			self.pages[index] = RetainedPage { key: request.key.clone(), page, byte_length };
			index
		} else {
			self.pages.push_back(RetainedPage { key: request.key.clone(), page, byte_length });
			self.pages.len() - 1
		};

		if request.purpose != RequestPurpose::Prefetch {
			self.visible_index = Some(index);
		}
		self.evict_to_limits(limits);

		Ok(())
	}

	fn deduplicate_page(
		&self,
		key: &PageKey,
		page: &mut ConversationHistoryPage,
	) -> Result<(), HistoryClosedReason> {
		let mut accepted = Vec::with_capacity(page.items.len());
		for item in page.items.drain(..) {
			let retained = self
				.pages
				.iter()
				.filter(|retained| &retained.key != key)
				.flat_map(|retained| retained.page.items.iter())
				.find(|retained| retained.history_item_id == item.history_item_id);
			let duplicate = retained.or_else(|| {
				accepted.iter().find(|retained: &&decodex_protocol::HistoryItemDto| {
					retained.history_item_id == item.history_item_id
				})
			});
			if let Some(duplicate) = duplicate {
				if duplicate != &item {
					return Err(HistoryClosedReason::MalformedContinuation);
				}
				continue;
			}
			accepted.push(item);
		}
		page.items = accepted;
		Ok(())
	}

	fn merged_visible_page(&self) -> Option<ConversationHistoryPage> {
		let visible_index = self.visible_index?;
		let visible = self.pages.get(visible_index)?;
		let mut items = Vec::new();
		for retained in self.pages.iter().take(visible_index.saturating_add(1)) {
			for item in &retained.page.items {
				if !items.iter().any(|existing: &decodex_protocol::HistoryItemDto| {
					existing.history_item_id == item.history_item_id
				}) {
					items.push(item.clone());
				}
			}
		}
		Some(ConversationHistoryPage { items, next_cursor: visible.page.next_cursor.clone() })
	}

	fn enqueue_adjacent_prefetch(&mut self) {
		if self.pending.is_some() || self.in_flight.is_some() {
			return;
		}
		let Some(visible) = self.visible_index.and_then(|index| self.pages.get(index)) else {
			return;
		};
		let Some(after) = visible.page.next_cursor.clone() else {
			return;
		};
		let key = PageKey::new(self.conversation_id.clone(), Some(after));

		if self.pages.iter().any(|page| page.key == key) {
			return;
		}

		let request = PageRequest::new(self.generation, key, RequestPurpose::Prefetch);

		self.pending = Some(request.clone());
		self.cache_lookup_armed = Some(request);
	}

	fn evict_to_limits(&mut self, limits: HistoryPagerLimits) {
		// Keep the visible page and evict the farthest end, preferring the older
		// front page when both ends are equally distant.
		while self.pages.len() > limits.max_window_pages
			|| self.pages.iter().map(|page| page.page.items.len()).sum::<usize>()
				> limits.max_window_items
			|| self.pages.iter().map(|page| page.byte_length).sum::<usize>()
				> limits.max_window_bytes
		{
			let Some(visible) = self.visible_index else {
				self.pages.pop_front();

				continue;
			};
			let back_distance = self.pages.len().saturating_sub(1).saturating_sub(visible);

			if visible == 0 {
				self.pages.pop_back();
			} else if visible >= back_distance {
				self.pages.pop_front();
				self.visible_index = visible.checked_sub(1);
			} else {
				self.pages.pop_back();
			}
		}
	}
}

struct RetainedPage {
	key: PageKey,
	page: ConversationHistoryPage,
	byte_length: usize,
}

struct ProvisionalPage {
	request: PageRequest,
	page: ConversationHistoryPage,
	byte_length: usize,
}

#[derive(Clone, Copy)]
enum HistoryAvailability {
	Retryable(HistoryRetryReason),
	Closed(HistoryClosedReason),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PageRequest {
	view_generation: u64,
	key: PageKey,
	purpose: RequestPurpose,
}

impl PageRequest {
	fn new(view_generation: u64, key: PageKey, purpose: RequestPurpose) -> Self {
		Self { view_generation, key, purpose }
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PageKey {
	conversation_id: EntityId,
	after: Option<HistoryCursorToken>,
}

impl PageKey {
	fn initial(conversation_id: EntityId) -> Self {
		Self::new(conversation_id, None)
	}

	fn new(conversation_id: EntityId, after: Option<HistoryCursorToken>) -> Self {
		Self { conversation_id, after }
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RequestPurpose {
	Initial,
	Visible,
	Prefetch,
}

fn validated_page_byte_length(
	page: &ConversationHistoryPage,
	limits: HistoryPagerLimits,
) -> Result<usize, HistoryClosedReason> {
	let bytes = serde_json::to_vec(page).map_err(|_| HistoryClosedReason::LocalBounds)?;

	if page.items.len() > usize::from(MAX_HISTORY_PAGE_SIZE)
		|| bytes.len() > limits.max_page_bytes
		|| bytes.len() > limits.max_window_bytes
		|| page.items.len() > limits.max_window_items
	{
		return Err(HistoryClosedReason::LocalBounds);
	}

	Ok(bytes.len())
}

fn current_unix_seconds() -> Option<i64> {
	let elapsed = SystemTime::now().duration_since(UNIX_EPOCH).ok()?;

	i64::try_from(elapsed.as_secs()).ok()
}

struct InFlightRequest {
	query_id: QueryId,
	session_generation: u64,
	server_id: ServerId,
	request_sequence: u64,
	request: PageRequest,
}

impl InFlightRequest {
	fn from_dispatch(dispatch: &HistoryDispatch) -> Self {
		Self {
			query_id: dispatch.envelope.query_id.clone(),
			session_generation: dispatch.session_generation,
			server_id: dispatch.server_id.clone(),
			request_sequence: dispatch.request_sequence,
			request: dispatch.request.clone(),
		}
	}

	fn matches(&self, dispatch: &HistoryDispatch) -> bool {
		self.query_id == dispatch.envelope.query_id
			&& self.session_generation == dispatch.session_generation
			&& self.server_id == dispatch.server_id
			&& self.request_sequence == dispatch.request_sequence
			&& self.request == dispatch.request
	}

	fn matches_send_token(&self, token: &HistorySendToken) -> bool {
		self.query_id == token.query_id
			&& self.session_generation == token.session_generation
			&& self.server_id == token.server_id
			&& self.request_sequence == token.request_sequence
	}

	fn matches_cache_identity(&self, identity: &CacheOperationIdentity) -> bool {
		self.query_id == identity.query_id
			&& self.session_generation == identity.session_generation
			&& self.server_id == identity.server_id
			&& self.request_sequence == identity.request_sequence
			&& self.request == identity.request
	}
}

struct CancelledRequest {
	query_id: QueryId,
	session_generation: u64,
	server_id: ServerId,
}

fn history_availability(error: HistoryQueryError) -> HistoryAvailability {
	match error {
		HistoryQueryError::InvalidRequest => {
			HistoryAvailability::Closed(HistoryClosedReason::InvalidRequest)
		},
		HistoryQueryError::ResourceExhausted => {
			HistoryAvailability::Retryable(HistoryRetryReason::ResourceExhausted)
		},
		HistoryQueryError::ProductStateUnavailable => {
			HistoryAvailability::Retryable(HistoryRetryReason::ProductStateUnavailable)
		},
		HistoryQueryError::IntegrityUnavailable => {
			HistoryAvailability::Retryable(HistoryRetryReason::IntegrityUnavailable)
		},
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use tempfile::TempDir;

	const SESSION_GENERATION: u64 = 7;
	const TEST_CACHE_SCHEMA_GENERATION: u32 = 1;

	fn server(value: &str) -> ServerId {
		ServerId::new(value).expect("test server identity is bounded")
	}

	fn entity(value: &str) -> EntityId {
		EntityId::new(value).expect("test entity identity is bounded")
	}

	fn cursor(value: &str) -> HistoryCursorToken {
		HistoryCursorToken::new(value).expect("test history cursor is bounded")
	}

	fn page(next_cursor: Option<&str>) -> ConversationHistoryPage {
		ConversationHistoryPage { items: Vec::new(), next_cursor: next_cursor.map(cursor) }
	}

	fn result(
		dispatch: &HistoryDispatch,
		server_id: &ServerId,
		history: ConversationHistoryResult,
	) -> QueryResultEnvelope {
		QueryResultEnvelope {
			version: CURRENT_VERSION,
			server_id: server_id.clone(),
			query_id: dispatch.envelope.query_id.clone(),
			payload: QueryResultPayload::ConversationHistory(history),
		}
	}

	fn open_pager() -> (TempDir, HistoryPager, ServerId) {
		let temporary = TempDir::new().expect("temporary cache parent is available");
		let pager = HistoryPager::production(temporary.path(), 1);
		let server_id = server("server-a");

		pager.bind_session(SESSION_GENERATION, server_id.clone());
		pager.open(entity("conversation-a")).expect("view identity is available");

		(temporary, pager, server_id)
	}

	fn seed_cached_head(
		parent: &Path,
		server_id: &ServerId,
		conversation_id: &EntityId,
		page: &ConversationHistoryPage,
	) {
		let authority = CacheAuthority::new(
			server_id.clone(),
			CURRENT_VERSION.major,
			CURRENT_VERSION.minor,
			TEST_CACHE_SCHEMA_GENERATION,
		)
		.expect("cache authority is valid");
		let request =
			CacheRequest::head(&authority, conversation_id.clone()).expect("head request is valid");
		let mut cache = HistoryPageCache::open(parent, TEST_CACHE_SCHEMA_GENERATION)
			.expect("history page cache opens");
		let prepared = cache
			.prepare_publication(
				&request,
				page,
				current_unix_seconds().expect("current wall time is representable"),
			)
			.expect("cached head prepares");
		let committed = match cache.commit_publication(prepared) {
			Ok(committed) => committed,
			Err(_) => panic!("cached head commits"),
		};

		cache.finish_publication(committed).expect("cached head publication finishes");
	}

	#[test]
	fn one_view_reserves_at_most_one_current_request() {
		let (_temporary, pager, server_id) = open_pager();
		let dispatch = pager
			.try_take_dispatch(SESSION_GENERATION, &server_id)
			.expect("initial request is ready");

		assert!(pager.dispatch_is_current(&dispatch));
		assert!(pager.try_take_dispatch(SESSION_GENERATION, &server_id).is_none());
		assert!(matches!(
			&dispatch.envelope.payload,
			QueryPayload::GetConversationHistory {
				conversation_id,
				after: None,
				page_size: MAX_HISTORY_PAGE_SIZE,
			} if conversation_id == &entity("conversation-a")
		));
	}

	#[test]
	fn cached_head_requires_sent_lookup_and_fresh_response_for_topology() {
		let temporary = TempDir::new_in(std::env::temp_dir())
			.expect("host temporary directory accepts an isolated fixture");
		let cache_parent = temporary.path().join("cache-parent");
		let server_id = server("server-cache-flow");
		let conversation_a = entity("conversation-cache-a");
		let conversation_b = entity("conversation-cache-b");
		let cached_page = page(Some("cached-next"));
		let fresh_page = page(Some("fresh-next"));

		seed_cached_head(&cache_parent, &server_id, &conversation_a, &cached_page);
		seed_cached_head(&cache_parent, &server_id, &conversation_b, &cached_page);

		let pager = HistoryPager::production(&cache_parent, TEST_CACHE_SCHEMA_GENERATION);

		assert!(matches!(
			&*pager
				.inner
				.page_cache
				.lock()
				.unwrap_or_else(std::sync::PoisonError::into_inner),
			PageCacheOwner::Dormant { parent, cache_schema_generation }
				if parent == &cache_parent
					&& *cache_schema_generation == TEST_CACHE_SCHEMA_GENERATION
		));

		pager.bind_session(SESSION_GENERATION, server_id.clone());
		pager.open(conversation_a.clone()).expect("cached conversation view opens");
		let head = pager
			.try_take_dispatch(SESSION_GENERATION, &server_id)
			.expect("fresh head request is ready");

		assert!(pager.snapshot().visible.is_none());
		let send = pager.begin_send(&head).expect("current head enters the send phase");
		assert!(pager.snapshot().visible.is_none());
		assert!(pager.finish_send(&send));
		assert!(pager.snapshot().visible.is_none());
		assert!(matches!(
			&*pager.inner.page_cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner),
			PageCacheOwner::Dormant { .. }
		));

		pager.lookup_sent_request(&send);

		let provisional = pager.snapshot();

		assert_eq!(provisional.visible, Some(cached_page.clone()));
		assert_eq!(provisional.visible_source, Some(HistoryPageSource::CachedUnverified));
		assert_eq!(provisional.cursor, HistoryCursorObservation::Unknown);
		assert_eq!(provisional.retained_pages, 1);
		assert!(matches!(
			&*pager.inner.page_cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner),
			PageCacheOwner::Enabled(_)
		));
		{
			let state = pager.lock();
			let active = state.active.as_ref().expect("cached view remains active");

			assert!(active.pages.is_empty());
			assert!(active.pending.is_none());
		}
		assert_eq!(pager.show_next(), HistoryNavigationResult::BoundaryUnknown);
		assert!(pager.try_take_dispatch(SESSION_GENERATION, &server_id).is_none());

		assert!(matches!(
			pager.route_result(
				SESSION_GENERATION,
				&server_id,
				result(&head, &server_id, ConversationHistoryResult::Page(fresh_page.clone()),),
			),
			HistoryRouteOutcome::Fresh
		));

		let fresh = pager.snapshot();

		assert_eq!(fresh.visible, Some(fresh_page));
		assert_eq!(fresh.visible_source, Some(HistoryPageSource::FreshServer));
		assert_eq!(fresh.cursor, HistoryCursorObservation::ContinuationAvailable);
		assert_eq!(fresh.retained_pages, 1);

		let prefetch = pager
			.try_take_dispatch(SESSION_GENERATION, &server_id)
			.expect("only the matching fresh cursor enables prefetch");

		assert!(matches!(
			&prefetch.envelope.payload,
			QueryPayload::GetConversationHistory { after: Some(after), .. }
				if after == &cursor("fresh-next")
		));

		pager.open(conversation_b).expect("second cached conversation opens");
		let stale_dispatch = pager
			.try_take_dispatch(SESSION_GENERATION, &server_id)
			.expect("second cached head request is ready");
		let stale_send =
			pager.begin_send(&stale_dispatch).expect("second cached head enters the send phase");

		assert!(pager.finish_send(&stale_send));
		pager.open(entity("conversation-cache-c")).expect("replacement view identity is available");
		pager.lookup_sent_request(&stale_send);

		let replaced = pager.snapshot();

		assert_eq!(replaced.conversation_id, Some(entity("conversation-cache-c")));
		assert!(replaced.visible.is_none());
		assert_eq!(replaced.visible_source, None);
		assert_eq!(replaced.cursor, HistoryCursorObservation::Unknown);
		assert_eq!(
			replaced.last_stale_cancellation,
			Some(HistoryStaleCancellation {
				request_sequence: stale_dispatch.request_sequence,
				reason: HistoryStaleReason::ConversationChanged,
			})
		);
	}

	#[test]
	fn retry_reissues_only_the_exact_retryable_request() {
		let (_temporary, pager, server_id) = open_pager();
		let first = pager
			.try_take_dispatch(SESSION_GENERATION, &server_id)
			.expect("initial request is ready");

		assert!(matches!(
			pager.route_result(
				SESSION_GENERATION,
				&server_id,
				result(
					&first,
					&server_id,
					ConversationHistoryResult::Unavailable {
						error: HistoryQueryError::ResourceExhausted,
					},
				),
			),
			HistoryRouteOutcome::Unavailable
		));
		assert_eq!(
			pager.snapshot().load,
			HistoryLoadState::RetryableUnavailable(HistoryRetryReason::ResourceExhausted)
		);
		assert!(pager.retry());
		assert!(!pager.retry());

		let retried = pager
			.try_take_dispatch(SESSION_GENERATION, &server_id)
			.expect("the exact retryable request is ready");

		assert_eq!(retried.request, first.request);
		assert_ne!(retried.envelope.query_id, first.envelope.query_id);
	}

	#[test]
	fn cancel_makes_the_in_flight_request_stale_and_closes_the_view() {
		let (_temporary, pager, server_id) = open_pager();
		let dispatch = pager
			.try_take_dispatch(SESSION_GENERATION, &server_id)
			.expect("initial request is ready");

		pager.cancel();

		let snapshot = pager.snapshot();

		assert_eq!(snapshot.load, HistoryLoadState::Inactive);
		assert_eq!(
			snapshot.last_stale_cancellation,
			Some(HistoryStaleCancellation {
				request_sequence: dispatch.request_sequence,
				reason: HistoryStaleReason::ViewCancelled,
			})
		);
		assert!(matches!(
			pager.route_result(
				SESSION_GENERATION,
				&server_id,
				result(&dispatch, &server_id, ConversationHistoryResult::Page(page(None))),
			),
			HistoryRouteOutcome::Stale
		));
	}

	#[test]
	fn empty_page_is_visible_without_proving_history_completion() {
		let (_temporary, pager, server_id) = open_pager();
		let dispatch = pager
			.try_take_dispatch(SESSION_GENERATION, &server_id)
			.expect("initial request is ready");

		assert!(matches!(
			pager.route_result(
				SESSION_GENERATION,
				&server_id,
				result(&dispatch, &server_id, ConversationHistoryResult::Page(page(None))),
			),
			HistoryRouteOutcome::Fresh
		));

		let snapshot = pager.snapshot();

		assert_eq!(snapshot.load, HistoryLoadState::Visible);
		assert_eq!(snapshot.cursor, HistoryCursorObservation::NoContinuationObserved);
		assert_eq!(snapshot.visible.expect("empty page remains visible").items.len(), 0);
		assert_eq!(pager.show_next(), HistoryNavigationResult::BoundaryUnknown);
	}

	#[test]
	fn navigation_cancels_the_exact_prefetch_and_ignores_its_late_result() {
		let (_temporary, pager, server_id) = open_pager();
		let initial = pager
			.try_take_dispatch(SESSION_GENERATION, &server_id)
			.expect("initial request is ready");

		assert!(matches!(
			pager.route_result(
				SESSION_GENERATION,
				&server_id,
				result(
					&initial,
					&server_id,
					ConversationHistoryResult::Page(page(Some("cursor-1"))),
				),
			),
			HistoryRouteOutcome::Fresh
		));

		let stale_prefetch = pager
			.try_take_dispatch(SESSION_GENERATION, &server_id)
			.expect("adjacent prefetch is ready");

		assert_eq!(pager.show_next(), HistoryNavigationResult::Moved);
		assert!(matches!(
			pager.route_result(
				SESSION_GENERATION,
				&server_id,
				result(&stale_prefetch, &server_id, ConversationHistoryResult::Page(page(None)),),
			),
			HistoryRouteOutcome::Stale
		));

		let visible = pager
			.try_take_dispatch(SESSION_GENERATION, &server_id)
			.expect("visible continuation replaces the stale prefetch");

		assert_ne!(visible.envelope.query_id, stale_prefetch.envelope.query_id);
		assert!(matches!(
			&visible.envelope.payload,
			QueryPayload::GetConversationHistory {
				after: Some(after),
				..
			} if after == &cursor("cursor-1")
		));
		assert_eq!(
			pager.snapshot().last_stale_cancellation,
			Some(HistoryStaleCancellation {
				request_sequence: stale_prefetch.request_sequence,
				reason: HistoryStaleReason::NavigationChanged,
			})
		);
	}

	#[test]
	fn self_referential_continuation_closes_without_reissuing_the_cursor() {
		let (_temporary, pager, server_id) = open_pager();
		let head = pager
			.try_take_dispatch(SESSION_GENERATION, &server_id)
			.expect("initial request is ready");

		assert!(matches!(
			pager.route_result(
				SESSION_GENERATION,
				&server_id,
				result(&head, &server_id, ConversationHistoryResult::Page(page(Some("cursor-1"))),),
			),
			HistoryRouteOutcome::Fresh
		));

		let continuation = pager
			.try_take_dispatch(SESSION_GENERATION, &server_id)
			.expect("continuation prefetch is ready");

		assert!(matches!(
			pager.route_result(
				SESSION_GENERATION,
				&server_id,
				result(
					&continuation,
					&server_id,
					ConversationHistoryResult::Page(page(Some("cursor-1"))),
				),
			),
			HistoryRouteOutcome::Closed
		));
		assert_eq!(
			pager.snapshot().load,
			HistoryLoadState::ClosedUnavailable(HistoryClosedReason::MalformedContinuation)
		);
		assert_eq!(pager.show_next(), HistoryNavigationResult::BoundaryUnknown);
		assert!(pager.try_take_dispatch(SESSION_GENERATION, &server_id).is_none());
	}

	#[test]
	fn continuation_to_a_retained_page_closes_without_creating_a_cycle() {
		let (_temporary, pager, server_id) = open_pager();
		let head = pager
			.try_take_dispatch(SESSION_GENERATION, &server_id)
			.expect("initial request is ready");
		let _ = pager.route_result(
			SESSION_GENERATION,
			&server_id,
			result(&head, &server_id, ConversationHistoryResult::Page(page(Some("cursor-1")))),
		);
		let first_continuation = pager
			.try_take_dispatch(SESSION_GENERATION, &server_id)
			.expect("first continuation prefetch is ready");
		let _ = pager.route_result(
			SESSION_GENERATION,
			&server_id,
			result(
				&first_continuation,
				&server_id,
				ConversationHistoryResult::Page(page(Some("cursor-2"))),
			),
		);

		assert_eq!(pager.show_next(), HistoryNavigationResult::Moved);

		let second_continuation = pager
			.try_take_dispatch(SESSION_GENERATION, &server_id)
			.expect("second continuation prefetch is ready");

		assert!(matches!(
			pager.route_result(
				SESSION_GENERATION,
				&server_id,
				result(
					&second_continuation,
					&server_id,
					ConversationHistoryResult::Page(page(Some("cursor-1"))),
				),
			),
			HistoryRouteOutcome::Closed
		));

		let snapshot = pager.snapshot();

		assert_eq!(
			snapshot.load,
			HistoryLoadState::ClosedUnavailable(HistoryClosedReason::MalformedContinuation)
		);
		assert_eq!(snapshot.retained_pages, 2);
		assert_eq!(pager.show_next(), HistoryNavigationResult::BoundaryUnknown);
		assert!(pager.try_take_dispatch(SESSION_GENERATION, &server_id).is_none());
	}

	#[test]
	fn session_replacement_discards_visible_and_retained_fresh_topology() {
		let (_temporary, pager, server_id) = open_pager();
		let initial = pager
			.try_take_dispatch(SESSION_GENERATION, &server_id)
			.expect("initial request is ready");

		assert!(matches!(
			pager.route_result(
				SESSION_GENERATION,
				&server_id,
				result(
					&initial,
					&server_id,
					ConversationHistoryResult::Page(page(Some("cursor-1"))),
				),
			),
			HistoryRouteOutcome::Fresh
		));
		let retained_prefetch = pager
			.try_take_dispatch(SESSION_GENERATION, &server_id)
			.expect("adjacent prefetch is ready");

		assert!(matches!(
			pager.route_result(
				SESSION_GENERATION,
				&server_id,
				result(
					&retained_prefetch,
					&server_id,
					ConversationHistoryResult::Page(page(Some("old-session-cursor"))),
				),
			),
			HistoryRouteOutcome::Fresh
		));

		let old_session = pager.snapshot();

		assert_eq!(old_session.retained_pages, 2);
		assert_eq!(old_session.visible_source, Some(HistoryPageSource::FreshServer));
		assert_eq!(old_session.cursor, HistoryCursorObservation::ContinuationAvailable);

		pager.bind_session(SESSION_GENERATION + 1, server_id.clone());

		let invalidated = pager.snapshot();

		assert!(invalidated.visible.is_none());
		assert_eq!(invalidated.visible_source, None);
		assert_eq!(invalidated.cursor, HistoryCursorObservation::Unknown);
		assert_eq!(invalidated.retained_pages, 0);
		assert_eq!(pager.show_next(), HistoryNavigationResult::BoundaryUnknown);

		let replacement = pager
			.try_take_dispatch(SESSION_GENERATION + 1, &server_id)
			.expect("replacement session refreshes the conversation head");

		assert!(matches!(
			&replacement.envelope.payload,
			QueryPayload::GetConversationHistory {
				conversation_id,
				after: None,
				..
			} if conversation_id == &entity("conversation-a")
		));
	}

	#[test]
	fn server_replacement_rejects_late_prefetch_until_matching_head_upgrade() {
		let (_temporary, pager, old_server) = open_pager();
		let initial = pager
			.try_take_dispatch(SESSION_GENERATION, &old_server)
			.expect("initial request is ready");

		assert!(matches!(
			pager.route_result(
				SESSION_GENERATION,
				&old_server,
				result(
					&initial,
					&old_server,
					ConversationHistoryResult::Page(page(Some("old-cursor"))),
				),
			),
			HistoryRouteOutcome::Fresh
		));
		let late_prefetch = pager
			.try_take_dispatch(SESSION_GENERATION, &old_server)
			.expect("old session prefetch is in flight");
		let new_server = server("server-b");

		pager.bind_session(SESSION_GENERATION + 1, new_server.clone());

		assert_eq!(pager.show_next(), HistoryNavigationResult::BoundaryUnknown);
		let replacement = pager
			.try_take_dispatch(SESSION_GENERATION + 1, &new_server)
			.expect("new server refreshes the conversation head");

		assert!(matches!(
			pager.route_result(
				SESSION_GENERATION,
				&old_server,
				result(
					&late_prefetch,
					&old_server,
					ConversationHistoryResult::Page(page(Some("late-cursor"))),
				),
			),
			HistoryRouteOutcome::Stale
		));
		assert!(pager.dispatch_is_current(&replacement));
		let stale_ignored = pager.snapshot();

		assert!(stale_ignored.visible.is_none());
		assert_eq!(
			stale_ignored.last_stale_cancellation,
			Some(HistoryStaleCancellation {
				request_sequence: late_prefetch.request_sequence,
				reason: HistoryStaleReason::SessionReplaced,
			})
		);

		assert!(matches!(
			pager.route_result(
				SESSION_GENERATION + 1,
				&new_server,
				result(
					&replacement,
					&new_server,
					ConversationHistoryResult::Page(page(Some("new-cursor"))),
				),
			),
			HistoryRouteOutcome::Fresh
		));

		let upgraded = pager.snapshot();

		assert_eq!(upgraded.visible_source, Some(HistoryPageSource::FreshServer));
		assert_eq!(upgraded.cursor, HistoryCursorObservation::ContinuationAvailable);

		let new_prefetch = pager
			.try_take_dispatch(SESSION_GENERATION + 1, &new_server)
			.expect("matching fresh topology enables prefetch");

		assert!(matches!(
			&new_prefetch.envelope.payload,
			QueryPayload::GetConversationHistory {
				after: Some(after),
				..
			} if after == &cursor("new-cursor")
		));
	}

	#[test]
	fn bounded_window_evicts_the_older_page_on_an_equal_distance_tie() {
		let server_id = server("server-a");
		let pager = HistoryPager::new(HistoryPagerLimits {
			max_page_bytes: PRODUCTION_MAX_PAGE_BYTES,
			max_window_bytes: PRODUCTION_MAX_WINDOW_BYTES,
			max_window_items: PRODUCTION_MAX_WINDOW_ITEMS,
			max_window_pages: 2,
		});

		pager.bind_session(SESSION_GENERATION, server_id.clone());
		pager.open(entity("conversation-a")).expect("view identity is available");

		let first = pager
			.try_take_dispatch(SESSION_GENERATION, &server_id)
			.expect("initial request is ready");
		let _ = pager.route_result(
			SESSION_GENERATION,
			&server_id,
			result(&first, &server_id, ConversationHistoryResult::Page(page(Some("cursor-1")))),
		);
		let second = pager
			.try_take_dispatch(SESSION_GENERATION, &server_id)
			.expect("second page prefetch is ready");
		let _ = pager.route_result(
			SESSION_GENERATION,
			&server_id,
			result(&second, &server_id, ConversationHistoryResult::Page(page(Some("cursor-2")))),
		);

		assert_eq!(pager.show_next(), HistoryNavigationResult::Moved);

		let third = pager
			.try_take_dispatch(SESSION_GENERATION, &server_id)
			.expect("third page prefetch is ready");
		let _ = pager.route_result(
			SESSION_GENERATION,
			&server_id,
			result(&third, &server_id, ConversationHistoryResult::Page(page(None))),
		);

		let snapshot = pager.snapshot();

		assert_eq!(snapshot.retained_pages, 2);
		assert_eq!(snapshot.cursor, HistoryCursorObservation::ContinuationAvailable);
		assert_eq!(pager.show_previous(), HistoryNavigationResult::BoundaryUnknown);
	}

	#[test]
	fn page_bytes_over_the_local_bound_close_only_the_request_view() {
		let server_id = server("server-a");
		let pager = HistoryPager::new(HistoryPagerLimits {
			max_page_bytes: 1,
			max_window_bytes: PRODUCTION_MAX_WINDOW_BYTES,
			max_window_items: PRODUCTION_MAX_WINDOW_ITEMS,
			max_window_pages: PRODUCTION_MAX_WINDOW_PAGES,
		});

		pager.bind_session(SESSION_GENERATION, server_id.clone());
		pager.open(entity("conversation-a")).expect("view identity is available");
		let dispatch = pager
			.try_take_dispatch(SESSION_GENERATION, &server_id)
			.expect("initial request is ready");

		assert!(matches!(
			pager.route_result(
				SESSION_GENERATION,
				&server_id,
				result(&dispatch, &server_id, ConversationHistoryResult::Page(page(None))),
			),
			HistoryRouteOutcome::Closed
		));
		assert_eq!(
			pager.snapshot().load,
			HistoryLoadState::ClosedUnavailable(HistoryClosedReason::LocalBounds)
		);
	}
}
