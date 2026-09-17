//! Editing transactions and secondary selections for the programmer composer.
use super::*;

#[derive(Clone)]
pub(super) struct Snapshot {
	content: String,
	primary: Range<usize>,
	extra: Vec<Range<usize>>,
}

impl ComposerInput {
	pub(super) fn move_multiple(&mut self, right: bool, cx: &mut Context<Self>) {
		self.selected_range = moved_range(&self.content, &self.selected_range, right);
		self.extra =
			self.extra.iter().map(|range| moved_range(&self.content, range, right)).collect();
		self.extra.sort_by_key(|r| r.start);
		self.extra.dedup();
		self.extra.retain(|r| r != &self.selected_range);
		self.selection_reversed = false;
		self.marked_range = None;
		self.scroll_manually = false;
		cx.notify();
	}

	pub(crate) fn programmer(&self) -> bool {
		self.programmer
	}

	pub(crate) fn set_programmer(&mut self, enabled: bool, cx: &mut Context<Self>) {
		self.programmer = enabled;
		self.extra.clear();
		self.marked_range = None;
		self.changed(cx);
	}

	pub(crate) fn cursor_count(&self) -> usize {
		self.extra.len() + 1
	}

	pub(super) fn selections(&self) -> Vec<Range<usize>> {
		let mut ranges = self.extra.clone();
		ranges.push(self.selected_range.clone());
		ranges.sort_by_key(|range| (range.start, range.end));
		ranges.dedup();
		ranges
	}

	fn snapshot(&self) -> Snapshot {
		Snapshot {
			content: self.content.clone(),
			primary: self.selected_range.clone(),
			extra: self.extra.clone(),
		}
	}

	pub(super) fn checkpoint(&mut self) {
		if self.marked_range.is_none() {
			if self.undo.len() >= 100 {
				self.undo.remove(0);
			}
			self.undo.push(self.snapshot());
			self.redo.clear();
		}
	}

	fn restore(&mut self, snapshot: Snapshot, cx: &mut Context<Self>) {
		self.content = snapshot.content;
		self.selected_range = snapshot.primary;
		self.extra = snapshot.extra;
		self.marked_range = None;
		self.selection_reversed = false;
		self.last_layout = None;
		self.changed(cx);
	}

	pub(super) fn undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
		if let Some(snapshot) = self.undo.pop() {
			self.redo.push(self.snapshot());
			self.restore(snapshot, cx);
		}
	}

	pub(super) fn redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
		if let Some(snapshot) = self.redo.pop() {
			self.undo.push(self.snapshot());
			self.restore(snapshot, cx);
		}
	}

	pub(super) fn cancel_cursors(
		&mut self,
		_: &CancelCursors,
		_: &mut Window,
		cx: &mut Context<Self>,
	) {
		self.extra.clear();
		self.marked_range = None;
		self.scroll_manually = false;
		cx.notify();
	}

	pub(super) fn select_next(&mut self, _: &SelectNext, _: &mut Window, cx: &mut Context<Self>) {
		if self.selected_range.is_empty() {
			self.selected_range = word_range(&self.content, self.cursor_offset());
			self.selection_reversed = false;
		} else {
			let needle = &self.content[self.selected_range.clone()];
			let occupied = self.selections();
			let from = self.selected_range.end;
			let found = self
				.content
				.match_indices(needle)
				.map(|(start, _)| start..start + needle.len())
				.collect::<Vec<_>>();
			if let Some(range) = found
				.iter()
				.filter(|r| r.start >= from)
				.chain(found.iter().filter(|r| r.start < from))
				.find(|r| !occupied.iter().any(|s| r.start < s.end && s.start < r.end))
			{
				self.extra.push(self.selected_range.clone());
				self.selected_range = range.clone();
				self.selection_reversed = false;
			}
		}
		self.scroll_manually = false;
		cx.notify();
	}

	pub(super) fn add_above(&mut self, _: &AddAbove, _: &mut Window, cx: &mut Context<Self>) {
		self.add_vertical(-1, cx);
	}

	pub(super) fn add_below(&mut self, _: &AddBelow, _: &mut Window, cx: &mut Context<Self>) {
		self.add_vertical(1, cx);
	}

	fn add_vertical(&mut self, direction: i32, cx: &mut Context<Self>) {
		let offset = self.cursor_offset();
		let lines: Vec<&str> = self.content.split('\n').collect();
		let row = self.content[..offset].bytes().filter(|b| *b == b'\n').count();
		let start = self.content[..offset].rfind('\n').map_or(0, |i| i + 1);
		let column =
			self.vertical_column.unwrap_or_else(|| self.content[start..offset].chars().count());
		let target = row as i32 + direction;
		if target < 0 || target >= lines.len() as i32 {
			return;
		}
		let target = target as usize;
		let start: usize = lines[..target].iter().map(|s| s.len() + 1).sum();
		let index = start
			+ lines[target].char_indices().nth(column).map_or(lines[target].len(), |(i, _)| i);
		let range = index..index;
		if self.selections().contains(&range) {
			return;
		}
		self.extra.push(self.selected_range.clone());
		self.selected_range = range;
		self.selection_reversed = false;
		self.vertical_column = Some(column);
		self.scroll_manually = false;
		cx.notify();
	}

	pub(super) fn edit_multiple(
		&mut self,
		primary: Range<usize>,
		text: &str,
		mark: bool,
		selection: Option<&Range<usize>>,
		cx: &mut Context<Self>,
	) {
		self.checkpoint();
		let mut ranges = self.extra.clone();
		ranges.push(primary.clone());
		ranges.sort_by_key(|r| (r.start, r.end));
		let mut merged: Vec<Range<usize>> = Vec::new();
		for range in ranges {
			if let Some(last) = merged.last_mut()
				&& (range.start < last.end || *last == range)
			{
				last.end = last.end.max(range.end);
			} else {
				merged.push(range);
			}
		}
		let removed: usize = merged.iter().map(|r| r.len()).sum();
		let budget = MAX_COMPOSER_BYTES.saturating_sub(self.content.len() - removed) / merged.len();
		let text = bounded_input(text, budget);
		let mut shift = 0isize;
		let mut selections = Vec::new();
		let mut primary_index = 0;
		for (index, range) in merged.iter().enumerate() {
			let start = (range.start as isize + shift) as usize;
			let end = start + text.len();
			if range.start <= primary.start && range.end >= primary.end {
				primary_index = index;
			}
			selections.push(if mark { start..end } else { end..end });
			shift += text.len() as isize - range.len() as isize;
		}
		for range in merged.into_iter().rev() {
			self.content.replace_range(range, &text);
		}
		let primary = selections.remove(primary_index);
		self.extra = selections;
		self.marked_range = mark.then_some(primary.clone());
		self.selected_range = if mark {
			let relative =
				selection.map(|r| range_from_utf16(&text, r)).unwrap_or(text.len()..text.len());
			primary.start + relative.start..primary.start + relative.end
		} else {
			primary
		};
		self.selection_reversed = false;
		self.last_layout = None;
		self.changed(cx);
	}

	pub(super) fn delete_multiple(&mut self, backward: bool, cx: &mut Context<Self>) {
		let before = self.snapshot();
		let expand = |range: Range<usize>| {
			if range.is_empty() {
				if backward {
					previous_boundary(&self.content, range.start)..range.end
				} else {
					range.start..next_boundary(&self.content, range.end)
				}
			} else {
				range
			}
		};
		let primary = expand(self.selected_range.clone());
		self.extra = self.extra.iter().cloned().map(expand).collect();
		self.edit_multiple(primary, "", false, None, cx);
		if let Some(snapshot) = self.undo.last_mut() {
			*snapshot = before;
		}
	}

	pub(super) fn indent(&mut self, _: &Indent, window: &mut Window, cx: &mut Context<Self>) {
		self.replace_text_in_range(None, "    ", window, cx);
	}
}

fn moved_range(content: &str, range: &Range<usize>, right: bool) -> Range<usize> {
	let index = if right {
		if range.is_empty() { next_boundary(content, range.end) } else { range.end }
	} else if range.is_empty() {
		previous_boundary(content, range.start)
	} else {
		range.start
	};
	index..index
}

fn word_range(text: &str, offset: usize) -> Range<usize> {
	let word = |c: char| c.is_alphanumeric() || c == '_';
	let mut start = offset;
	let mut end = offset;
	while start > 0 {
		let previous = previous_boundary(text, start);
		if !text[previous..start].chars().all(word) {
			break;
		}
		start = previous;
	}
	while end < text.len() {
		let next = next_boundary(text, end);
		if !text[end..next].chars().all(word) {
			break;
		}
		end = next;
	}
	start..end
}

#[cfg(test)]
mod tests {
	use super::*;
	fn open<'a>(
		cx: &'a mut gpui::TestAppContext,
		content: &str,
	) -> (Entity<ComposerInput>, &'a mut gpui::VisualTestContext) {
		cx.update(bind_keys);
		let (input, visual) = cx.add_window_view(|_, cx| ComposerInput::new(0, cx));
		input.update(visual, |input, cx| {
			input.set_programmer(true, cx);
			input.set_content(content, cx);
			input.move_to(0, cx);
		});
		visual.update(|window, cx| {
			window.focus(&input.focus_handle(cx), cx);
			window.draw(cx).clear();
		});
		(input, visual)
	}
	#[gpui::test]
	fn select_occurrences_replace_and_undo(cx: &mut gpui::TestAppContext) {
		let (input, visual) = open(cx, "alpha beta alpha alpha");
		visual.simulate_keystrokes("cmd-d cmd-d cmd-d");
		input.update(visual, |input, _| assert_eq!(input.cursor_count(), 3));
		visual.update(|window, cx| {
			input.update(cx, |input, cx| input.replace_text_in_range(None, "x", window, cx))
		});
		input.update(visual, |input, _| assert_eq!(input.content(), "x beta x x"));
		visual.simulate_keystrokes("cmd-z");
		input.update(visual, |input, _| {
			assert_eq!(input.content(), "alpha beta alpha alpha");
			assert_eq!(input.cursor_count(), 3);
		});
		visual.simulate_keystrokes("cmd-shift-z backspace");
		input.update(visual, |input, _| assert_eq!(input.content(), " beta  "));
	}
	#[gpui::test]
	fn vertical_cursors_keep_unicode_column_and_enter_inserts(cx: &mut gpui::TestAppContext) {
		let (input, visual) = open(cx, "一二三\na\nxyz");
		input.update(visual, |input, cx| input.move_to("一二".len(), cx));
		visual.simulate_keystrokes("alt-down alt-down");
		input.update(visual, |input, _| {
			assert_eq!(input.cursor_count(), 3);
			assert_eq!(input.selected_range.start, "一二三\na\nxy".len());
		});
		visual.update(|window, cx| {
			input.update(cx, |input, cx| input.replace_text_in_range(None, "!", window, cx))
		});
		input.update(visual, |input, _| assert_eq!(input.content(), "一二!三\na!\nxy!z"));
		visual.simulate_keystrokes("escape enter");
		input.update(visual, |input, _| {
			assert_eq!(input.cursor_count(), 1);
			assert_eq!(input.content(), "一二!三\na!\nxy!\nz");
		});
	}
	#[gpui::test]
	fn composition_replaces_all_marked_ranges_atomically(cx: &mut gpui::TestAppContext) {
		let (input, visual) = open(cx, "a a");
		visual.simulate_keystrokes("cmd-d cmd-d");
		visual.update(|window, cx| {
			input.update(cx, |input, cx| {
				input.replace_and_mark_text_in_range(None, "ni", Some(2..2), window, cx);
				input.replace_and_mark_text_in_range(None, "你", Some(1..1), window, cx);
				input.replace_text_in_range(None, "你", window, cx);
				assert_eq!(input.content(), "你 你");
				input.undo(&Undo, window, cx);
				assert_eq!(input.content(), "a a");
			})
		});
	}
}
