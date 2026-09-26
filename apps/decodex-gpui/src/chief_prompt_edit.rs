//! Source-bound native prompt review. Opening or editing this panel never reverts history.
use super::*;
use decodex_protocol::{DesktopPromptEditDraft, PromptDraft, PromptEditPhase};
#[path = "chief_prompt_confirm.rs"] mod confirmation;
#[path = "chief_prompt_handback.rs"] mod handback;
#[path = "chief_prompt_remove.rs"] mod removal;

#[derive(Default)]
pub(super) struct Panel {
	key: String,
	profile: Option<ClientProfile>,
	work: String,
	thread: String,
	draft: Option<DesktopPromptEditDraft>,
	editors: Vec<(usize, Entity<ComposerInput>)>,
	subscriptions: Vec<gpui::Subscription>,
	task: Option<Task<()>>,
	feedback: String,
	removal: Option<removal::Removal>,
	confirmation: Option<DesktopPromptEditDraft>,
}

impl ChiefSurface {
	fn preflight_prompt_editor(
		&mut self,
		expected: DesktopPromptEditDraft,
		cx: &mut Context<Self>,
	) {
		if self.prompt_edit.task.is_some()
			|| !self.prompt_editor_source_current()
			|| self.prompt_edit.draft.as_ref() != Some(&expected)
			|| !self.command_connection_ready()
		{
			return;
		}
		if let Err(error) = expected.input.validate_native_input() {
			self.prompt_edit.feedback = error.into();
			cx.notify();
			return;
		}
		let Some(profile) = self.profile.clone() else {
			return;
		};
		let execution = self.draft_profiles.execution.choice(expected.work_id.as_str());
		let expected_execution = execution.clone();
		let key = self.prompt_edit.key.clone();
		let draft = expected.clone();
		let (send, receive) = tokio::sync::oneshot::channel();
		let started =
			std::thread::Builder::new().name("prompt-preflight-io".into()).spawn(move || {
				let result = (|| {
					let runtime = tokio::runtime::Builder::new_current_thread()
						.enable_all()
						.build()
						.map_err(|_| "Could not start input check")?;
					runtime
						.block_on(ChiefClient::new(profile).preflight_prompt_input(
							draft.work_id,
							draft.thread_id,
							&draft.input,
							&execution,
						))
						.map_err(
							|_| "Input or current thread settings could not be qualified. History is unchanged.",
						)
				})();
				let _ = send.send(result);
			});
		if started.is_err() {
			self.prompt_edit.feedback = "Could not start input check".into();
			cx.notify();
			return;
		}
		self.prompt_edit.feedback = "Checking edited input and current thread settings…".into();
		self.prompt_edit.task = Some(cx.spawn(async move |surface, cx| {
			let result = receive.await.unwrap_or(Err("Input check stopped"));
			let _ = surface.update(cx, |s, cx| {
				if s.prompt_edit.key != key || !s.prompt_editor_source_current() { return; }
				s.prompt_edit.task = None;
				s.prompt_edit.feedback = if s.prompt_edit.draft.as_ref() != Some(&expected)
					|| s.draft_profiles.execution.choice(expected.work_id.as_str()) != expected_execution {
					"Draft or execution settings changed during the check. Check the current input again."
				} else { match result {
					Ok(()) => "Input fields and request size fit the current thread settings. History is unchanged; local files and remote media still require qualification.",
					Err(message) => message,
				}}.into();
				cx.notify();
			});
		}));
		cx.notify();
	}

	fn recover_prompt_editor(&mut self, expected: DesktopPromptEditDraft, cx: &mut Context<Self>) {
		if self.prompt_edit.task.is_some()
			|| !self.prompt_editor_source_current()
			|| self.prompt_edit.draft.as_ref() != Some(&expected)
			|| !self.command_connection_ready()
		{
			return;
		}
		let Some(profile) = self.profile.clone() else {
			return;
		};
		let key = self.prompt_edit.key.clone();
		let original = expected.clone();
		let (send, receive) = tokio::sync::oneshot::channel();
		let started =
			std::thread::Builder::new().name("prompt-recovery-io".into()).spawn(move || {
				let result = (|| {
					let runtime = tokio::runtime::Builder::new_current_thread()
						.enable_all()
						.build()
						.map_err(|_| "Could not start history recovery")?;
					runtime.block_on(async {
						let client = ChiefClient::new(profile);
						// Recovery only re-reads native history; never replay confirmation or send.
						let _ = client
							.execute(
								ChiefActionDto::RecoverPromptEdit {
									work_id: original.work_id.clone(),
									thread_id: original.thread_id.clone(),
								},
								IdempotencyKey::new(unique_command())
									.map_err(|_| "Invalid recovery identity")?,
							)
							.await;
						let (status, content) = client
							.prompt_edit(original.work_id.clone(), original.thread_id.clone())
							.await
							.map_err(|_| "History edit status is unavailable")?;
						let input =
							PromptDraft::new(content.ok_or("Original input is unavailable")?)?;
						let recovered = original.recover_receipt(&status, &input)?;
						Ok((recovered, status.phase))
					})
				})();
				let _ = send.send(result);
			});
		if started.is_err() {
			self.prompt_edit.feedback = "Could not start history recovery".into();
			cx.notify();
			return;
		}
		self.prompt_edit.feedback = "Reading the history edit receipt…".into();
		self.prompt_edit.task = Some(cx.spawn(async move |surface, cx| {
			let result = receive.await.unwrap_or(Err("History recovery stopped"));
			let _ = surface.update(cx, |s, cx| {
				if s.prompt_edit.key != key || !s.prompt_editor_source_current() { return; }
				s.prompt_edit.task = None;
				if s.prompt_edit.draft.as_ref() != Some(&expected) {
					s.prompt_edit.feedback = "Draft changed during recovery. Read the receipt again.".into();
				} else {
					match result {
						Ok((recovered, phase)) => match s.stage_prompt_editor(recovered.clone(), cx) {
							Ok(()) => {
								s.prompt_edit.draft = Some(recovered);
								s.prompt_edit.feedback = match phase {
									PromptEditPhase::Unchanged => "History is unchanged. Edited input retained; recheck original history before confirming again.",
								PromptEditPhase::Restored => "History edit receipt recovered. Edited input retained; nothing was sent.",
									PromptEditPhase::Applied => "History edit applied. Saving the retained draft; handback is still pending.",
									_ => "History edit remains uncertain. Retain this draft and recover again; do not repeat confirmation.",
								}.into();
								s.history_requested_for = None;
								s.load_history(cx);
							},
							Err(message) => s.prompt_edit.feedback = message.into(),
						},
						Err(message) => s.prompt_edit.feedback = message.into(),
					}
				}
				cx.notify();
			});
		}));
		cx.notify();
	}

	pub(super) fn reset_prompt_edit(&mut self) {
		self.prompt_edit = Panel::default();
	}

	fn reopen_prompt_editor(&mut self, work: &str, review: &str, cx: &mut Context<Self>) {
		if self.selected.as_deref() != Some(work) {
			return;
		}
		let Some(draft) = self
			.saved_prompt_editors(work)
			.into_iter()
			.find(|draft| draft.review_token.as_str() == review)
		else {
			return;
		};
		let pending = draft.handback_pending || draft.receipt_id.is_some();
		self.prompt_edit = Panel {
			key: unique_command(),
			profile: self.profile.clone(),
			work: work.into(),
			thread: draft.thread_id.as_str().into(),
			..Default::default()
		};
		self.prompt_edit.feedback = match self.install_prompt_editors(draft, cx) {
			Ok(()) if pending =>
				"Draft reopened. Read the history edit status before continuing.".into(),
			Ok(()) =>
				"Draft reopened. History must be reviewed again before an edit is confirmed.".into(),
			Err(error) => error.into(),
		};
		cx.notify();
	}

	pub(super) fn invalidate_prompt_edit(&mut self, next: &ChiefSnapshotDto) {
		if self.prompt_edit.work.is_empty() {
			return;
		}
		let before = self.snapshot.as_ref().and_then(|snapshot| {
			snapshot.work_items.iter().find(|work| work.id == self.prompt_edit.work)
		});
		let after = next.work_items.iter().find(|work| work.id == self.prompt_edit.work);
		if self
			.snapshot
			.as_ref()
			.is_none_or(|snapshot| snapshot.runtime_source != next.runtime_source)
			|| !matches!((before, after), (Some(before), Some(after)) if before.codex_thread_id == after.codex_thread_id && before.active_turn_id == after.active_turn_id)
		{
			self.reset_prompt_edit();
		}
	}

	pub(super) fn review_prompt(
		&mut self,
		work: &str,
		thread: &str,
		turn: &str,
		item: &str,
		cx: &mut Context<Self>,
	) {
		self.request_prompt_review((work, thread, turn, item), None, cx);
	}

	fn request_prompt_review(
		&mut self,
		source: (&str, &str, &str, &str),
		previous: Option<DesktopPromptEditDraft>,
		cx: &mut Context<Self>,
	) {
		let (work, thread, turn, item) = source;
		if self.selected.as_deref() != Some(work) || !self.command_connection_ready() {
			return;
		}
		let Some(profile) = self.profile.clone() else { return };
		let (Ok(work_id), Ok(thread_id), Ok(turn_id), Ok(item_id)) =
			(EntityId::new(work), WireText::new(thread), WireText::new(turn), WireText::new(item))
		else {
			return;
		};
		let key = unique_command();
		self.prompt_edit = Panel {
			key: key.clone(),
			profile: Some(profile.clone()),
			work: work.into(),
			thread: thread.into(),
			feedback: "Reading the original input…".into(),
			..Default::default()
		};
		let (send, receive) = tokio::sync::oneshot::channel();
		let request_key = key.clone();
		let started =
			std::thread::Builder::new().name("prompt-review-io".into()).spawn(move || {
				let result = (|| {
					let runtime = tokio::runtime::Builder::new_current_thread()
						.enable_all()
						.build()
						.map_err(|_| "Could not start prompt review")?;
					runtime.block_on(async {
						let client = ChiefClient::new(profile);
						let response = client
							.execute(
								ChiefActionDto::PreparePromptEdit {
									work_id: work_id.clone(),
									thread_id: thread_id.clone(),
									turn_id: turn_id.clone(),
									item_id: item_id.clone(),
								},
								IdempotencyKey::new(&request_key)
									.map_err(|_| "Invalid review identity")?,
							)
							.await
							.map_err(|_| "Prompt review could not be confirmed")?;
						if !matches!(response, ChiefCommandResponse::Accepted { work_id: ref accepted } if accepted == &work_id) { return Err("Prompt review was not accepted"); }
						let (status, content) = client
							.prompt_edit(work_id, thread_id)
							.await
							.map_err(|_| "Original input could not be read")?;
						let evidence =
							status.evidence.ok_or("This input is not available for editing")?;
						if status.phase != PromptEditPhase::Review
							|| evidence.before_turn_id != turn_id
							|| evidence.item_id != item_id
						{
							return Err("This input is not available for editing");
						}
						let input = PromptDraft::new(content.ok_or("Original input is missing")?)?;
						Ok((
							DesktopPromptEditDraft {
								work_id: status.work_id,
								thread_id: status.thread_id,
								before_turn_id: evidence.before_turn_id,
								item_id: evidence.item_id,
								original_hash: input.fingerprint()?,
								review_token: evidence.review_token,
								receipt_id: None,
								handback_pending: false,
								confirmation_key: None,
								input,
							},
							evidence.removed_turns,
						))
					})
				})();
				let _ = send.send(result);
			});
		if started.is_err() {
			self.prompt_edit.feedback = "Could not start prompt review".into();
			cx.notify();
			return;
		}
		self.prompt_edit.task = Some(cx.spawn(async move |surface, cx| {
			let result = receive.await.unwrap_or(Err("Prompt review stopped"));
			let _ = surface.update(cx, |s, cx| {
				if s.prompt_edit.key != key || !s.prompt_editor_source_current() { return; }
				s.prompt_edit.task = None;
				match result {
					Ok((draft, turns)) => {
						let reviewed = match previous.as_ref() { Some(previous) => s.renew_prompt_editor(previous, &draft, cx), None => Ok(draft) };
						if let Err(message) = reviewed.and_then(|draft| s.install_prompt_editors(draft, cx)) {
							s.prompt_edit.feedback = message.into();
						} else {
							s.prompt_edit.feedback = format!("Editing this input would remove {turns} turns, including this one. Workspace file changes would remain. History is unchanged.");
						}
					},
					Err(message) => s.prompt_edit.feedback = message.into(),
				}
				cx.notify();
			});
		}));
		cx.notify();
	}

	fn prompt_editor_source_current(&self) -> bool {
		self.profile.is_some()
			&& self.profile == self.prompt_edit.profile
			&& self.selected.as_deref() == Some(&self.prompt_edit.work)
			&& self.snapshot.as_ref().is_some_and(|snapshot| {
				snapshot.work_items.iter().any(|work| {
					work.id == self.prompt_edit.work
						&& work.codex_thread_id.as_deref() == Some(&self.prompt_edit.thread)
				})
			})
	}

	fn install_prompt_editors(
		&mut self,
		draft: DesktopPromptEditDraft,
		cx: &mut Context<Self>,
	) -> Result<(), &'static str> {
		let mut editors = Vec::new();
		for (index, part) in draft.input.parts().iter().enumerate() {
			if part["type"].as_str() == Some("text") {
				let editor = cx.new(|cx| ComposerInput::new(0, cx));
				editor.update(cx, |editor, cx| editor.set_native_part(part.clone(), cx))?;
				editors.push((index, editor));
			}
		}
		self.bind_prompt_editors(draft, editors, cx)
	}

	fn bind_prompt_editors(
		&mut self,
		draft: DesktopPromptEditDraft,
		editors: Vec<(usize, Entity<ComposerInput>)>,
		cx: &mut Context<Self>,
	) -> Result<(), &'static str> {
		self.stage_prompt_editor(draft.clone(), cx)?;
		self.prompt_edit.task = None;
		self.prompt_edit.confirmation = None;
		self.prompt_edit.key = unique_command();
		self.prompt_edit.draft = Some(draft);
		self.prompt_edit.subscriptions.clear();
		for (index, editor) in &editors {
			let index = *index;
			let key = self.prompt_edit.key.clone();
			self.prompt_edit.subscriptions.push(cx.subscribe(editor, move |s, editor, _, cx| {
				if s.prompt_edit.key != key || !s.prompt_editor_source_current() {
					return;
				}
				let Some(part) = editor.read(cx).native_part().cloned() else { return };
				let Some(mut draft) = s.prompt_edit.draft.clone() else { return };
				let previous = draft.input.parts()[index].clone();
				if part == previous {
					return;
				}
				if let Err(error) = draft
					.input
					.replace_part(index, part)
					.and_then(|()| s.stage_prompt_editor(draft.clone(), cx))
				{
					s.prompt_edit.feedback = error.into();
					let _ = editor.update(cx, |editor, cx| editor.set_native_part(previous, cx));
					cx.notify();
					return;
				}
				s.prompt_edit.draft = Some(draft);
				s.prompt_edit.confirmation = None;
				cx.notify();
			}));
		}
		self.prompt_edit.editors = editors;
		Ok(())
	}

	pub(super) fn prompt_edit_panel(&self, work: &str, cx: &mut Context<Self>) -> gpui::AnyElement {
		let mut saved = div().flex().flex_col().gap_2();
		for draft in self.saved_prompt_editors(work) {
			let review = draft.review_token.as_str().to_owned();
			let owner = work.to_owned();
			let title = draft
				.input
				.parts()
				.iter()
				.find_map(|part| part["text"].as_str())
				.map(|text| text.chars().take(60).collect::<String>())
				.filter(|text| !text.trim().is_empty())
				.unwrap_or_else(|| "Attachments".into());
			let id = format!("saved-prompt-{review}");
			saved = saved.child(self.workspace_action(
				id,
				format!("Edit draft: {title}"),
				move |s, cx| s.reopen_prompt_editor(&owner, &review, cx),
				cx,
			));
			if draft.receipt_id.is_none() && !draft.handback_pending {
				let id = format!("discard-prompt-{}", draft.review_token.as_str());
				saved = saved.child(self.workspace_action(
					id,
					"Discard edit draft".into(),
					move |s, cx| {
						match s.discard_prompt_editor(&draft, cx) {
							Ok(())
								if s.prompt_edit.draft.as_ref().is_some_and(|current| {
									current.review_token == draft.review_token
								}) =>
								s.reset_prompt_edit(),
							Ok(()) => {},
							Err(error) => s.prompt_edit.feedback = error.into(),
						}
						cx.notify();
					},
					cx,
				));
			}
		}
		if self.prompt_edit.work != work || !self.prompt_editor_source_current() {
			return saved.into_any_element();
		}
		let mut panel = saved.child(self.prompt_edit.feedback.clone());
		for (_, editor) in &self.prompt_edit.editors {
			panel = panel.child(editor.clone());
		}
		if let Some(draft) = &self.prompt_edit.draft {
			if draft.receipt_id.is_some() && draft.handback_pending {
				let expected = draft.clone();
				panel = panel.child(self.workspace_action(
					"prompt-handback".into(),
					"Finish restoring draft".into(),
					move |s, cx| s.handback_prompt_editor(expected.clone(), cx),
					cx,
				));
			}
			if draft.receipt_id.is_none() {
				let expected = draft.clone();
				panel = panel.child(
					self.workspace_action(
						"prompt-confirm-review".into(),
						if draft.confirmation_key.is_some() {
							"Continue saved confirmation…"
						} else {
							"Confirm history edit…"
						}
						.into(),
						move |s, cx| {
							if s.prompt_edit.task.is_none()
								&& s.prompt_edit.draft.as_ref() == Some(&expected)
								&& s.prompt_editor_source_current()
							{
								s.prompt_edit.confirmation = Some(expected.clone());
								cx.notify();
							}
						},
						cx,
					),
				);
			}
			if let Some(expected) = &self.prompt_edit.confirmation {
				let expected = expected.clone();
				panel = panel.child("Remove the selected turn and all later turns? Workspace file changes remain. The edited draft will be kept and will not be sent.")
					.child(self.workspace_action("prompt-confirm-apply".into(), "Remove turns and keep draft".into(), move |s, cx| s.confirm_prompt_editor(expected.clone(), cx), cx))
					.child(self.workspace_action("prompt-confirm-cancel".into(), "Cancel".into(), |s, cx| { s.prompt_edit.confirmation = None; cx.notify(); }, cx));
			}
			let expected = draft.clone();
			panel = panel.child(self.workspace_action(
				"prompt-preflight".into(),
				"Check edited input".into(),
				move |s, cx| s.preflight_prompt_editor(expected.clone(), cx),
				cx,
			));
			panel = panel.child(if self.prompt_editor_saved(draft) {
				"Edited draft saved locally."
			} else {
				"Edited draft is not yet confirmed saved locally."
			});
			let expected = draft.clone();
			panel = panel.child(self.workspace_action(
				"prompt-recover".into(),
				"Read history edit receipt".into(),
				move |s, cx| s.recover_prompt_editor(expected.clone(), cx),
				cx,
			));
			if draft.receipt_id.is_none() && !draft.handback_pending {
				let original = draft.clone();
				panel = panel.child(self.workspace_action(
					"prompt-review-refresh".into(),
					"Recheck original history".into(),
					move |s, cx| {
						let source = (
							original.work_id.as_str(),
							original.thread_id.as_str(),
							original.before_turn_id.as_str(),
							original.item_id.as_str(),
						);
						s.request_prompt_review(source, Some(original.clone()), cx);
					},
					cx,
				));
			}
			for (index, part) in draft
				.input
				.parts()
				.iter()
				.enumerate()
				.filter(|(_, part)| part["type"].as_str() != Some("text"))
			{
				let expected = draft.clone();
				panel = panel.child(self.workspace_action(
					format!("prompt-remove-{index}"),
					format!("Remove {}…", removal::label(part)),
					move |s, cx| s.begin_prompt_removal(index, &expected, cx),
					cx,
				));
			}
		}
		panel = panel.child(self.prompt_removal_panel(cx));
		panel
			.child(self.workspace_action(
				"prompt-review-close".into(),
				"Close review".into(),
				|s, cx| {
					s.prompt_edit = Panel::default();
					cx.notify();
				},
				cx,
			))
			.into_any_element()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::os::unix::fs::{MetadataExt, PermissionsExt};
	fn original_input() -> PromptDraft {
		PromptDraft::new(vec![
			serde_json::json!({"type":"text","text":"Original"}),
			serde_json::json!({"type":"image","fileId":"native-image"}),
		])
		.unwrap()
	}

	#[gpui::test]
	fn prompt_review_keeps_the_main_composer_and_stops_after_source_invalidation(
		cx: &mut gpui::TestAppContext,
	) {
		let root = tempfile::tempdir().unwrap();
		let path = root.path().canonicalize().unwrap();
		std::fs::create_dir(path.join("server")).unwrap();
		std::fs::set_permissions(path.join("server"), std::fs::Permissions::from_mode(0o700))
			.unwrap();
		let uid = std::fs::metadata(&path).unwrap().uid();
		let config = path.join("config.toml");
		std::fs::write(&config, format!("version = 1\nactive_profile = \"local\"\ncache = {{}}\n[profiles.local]\nkind = \"local\"\npolicy = \"same_uid\"\nservice_owner_uid = {uid}\nexpected_server_identity = \"018f0f9e-7b6e-4a31-8f4c-1d2e3f405162\"\n")).unwrap();
		std::fs::set_permissions(&config, std::fs::Permissions::from_mode(0o600)).unwrap();
		let profile = ClientProfile::load(&path, None).unwrap();
		let surface = cx.new(ChiefSurface::new);
		let editor = surface.update(cx, |s, cx| {
			s.bind_profile(Some(profile.clone()), cx);
			s.visual_workspace_fixture(cx);
			let work = s.selected.clone().unwrap();
			s.snapshot
				.as_mut()
				.unwrap()
				.work_items
				.iter_mut()
				.find(|w| w.id == work)
				.unwrap()
				.codex_thread_id = Some("thread".into());
			s.composer.update(cx, |input, cx| input.set_content("Unrelated unsent input", cx));
			s.prompt_edit = Panel {
				key: "review".into(),
				profile: Some(profile),
				work: work.clone(),
				thread: "thread".into(),
				..Default::default()
			};
			s.install_prompt_editors(
				DesktopPromptEditDraft {
					work_id: EntityId::new(work).unwrap(),
					thread_id: WireText::new("thread").unwrap(),
					before_turn_id: WireText::new("turn").unwrap(),
					item_id: WireText::new("item").unwrap(),
					original_hash: original_input().fingerprint().unwrap(),
					review_token: WireText::new("b".repeat(64)).unwrap(),
					receipt_id: None,
					handback_pending: false,
					confirmation_key: None,
					input: PromptDraft::new(vec![
						serde_json::json!({"type":"text","text":"Original"}),
						serde_json::json!({"type":"image","fileId":"native-image"}),
					])
					.unwrap(),
				},
				cx,
			)
			.unwrap();
			assert_eq!(s.composer.read(cx).content(), "Unrelated unsent input");
			s.prompt_edit.editors[0].1.clone()
		});
		editor.update(cx, |input, cx| {
			input.set_native_part(serde_json::json!({"type":"text","text":"Edited"}), cx).unwrap()
		});
		cx.run_until_parked();
		surface.update(cx, |s, cx| {
			let draft = s.prompt_edit.draft.as_ref().unwrap();
			assert_eq!(draft.input.parts()[0]["text"], "Edited");
			assert_eq!(draft.input.parts()[1]["fileId"], "native-image");
			assert!(s.submission.waiting.is_none());
			assert_eq!(s.composer.read(cx).content(), "Unrelated unsent input");
			let saved = s.prompt_edit.draft.clone().unwrap();
			let work = saved.work_id.as_str().to_owned();
			let review = saved.review_token.as_str().to_owned();
			s.reset_prompt_edit();
			s.reopen_prompt_editor(&work, &review, cx);
			assert_eq!(s.prompt_edit.draft.as_ref(), Some(&saved));
			assert_eq!(s.prompt_edit.editors[0].1.read(cx).content(), "Edited");
			assert!(s.submission.waiting.is_none());
			let retained_editor = s.prompt_edit.editors[0].1.clone();
			s.begin_prompt_removal(1, &saved, cx);
			let old_removal = s.prompt_edit.removal.as_ref().unwrap().key.clone();
			s.begin_prompt_removal(1, &saved, cx);
			let current_removal = s.prompt_edit.removal.as_ref().unwrap().key.clone();
			s.apply_prompt_removal(&old_removal, cx);
			assert_eq!(s.prompt_edit.draft.as_ref(), Some(&saved));
			s.apply_prompt_removal(&current_removal, cx);
			assert_eq!(s.prompt_edit.draft.as_ref().unwrap().input.parts().len(), 1);
			assert_eq!(s.prompt_edit.editors[0].1, retained_editor);
			assert_eq!(s.prompt_edit.editors[0].1.read(cx).content(), "Edited");
			assert!(s.submission.waiting.is_none());
			s.install_prompt_editors(saved.clone(), cx).unwrap();
			let mut stale = saved.clone();
			stale.input.replace_text(0, 0..0, "stale").unwrap();
			assert!(s.discard_prompt_editor(&stale, cx).is_err());
			assert_eq!(s.saved_prompt_editors(&work), vec![saved.clone()]);
			s.discard_prompt_editor(&saved, cx).unwrap();
			assert!(s.saved_prompt_editors(&work).is_empty());
			s.stage_prompt_editor(saved.clone(), cx).unwrap();
			let mut fresh = saved.clone();
			fresh.input = original_input();
			fresh.review_token = WireText::new("c".repeat(64)).unwrap();
			let renewed = s.renew_prompt_editor(&saved, &fresh, cx).unwrap();
			assert_eq!(renewed.input, saved.input);
			assert_eq!(s.saved_prompt_editors(&work), vec![renewed.clone()]);
			let mut later = renewed.clone();
			later.input.replace_text(0, 0..0, "Later edit ").unwrap();
			s.stage_prompt_editor(later.clone(), cx).unwrap();
			fresh.review_token = WireText::new("d".repeat(64)).unwrap();
			assert!(s.renew_prompt_editor(&renewed, &fresh, cx).is_err());
			assert_eq!(s.saved_prompt_editors(&work), vec![later.clone()]);
			let pending =
				later.begin_confirmation(IdempotencyKey::new("confirm-once").unwrap()).unwrap();
			s.stage_prompt_editor(pending.clone(), cx).unwrap();
			assert!(s.discard_prompt_editor(&pending, cx).is_err());
			assert!(s.renew_prompt_editor(&pending, &fresh, cx).is_err());
			assert_eq!(s.saved_prompt_editors(&work), vec![pending]);
			s.mark_stale(cx);
			assert!(s.prompt_edit.draft.is_none());
			assert!(!s.prompt_editor_source_current());
		});
		editor.update(cx, |input, cx| input.set_content("Old callback", cx));
		cx.run_until_parked();
		surface.update(cx, |s, _| assert!(s.prompt_edit.draft.is_none()));
	}
}
