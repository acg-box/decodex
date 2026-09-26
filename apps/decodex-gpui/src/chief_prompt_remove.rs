//! Explicit local input removal. No history mutation or submission occurs here.
use super::*;
use std::collections::BTreeSet;

pub(super) struct Removal {
	pub(super) key: String,
	before: DesktopPromptEditDraft,
	part: usize,
	markers: BTreeSet<(usize, usize)>,
}

pub(super) fn label(input: &PromptDraft, index: usize) -> String {
	let part = &input.parts()[index];
	let kind = part["type"].as_str().unwrap_or("input");
	let title = match kind {
		"skill" | "mention" => part["name"].as_str(),
		"image" => part["fileId"].as_str(),
		"localImage" | "localAudio" => part["path"]
			.as_str()
			.and_then(|path| std::path::Path::new(path).file_name())
			.and_then(|name| name.to_str()),
		_ => None,
	};
	let kind = match kind {
		"image" | "localImage" => {
			let number = input.parts()[..=index]
				.iter()
				.filter(|part| matches!(part["type"].as_str(), Some("image" | "localImage")))
				.count();
			format!("Image #{number}")
		},
		"audio" | "localAudio" => "audio".into(),
		"mention" => "reference".into(),
		other => other.into(),
	};
	match title {
		Some(title) => format!("{kind}: {}", title.chars().take(80).collect::<String>()),
		None => kind,
	}
}

impl ChiefSurface {
	pub(in super::super) fn begin_prompt_removal(
		&mut self,
		part: usize,
		expected: &DesktopPromptEditDraft,
		cx: &mut Context<Self>,
	) {
		if !self.prompt_editor_source_current()
			|| self.prompt_edit.draft.as_ref() != Some(expected)
			|| expected.input.parts().get(part).is_none_or(|part| part["type"] == "text")
		{
			return;
		}
		self.prompt_edit.removal = Some(Removal {
			key: unique_command(),
			before: expected.clone(),
			part,
			markers: BTreeSet::new(),
		});
		cx.notify();
	}

	pub(super) fn apply_prompt_removal(&mut self, key: &str, cx: &mut Context<Self>) {
		if self.prompt_edit.removal.as_ref().is_none_or(|removal| removal.key != key) {
			return;
		}
		let Some(removal) = self.prompt_edit.removal.take() else {
			return;
		};
		if !self.prompt_editor_source_current()
			|| self.prompt_edit.draft.as_ref() != Some(&removal.before)
			|| self.prompt_edit.editors.iter().any(|(index, editor)| {
				editor.read(cx).native_part() != removal.before.input.parts().get(*index)
			}) {
			self.prompt_edit.feedback =
				"The draft changed. Select the input to remove again.".into();
			cx.notify();
			return;
		}
		let mut draft = removal.before;
		let markers: Vec<_> = removal.markers.into_iter().collect();
		let result = draft.input.remove_bound_part(removal.part, &markers).and_then(|()| {
			let mut editors = Vec::new();
			for (index, part) in
				draft.input.parts().iter().enumerate().filter(|(_, part)| part["type"] == "text")
			{
				let old_index = if index >= removal.part { index + 1 } else { index };
				let retained = self
					.prompt_edit
					.editors
					.iter()
					.find(|(previous, editor)| {
						*previous == old_index && editor.read(cx).native_part() == Some(part)
					})
					.map(|(_, editor)| editor.clone());
				let editor = match retained {
					Some(editor) => editor,
					None => {
						let editor = cx.new(|cx| ComposerInput::new(0, cx));
						editor.update(cx, |editor, cx| editor.set_native_part(part.clone(), cx))?;
						editor
					},
				};
				editors.push((index, editor));
			}
			self.bind_prompt_editors(draft, editors, cx)
		});
		self.prompt_edit.feedback = match result {
			Ok(()) => "Input removed from this draft. History is unchanged.".into(),
			Err(error) => error.into(),
		};
		cx.notify();
	}

	pub(in super::super) fn prompt_removal_panel(
		&self,
		cx: &mut Context<Self>,
	) -> gpui::AnyElement {
		let Some(removal) = &self.prompt_edit.removal else {
			return div().into_any_element();
		};
		let mut panel = div()
			.w_full()
			.min_w_0()
			.flex_none()
			.flex()
			.flex_col()
			.gap_2()
			.child(format!(
				"Remove {} from this draft?",
				label(&removal.before.input, removal.part)
			))
			.child(
				"Also select any references to remove from the message. Unselected text is kept.",
			);
		if matches!(
			removal.before.input.parts()[removal.part]["type"].as_str(),
			Some("image" | "localImage")
		) {
			panel = panel.child("Remaining images will be numbered in their new order. Review any image numbers in your message.");
		}
		for (part_index, part) in removal.before.input.parts().iter().enumerate() {
			let Some(elements) = part["text_elements"].as_array() else {
				continue;
			};
			let text = part["text"].as_str().unwrap_or_default();
			for (element_index, element) in elements.iter().enumerate() {
				let title = element["placeholder"]
					.as_str()
					.or_else(|| {
						let start =
							usize::try_from(element["byteRange"]["start"].as_u64()?).ok()?;
						let end = usize::try_from(element["byteRange"]["end"].as_u64()?).ok()?;
						text.get(start..end)
					})
					.unwrap_or("Text reference")
					.chars()
					.take(100)
					.collect::<String>();
				let identity = (part_index, element_index);
				let key = removal.key.clone();
				panel = panel.child(super::super::mcp_forms::mcp_button(
					format!("prompt-remove-marker-{part_index}-{element_index}"),
					title,
					removal.markers.contains(&identity),
					cx,
					move |s, cx| {
						if let Some(removal) = &mut s.prompt_edit.removal
							&& removal.key == key && !removal.markers.insert(identity)
						{
							removal.markers.remove(&identity);
						}
						cx.notify();
					},
				));
			}
		}
		let confirm_key = removal.key.clone();
		let cancel_key = removal.key.clone();
		panel
			.child(self.workspace_action(
				"prompt-remove-confirm".into(),
				"Remove selected input and references".into(),
				move |s, cx| s.apply_prompt_removal(&confirm_key, cx),
				cx,
			))
			.child(self.workspace_action(
				"prompt-remove-cancel".into(),
				"Cancel".into(),
				move |s, cx| {
					if s.prompt_edit
						.removal
						.as_ref()
						.is_some_and(|removal| removal.key == cancel_key)
					{
						s.prompt_edit.removal = None;
					}
					cx.notify();
				},
				cx,
			))
			.into_any_element()
	}
}
