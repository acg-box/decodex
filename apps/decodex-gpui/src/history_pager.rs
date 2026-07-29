//! Presentation-neutral, bounded ConversationHistory paging for one GPUI view.

use std::{
	collections::VecDeque,
	sync::{Arc, Mutex, MutexGuard},
};

use tokio::sync::Notify;

use decodex_protocol::{
	CURRENT_VERSION, ConversationHistoryPage, ConversationHistoryResult, EntityId,
	HistoryCursorToken, HistoryQueryError, MAX_HISTORY_PAGE_SIZE, QueryEnvelope, QueryId,
	QueryPayload, QueryResultEnvelope, QueryResultPayload, ServerId,
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
	pub(crate) cursor: HistoryCursorObservation,
	pub(crate) retained_pages: usize,
	pub(crate) retained_items: usize,
	pub(crate) retained_bytes: usize,
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
	notify: Notify,
}

impl HistoryPager {
	pub(crate) fn production() -> Self {
		Self::new(HistoryPagerLimits::production())
	}

	fn new(limits: HistoryPagerLimits) -> Self {
		Self {
			inner: Arc::new(HistoryPagerInner {
				state: Mutex::new(PagerState::new(limits)),
				notify: Notify::new(),
			}),
		}
	}

	/// Start a fresh view and cancel every result bound to the previous view.
	pub(crate) fn open(&self, conversation_id: EntityId) -> Result<(), HistoryClosedReason> {
		let mut state = self.lock();
		let generation =
			state.next_view_generation().ok_or(HistoryClosedReason::RequestIdentityExhausted)?;

		state.cancel_in_flight(HistoryStaleReason::ConversationChanged);
		state.active = Some(ActiveView::new(conversation_id, generation));
		drop(state);
		self.inner.notify.notify_one();

		Ok(())
	}

	/// Move to the next retained page or request the exact observed continuation.
	pub(crate) fn show_next(&self) -> HistoryNavigationResult {
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
		if let Some(next_index) = retained_next {
			active.visible_index = Some(next_index);
			active.pending = None;
			active.enqueue_adjacent_prefetch();
		} else if let Some(after) = continuation {
			active.pending = Some(PageRequest::new(
				generation,
				PageKey::new(conversation_id, Some(after)),
				RequestPurpose::Visible,
			));
		}
		drop(state);
		self.inner.notify.notify_one();

		HistoryNavigationResult::Moved
	}

	/// Move to the previous retained page. Evicted history remains unknown.
	pub(crate) fn show_previous(&self) -> HistoryNavigationResult {
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
		active.unavailable = None;
		drop(state);
		self.inner.notify.notify_one();

		true
	}

	/// Cancel the current view without claiming that its Conversation is absent.
	pub(crate) fn cancel(&self) {
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

	/// Bind future dispatch to one retained-session generation and stable server.
	pub(crate) fn bind_session(&self, generation: u64, server_id: ServerId) {
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

				if let Err(reason) = active.admit_live_page(request.clone(), page, limits) {
					active.unavailable = Some(HistoryAvailability::Closed(reason));
					active.retry_request = None;

					return HistoryRouteOutcome::Closed;
				}

				active.unavailable = None;
				active.retry_request = None;
				if request.purpose != RequestPurpose::Prefetch {
					active.enqueue_adjacent_prefetch();
				}

				if active.pending.is_some() {
					self.inner.notify.notify_one();
				}

				HistoryRouteOutcome::Fresh
			},
		}
	}

	fn lock(&self) -> MutexGuard<'_, PagerState> {
		self.inner.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
	}
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
			active.unavailable = Some(HistoryAvailability::Closed(reason));
			active.pending = None;
			active.in_flight = None;
			active.retry_request = None;
		}
	}

	fn snapshot(&self) -> HistorySnapshot {
		let Some(active) = self.active.as_ref() else {
			return HistorySnapshot {
				conversation_id: None,
				view_generation: self.next_view_generation,
				load: HistoryLoadState::Inactive,
				visible: None,
				visible_source: None,
				cursor: HistoryCursorObservation::Unknown,
				retained_pages: 0,
				retained_items: 0,
				retained_bytes: 0,
				last_stale_cancellation: self.last_stale_cancellation,
			};
		};
		let visible = active.visible_index.and_then(|index| active.pages.get(index));
		let current_request = active
			.in_flight
			.as_ref()
			.map(|request| request.request.purpose)
			.or_else(|| active.pending.as_ref().map(|request| request.purpose));
		let load = match active.unavailable {
			Some(HistoryAvailability::Retryable(reason)) =>
				HistoryLoadState::RetryableUnavailable(reason),
			Some(HistoryAvailability::Closed(reason)) =>
				HistoryLoadState::ClosedUnavailable(reason),
			None => match (visible.is_some(), current_request) {
				(false, Some(_)) => HistoryLoadState::InitialLoading,
				(true, Some(RequestPurpose::Prefetch)) => HistoryLoadState::PrefetchingAdjacent,
				(true, Some(_)) => HistoryLoadState::RefreshingVisible,
				(true, None) => HistoryLoadState::Visible,
				(false, None) => HistoryLoadState::InitialLoading,
			},
		};
		let cursor = visible.map_or(HistoryCursorObservation::Unknown, |page| {
			if page.page.next_cursor.is_some() {
				HistoryCursorObservation::ContinuationAvailable
			} else {
				HistoryCursorObservation::NoContinuationObserved
			}
		});
		let retained_pages = active.pages.len();
		let retained_items = active.pages.iter().map(|page| page.page.items.len()).sum::<usize>();
		let retained_bytes = active.pages.iter().map(|page| page.byte_length).sum::<usize>();

		HistorySnapshot {
			conversation_id: Some(active.conversation_id.clone()),
			view_generation: active.generation,
			load,
			visible: visible.map(|page| page.page.clone()),
			visible_source: if visible.is_some() {
				Some(HistoryPageSource::FreshServer)
			} else {
				None
			},
			cursor,
			retained_pages,
			retained_items,
			retained_bytes,
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
			pending: Some(initial),
			in_flight: None,
			retry_request: None,
			unavailable: None,
		}
	}

	fn invalidate_session_authority(&mut self, unavailable: Option<HistoryAvailability>) {
		self.pages.clear();
		self.visible_index = None;
		self.pending = Some(PageRequest::new(
			self.generation,
			PageKey::initial(self.conversation_id.clone()),
			RequestPurpose::Initial,
		));
		self.in_flight = None;
		self.retry_request = None;
		self.unavailable = unavailable;
	}

	fn admit_live_page(
		&mut self,
		request: PageRequest,
		page: ConversationHistoryPage,
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

		let bytes = serde_json::to_vec(&page).map_err(|_| HistoryClosedReason::LocalBounds)?;

		if page.items.len() > usize::from(MAX_HISTORY_PAGE_SIZE)
			|| bytes.len() > limits.max_page_bytes
			|| bytes.len() > limits.max_window_bytes
			|| page.items.len() > limits.max_window_items
		{
			return Err(HistoryClosedReason::LocalBounds);
		}

		let existing = self.pages.iter().position(|retained| retained.key == request.key);
		let index = if let Some(index) = existing {
			self.pages[index] =
				RetainedPage { key: request.key.clone(), page, byte_length: bytes.len() };
			index
		} else {
			self.pages.push_back(RetainedPage {
				key: request.key.clone(),
				page,
				byte_length: bytes.len(),
			});
			self.pages.len() - 1
		};

		if request.purpose != RequestPurpose::Prefetch {
			self.visible_index = Some(index);
		}
		self.evict_to_limits(limits);

		Ok(())
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

		self.pending = Some(PageRequest::new(self.generation, key, RequestPurpose::Prefetch));
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
}

struct CancelledRequest {
	query_id: QueryId,
	session_generation: u64,
	server_id: ServerId,
}

fn history_availability(error: HistoryQueryError) -> HistoryAvailability {
	match error {
		HistoryQueryError::InvalidRequest =>
			HistoryAvailability::Closed(HistoryClosedReason::InvalidRequest),
		HistoryQueryError::ResourceExhausted =>
			HistoryAvailability::Retryable(HistoryRetryReason::ResourceExhausted),
		HistoryQueryError::ProductStateUnavailable =>
			HistoryAvailability::Retryable(HistoryRetryReason::ProductStateUnavailable),
		HistoryQueryError::IntegrityUnavailable =>
			HistoryAvailability::Retryable(HistoryRetryReason::IntegrityUnavailable),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	const SESSION_GENERATION: u64 = 7;

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

	fn open_pager() -> (HistoryPager, ServerId) {
		let pager = HistoryPager::production();
		let server_id = server("server-a");

		pager.bind_session(SESSION_GENERATION, server_id.clone());
		pager.open(entity("conversation-a")).expect("view identity is available");

		(pager, server_id)
	}

	#[test]
	fn one_view_reserves_at_most_one_current_request() {
		let (pager, server_id) = open_pager();
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
	fn empty_page_is_visible_without_proving_history_completion() {
		let (pager, server_id) = open_pager();
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
		let (pager, server_id) = open_pager();
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
		let (pager, server_id) = open_pager();
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
		let (pager, server_id) = open_pager();
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
		let (pager, server_id) = open_pager();
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
		let (pager, old_server) = open_pager();
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
