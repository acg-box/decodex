//! Undo and redo for the conversation draft.
use super::{ComposerInput, Context, Range, Redo, Undo, Window};

#[derive(Clone)]
pub(super) struct Snapshot {
	content: String,
	primary: Range<usize>,
}
impl ComposerInput {
	fn snapshot(&self) -> Snapshot {
		Snapshot { content: self.content.clone(), primary: self.selected_range.clone() }
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
}
