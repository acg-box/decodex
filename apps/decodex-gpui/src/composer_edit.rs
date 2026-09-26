//! Undo and redo for the conversation draft.
use super::{ComposerInput, Context, Range, Redo, Undo, Window};

#[derive(Clone)]
pub(super) struct Snapshot {
	content: String,
	native_part: Option<decodex_protocol::PromptDraft>,
	bytes: usize,
	primary: Range<usize>,
}
impl ComposerInput {
	fn snapshot(&self) -> Snapshot {
		Snapshot {
			content: self.content.clone(),
			native_part: self.native_part.clone(),
			bytes: self.content.len()
				+ self.native_part.as_ref().map_or(0, |part| part.parts()[0].to_string().len()),
			primary: self.selected_range.clone(),
		}
	}

	pub(super) fn checkpoint(&mut self) {
		if self.marked_range.is_none() {
			if self.undo.len() >= 100 {
				self.undo.remove(0);
			}
			self.undo.push(self.snapshot());
			// Large restored inputs must not turn 100 undo entries into gigabytes.
			let mut bytes: usize = self.undo.iter().map(|snapshot| snapshot.bytes).sum();
			while bytes > 16 * 1024 * 1024 && self.undo.len() > 1 {
				bytes -= self.undo.remove(0).bytes;
			}
			self.redo.clear();
		}
	}

	fn restore(&mut self, snapshot: Snapshot, cx: &mut Context<Self>) {
		self.content = snapshot.content;
		self.native_part = snapshot.native_part;
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
