//! Restore task markers consumed before blocks, without duplicating delayed parser events.
//! Based on the behavior reviewed in openai/codex 5a11c456060c2ce131de1ba195fc7729f7d7db66.
use std::{iter, ops::Range};

use pulldown_cmark::{Event, Tag, TagEnd};

pub(super) fn events<'a>(
	input: &'a str,
	events: impl Iterator<Item = (Event<'a>, Range<usize>)>,
) -> impl Iterator<Item = (Event<'a>, Range<usize>)> {
	let mut events = events.peekable();
	let mut recovered = None;
	let mut marker_end = 0;

	iter::from_fn(move || {
		if let Some(marker) = recovered.take() {
			return Some(marker);
		}
		loop {
			let (event, range) = events.next()?;
			if matches!(event, Event::Start(Tag::Item)) {
				if let Some((next, next_range)) = events.peek() {
					let end = if matches!(next, Event::End(TagEnd::Item)) {
						range.end
					} else {
						next_range.start
					};
					if let Some(checked) = consumed_marker(input, range.start..end) {
						marker_end = end;
						recovered = Some((Event::TaskListMarker(checked), range.start..end));
					}
				}
			} else if matches!(event, Event::TaskListMarker(_)) && range.start < marker_end {
				continue;
			}
			return Some((event, range));
		}
	})
}

fn consumed_marker(input: &str, range: Range<usize>) -> Option<bool> {
	let prefix = input.get(range)?.lines().next()?.trim();
	let (_, marker) = prefix.split_once(char::is_whitespace)?;
	match marker.trim() {
		"[ ]" => Some(false),
		"[x]" | "[X]" => Some(true),
		_ => None,
	}
}
