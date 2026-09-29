//! User-requested Markdown export; loaded excerpts are explicit alternatives.
use super::*;
use std::{io::Write, os::unix::fs::OpenOptionsExt, path::Path};

impl AgentSurface {
	pub(super) fn transcript_panel(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
		if self.native_goal_target().is_none() {
			return div().into_any_element();
		}
		let mut panel = div().flex().flex_col().gap_2().child("Conversation export");
		if self.transcript_busy {
			return panel.child("Reading conversation…").into_any_element();
		}
		for (id, label, save, loaded) in [
			("transcript-copy", "Copy Markdown", false, false),
			("transcript-save", "Save Markdown…", true, false),
			("transcript-copy-loaded", "Copy loaded excerpt", false, true),
			("transcript-save-loaded", "Save loaded excerpt…", true, true),
		] {
			if loaded && !self.transcript_failed {
				continue;
			}
			panel = panel.child(self.workspace_action(
				id.into(),
				label.into(),
				move |s, cx| s.export_transcript(save, loaded, cx),
				cx,
			));
		}
		panel.into_any_element()
	}

	fn loaded_transcript(&self, target: &(String, String)) -> Option<String> {
		let mut text = String::from(
			"# Loaded conversation excerpt\n\nThis is the currently loaded display text. Earlier history or shortened content can be missing.\n\n",
		);
		let start = text.len();
		if self
			.native_history
			.binding
			.as_ref()
			.is_some_and(|b| (&b.work, &b.thread) == (&target.0, &target.1))
		{
			for item in self.native_history.visible_export_items() {
				if let decodex_protocol::AgentTimelineContent::Item {
					kind,
					text: body,
					truncated,
					..
				} = item
				{
					if body.is_empty() {
						continue;
					}
					text.push_str(&format!("## {kind}\n\n{body}\n\n"));
					if *truncated {
						text.push_str("[Content shortened in the display]\n\n");
					}
				}
			}
		} else if self.snapshot.as_ref().is_some_and(|s| {
			s.work_items.iter().any(|w| {
				w.id == target.0 && w.codex_thread_id.as_deref() == Some(target.1.as_str())
			})
		}) && let Some((work, AgentHistoryResult::Available { entries, .. })) =
			&self.history
			&& work == &target.0
		{
			for entry in entries {
				text.push_str(&format!("## {}\n\n{}\n\n", entry.kind, entry.text));
			}
		}
		(text.len() > start).then_some(text)
	}

	fn export_transcript(&mut self, save: bool, loaded: bool, cx: &mut Context<Self>) {
		if self.transcript_busy {
			return;
		}
		let (Some(target), Some(profile), Some(source)) = (
			self.native_goal_target(),
			self.profile.clone(),
			self.snapshot.as_ref().and_then(|s| s.runtime_source.clone()),
		) else {
			return;
		};
		let (Ok(work), Ok(thread)) =
			(EntityId::new(target.0.clone()), EntityId::new(target.1.clone()))
		else {
			return;
		};
		let excerpt = loaded.then(|| self.loaded_transcript(&target)).flatten();
		if loaded && excerpt.is_none() {
			self.feedback = "No loaded conversation text is available.".into();
			cx.notify();
			return;
		}
		let directory =
			std::env::var_os("HOME").map(std::path::PathBuf::from).unwrap_or_else(|| "/tmp".into());
		let destination = save.then(|| {
			cx.prompt_for_new_path(
				&directory,
				Some(if loaded { "conversation-excerpt.md" } else { "conversation.md" }),
			)
		});
		self.transcript_busy = true;
		cx.notify();
		cx.spawn(async move |surface,cx| {
			let path = match destination {
				Some(destination) => match destination.await {
					Ok(Ok(Some(path)))=>Some(path),
					_=>{let _=surface.update(cx,|s,cx|{s.transcript_busy=false;cx.notify();});return;}
				},
				None=>None,
			};
			let result = cx.background_executor().spawn(async move {
				if let Some(text)=excerpt {return Ok(text);}
				let runtime=tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|_|())?;
				runtime.block_on(AgentClient::new(profile).transcript(work,thread)).map_err(|_|())
			}).await;
			let valid=surface.update(cx,|s,cx| {
				if s.native_goal_target()!=Some(target) || s.snapshot.as_ref().and_then(|s|s.runtime_source.as_ref())!=Some(&source) {
					s.transcript_busy=false;s.feedback="Conversation source changed. Export again from the selected conversation.".into();cx.notify();return false;
				}
				true
			}).unwrap_or(false);
			if !valid {return;}
			let Ok(text)=result else {
				let _=surface.update(cx,|s,cx|{s.transcript_busy=false;s.transcript_failed=true;s.feedback="Complete history could not be exported. Retry, or explicitly export the loaded excerpt.".into();cx.notify();});return;
			};
			let saved = if let Some(path)=path {
				cx.background_executor().spawn(async move {write_transcript(&path,&text)}).await
			} else {
				let _=surface.update(cx,|_,cx|cx.write_to_clipboard(gpui::ClipboardItem::new_string(text)));Ok(())
			};
			let _=surface.update(cx,|s,cx| {
				s.transcript_busy=false;
				s.transcript_failed=false;
				s.feedback=match saved {Ok(())=>if loaded {"Loaded excerpt exported."} else {"Conversation exported as Markdown."},Err(reason)=>reason}.into();cx.notify();
			});
		}).detach();
	}
}
fn write_transcript(path: &Path, text: &str) -> Result<(), &'static str> {
	let mut file = std::fs::OpenOptions::new()
		.write(true)
		.create_new(true)
		.mode(0o600)
		.open(path)
		.map_err(|_| "Cannot create export. Choose a new filename.")?;
	file.write_all(text.as_bytes())
		.and_then(|_| file.sync_all())
		.map_err(|_| "The file could not be saved completely.")
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn transcript_file_preserves_markdown_and_does_not_overwrite() {
		let directory = tempfile::tempdir().unwrap();
		let file = directory.path().join("conversation.md");
		let text = "# Conversation\n\n中文 **answer**\n```rust\nlet x = 1;\n```\n";
		write_transcript(&file, text).unwrap();
		assert_eq!(std::fs::read_to_string(&file).unwrap(), text);
		assert!(write_transcript(&file, "replacement").is_err());
		assert_eq!(std::fs::read_to_string(&file).unwrap(), text);
	}
}
