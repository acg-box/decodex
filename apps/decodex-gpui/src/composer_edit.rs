//! Undo and redo for the conversation draft.
use std::time::{Duration, Instant};

use crate::composer_input::{ComposerInput, Context, Range, Redo, Undo, Window};
use decodex_protocol::PromptDraft;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum EditGroup {
	Typing,
	Backspace,
	Delete,
}

pub(super) enum Edit {
	Replace,
	Group(EditGroup),
	Marked(Option<Range<usize>>),
}

#[derive(Clone)]
pub(super) struct Snapshot {
	content: String,
	native_part: Option<PromptDraft>,
	bytes: usize,
	selection: Range<usize>,
	reversed: bool,
	after_wrap: bool,
}
impl ComposerInput {
	fn snapshot(&self) -> Snapshot {
		Snapshot {
			content: self.content.clone(),
			native_part: self.native_part.clone(),
			bytes: self.content.len()
				+ self.native_part.as_ref().map_or(0, |part| part.parts()[0].to_string().len()),
			selection: self.selected_range.clone(),
			reversed: self.selection_reversed,
			after_wrap: self.caret_after_wrap,
		}
	}

	pub(super) fn checkpoint(&mut self, edit: &Edit) {
		if self.marked_range.is_some() {
			self.edit_group = None;
			return;
		}

		let previous = self.edit_group.take();
		if let Edit::Group(kind) = edit {
			let now = Instant::now();
			self.edit_group = Some((*kind, now));
			if previous.is_some_and(|(previous_kind, at)| {
				previous_kind == *kind && now.duration_since(at) <= Duration::from_secs(1)
			}) {
				return;
			}
		}

		if self.undo.len() >= 100 {
			self.undo.remove(0);
		}

		self.undo.push(self.snapshot());
		self.redo.clear();

		// Large restored inputs must not turn 100 undo entries into gigabytes.
		let mut bytes: usize = self.undo.iter().map(|snapshot| snapshot.bytes).sum();

		while bytes > 16 * 1_024 * 1_024 && self.undo.len() > 1 {
			bytes -= self.undo.remove(0).bytes;
		}
	}

	fn restore(&mut self, snapshot: Snapshot, cx: &mut Context<Self>) {
		self.content = snapshot.content;
		self.native_part = snapshot.native_part;
		self.selected_range = snapshot.selection;
		self.marked_range = None;
		self.selection_reversed = snapshot.reversed;
		self.caret_after_wrap = snapshot.after_wrap;
		self.edit_group = None;
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
}
