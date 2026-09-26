//! Restore a native text part without dropping its markers or undo metadata.
use super::*;

pub(super) const MAX_NATIVE_EDITOR_BYTES: usize = 8 * 1024 * 1024;

impl ComposerInput {
	/// Install one complete canonical text part. Other parts stay with the prompt owner.
	pub(crate) fn set_native_part(
		&mut self,
		part: serde_json::Value,
		cx: &mut Context<Self>,
	) -> Result<(), &'static str> {
		if part["type"].as_str() != Some("text") {
			return Err("Input part is not text");
		}
		let text = part["text"].as_str().ok_or("Input text is missing")?;
		if text.len() > MAX_NATIVE_EDITOR_BYTES {
			return Err("Native editor text is too large");
		}
		let text = text.to_owned();
		let native = decodex_protocol::PromptDraft::new(vec![part])?;
		self.content = text;
		self.native_part = Some(native);
		self.selected_range = self.content.len()..self.content.len();
		self.marked_range = None;
		self.selection_reversed = false;
		self.last_layout = None;
		self.undo.clear();
		self.redo.clear();
		self.changed(cx);
		Ok(())
	}

	/// Current native text and marker state for durable draft capture.
	pub(crate) fn native_part(&self) -> Option<&serde_json::Value> {
		self.native_part.as_ref().map(|part| &part.parts()[0])
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use serde_json::json;

	#[gpui::test]
	fn large_native_edits_keep_a_bounded_undo_history(cx: &mut gpui::TestAppContext) {
		let input = cx.new(|cx| ComposerInput::new(0, cx));
		input.update(cx, |input, cx| {
			input
				.set_native_part(json!({"type":"text","text":"x".repeat(3 * 1024 * 1024)}), cx)
				.unwrap();
			for replacement in ["a", "b", "c", "d", "e"] {
				input.replace_bytes(0..1, replacement, false, None, cx);
			}
			assert!(!input.undo.is_empty());
			assert!(input.undo.len() <= 2);
			assert!(input.content().starts_with('e'));
		});
	}

	#[gpui::test]
	fn native_editor_restores_long_text_and_undoes_marker_offsets(cx: &mut gpui::TestAppContext) {
		let (input, visual) = cx.add_window_view(|_, cx| ComposerInput::new(0, cx));
		let original = json!({"type":"text","text":format!("你 $skill {}", "tail".repeat(5000)),"text_elements":[{"byteRange":{"start":4,"end":10},"placeholder":"$skill"}],"extension":"retain"});
		input.update(visual, |input, cx| {
			input.set_native_part(original.clone(), cx).unwrap();
			assert_eq!(input.content(), original["text"].as_str().unwrap());
			input.replace_bytes(0..3, "hello", false, None, cx);
			assert_eq!(input.native_part().unwrap()["text_elements"][0]["byteRange"]["start"], 6);
		});
		visual.update(|window, cx| input.update(cx, |input, cx| input.undo(&Undo, window, cx)));
		input.update(visual, |input, _| assert_eq!(input.native_part(), Some(&original)));
		visual.update(|window, cx| input.update(cx, |input, cx| input.redo(&Redo, window, cx)));
		input.update(visual, |input, cx| {
			assert!(input.content().starts_with("hello $skill"));
			assert_eq!(input.native_part().unwrap()["extension"], "retain");
			let before = input.native_part().cloned();
			input.replace_bytes(7..8, "x", false, None, cx);
			assert_eq!(input.native_part(), before.as_ref());
			input.set_content("ordinary draft", cx);
			assert!(input.native_part().is_none());
			assert_eq!(input.content(), "ordinary draft");
		});
	}
}
