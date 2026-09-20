//! Bounded native history for positive submitted-message reconciliation.

use super::{
	ExactReconciliationError, ExactThreadId, ExactThreadReadParams, Instant, LossyThreadHistory,
	MAX_APP_SERVER_FRAME_BYTES, MAX_EXACT_THREAD_READ_ITEMS, MAX_EXACT_THREAD_READ_TURNS,
	SupervisedProcess, ThreadReadResponse,
};
use crate::account_launch::protocol::{
	ProtocolThread, ProtocolThreadItem, ProtocolTurn, SensitiveString,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{collections::HashSet, time::Duration};

const PAGE_SIZE: usize = 100;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Params<'a> {
	thread_id: &'a ExactThreadId,
	cursor: Option<&'a str>,
	limit: usize,
	sort_direction: &'static str,
	#[serde(skip_serializing_if = "Option::is_none")]
	items_view: Option<&'static str>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Page<T> {
	data: Vec<T>,
	#[serde(deserialize_with = "Option::<SensitiveString>::deserialize")]
	next_cursor: Option<SensitiveString>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Item {
	turn_id: SensitiveString,
	item: ProtocolThreadItem,
}

impl SupervisedProcess {
	pub(super) fn read_submission_history(
		&mut self,
		thread: &mut ProtocolThread,
		id: &ExactThreadId,
		started: Instant,
		timeout: Duration,
	) -> Result<LossyThreadHistory, ExactReconciliationError> {
		match thread.history_mode.as_deref() {
			None | Some("legacy") => {
				let result = self
					.request_rpc::<_, ThreadReadResponse>(
						"thread/read",
						&ExactThreadReadParams { thread_id: id, include_turns: true },
						remaining(started, timeout)?,
					)
					.map_err(ExactReconciliationError::from_rpc)?;
				if result.thread.id.as_str() != id.as_str() {
					return Err(ExactReconciliationError::InvalidResult);
				}
				thread.turns = result.thread.turns;
				Ok(LossyThreadHistory::IncludeTurnsReadback)
			},
			Some("paginated") => {
				let mut budget = MAX_APP_SERVER_FRAME_BYTES;
				thread.turns = self.read_history_pages::<ProtocolTurn>(
					id,
					true,
					started,
					timeout,
					&mut budget,
				)?;
				let mut ids = HashSet::new();
				for turn in &thread.turns {
					if turn.id.is_empty()
						|| !ids.insert(turn.id.as_str())
						|| !turn.items.is_empty()
						|| turn.items_view.as_deref() != Some("notLoaded")
					{
						return Err(ExactReconciliationError::InvalidResult);
					}
				}
				let items =
					self.read_history_pages::<Item>(id, false, started, timeout, &mut budget)?;
				attach_items(thread, items)?;
				Ok(LossyThreadHistory::PaginatedReadback)
			},
			_ => Err(ExactReconciliationError::InvalidResult),
		}
	}

	fn read_history_pages<T: DeserializeOwned + Record>(
		&mut self,
		id: &ExactThreadId,
		turns: bool,
		started: Instant,
		timeout: Duration,
		budget: &mut usize,
	) -> Result<Vec<T>, ExactReconciliationError> {
		let mut cursors: Vec<SensitiveString> = Vec::new();
		let mut data = Vec::new();
		for _ in 0..128 {
			let page = self
				.request_rpc::<_, Page<T>>(
					if turns { "thread/turns/list" } else { "thread/items/list" },
					&Params {
						thread_id: id,
						cursor: cursors.last().map(SensitiveString::as_str),
						limit: PAGE_SIZE,
						sort_direction: "asc",
						items_view: turns.then_some("notLoaded"),
					},
					remaining(started, timeout)?,
				)
				.map_err(ExactReconciliationError::from_rpc)?;
			if page.data.len() > PAGE_SIZE
				|| data.len().saturating_add(page.data.len()) > T::MAXIMUM
			{
				return Err(ExactReconciliationError::InvalidResult);
			}
			for record in &page.data {
				*budget = budget
					.checked_sub(record.size())
					.ok_or(ExactReconciliationError::InvalidResult)?;
			}
			data.extend(page.data);
			let Some(next) = page.next_cursor else { return Ok(data) };
			if next.is_empty()
				|| next.len() > 4096
				|| cursors.iter().any(|old| old.as_str() == next.as_str())
			{
				return Err(ExactReconciliationError::InvalidResult);
			}
			cursors.push(next);
		}
		Err(ExactReconciliationError::InvalidResult)
	}
}

fn remaining(started: Instant, timeout: Duration) -> Result<Duration, ExactReconciliationError> {
	timeout
		.checked_sub(started.elapsed())
		.filter(|left| !left.is_zero())
		.ok_or(ExactReconciliationError::Transport)
}

fn attach_items(
	thread: &mut ProtocolThread,
	items: Vec<Item>,
) -> Result<(), ExactReconciliationError> {
	let mut seen = HashSet::new();
	for entry in items {
		let id = entry
			.item
			.id
			.as_deref()
			.filter(|id| !id.is_empty())
			.ok_or(ExactReconciliationError::InvalidResult)?;
		if !seen.insert((entry.turn_id.as_str().to_owned(), id.to_owned())) {
			return Err(ExactReconciliationError::InvalidResult);
		}
		let turn = thread
			.turns
			.iter_mut()
			.find(|turn| turn.id.as_str() == entry.turn_id.as_str())
			.ok_or(ExactReconciliationError::InvalidResult)?;
		turn.items.push(entry.item);
	}
	Ok(())
}

trait Record {
	const MAXIMUM: usize;
	fn size(&self) -> usize;
}
impl Record for ProtocolTurn {
	const MAXIMUM: usize = MAX_EXACT_THREAD_READ_TURNS;

	fn size(&self) -> usize {
		self.id
			.len()
			.saturating_add(self.items_view.as_deref().map_or(0, str::len))
			.saturating_add(self.items.iter().map(item_size).sum::<usize>())
	}
}
impl Record for Item {
	const MAXIMUM: usize = MAX_EXACT_THREAD_READ_ITEMS;

	fn size(&self) -> usize {
		self.turn_id.len().saturating_add(item_size(&self.item))
	}
}
fn item_size(item: &ProtocolThreadItem) -> usize {
	item.kind
		.len()
		.saturating_add(item.id.as_deref().map_or(0, str::len))
		.saturating_add(item.text.as_deref().map_or(0, str::len))
		.saturating_add(item.client_id.as_deref().map_or(0, str::len))
}
