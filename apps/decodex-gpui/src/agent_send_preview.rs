//! Ephemeral sending bubbles. Durable records remain owned by the service.
use super::*;

pub(super) struct Preview {
	recorded: std::cell::Cell<bool>,
	key: String,
	owner: String,
	text: String,
	local_after: i64,
	native_after: Option<u64>,
	local_ordinal: usize,
	native_ordinal: usize,
}

impl AgentSurface {
	pub(super) fn capture_send_preview(&mut self, pending: &PendingCommand) {
		let (Some(key), Some(owner), Some(text)) = (&pending.key, &pending.owner, &pending.draft)
		else {
			return;
		};

		if text.is_empty() {
			return;
		}

		let retained = std::mem::take(&mut self.submission.previews);

		self.submission.previews =
			retained.into_iter().filter(|p| !self.preview_recorded(p)).collect();

		let local_after = self
			.history
			.as_ref()
			.filter(|(id, _)| id == owner)
			.and_then(|(_, h)| match h {
				AgentHistoryResult::Available { entries, .. } => entries.iter().map(|e| e.id).max(),
				_ => None,
			})
			.unwrap_or(0);
		let native_after = self
			.native_history
			.binding
			.as_ref()
			.filter(|b| &b.work == owner)
			.and_then(|_| self.native_history.entries.last().map(|e| e.position));
		let matching = |p: &&Preview| p.owner == *owner && p.text == *text;
		let local_ordinal = 1 + self
			.submission
			.previews
			.iter()
			.filter(matching)
			.filter(|p| p.local_after == local_after)
			.count();
		let native_ordinal = 1 + self
			.submission
			.previews
			.iter()
			.filter(matching)
			.filter(|p| p.native_after == native_after)
			.count();

		self.submission.previews.push(Preview {
			recorded: Default::default(),
			key: key.as_str().into(),
			owner: owner.clone(),
			text: text.clone(),
			local_after,
			native_after,
			local_ordinal,
			native_ordinal,
		});
	}

	fn preview_recorded(&self, preview: &Preview) -> bool {
		if preview.recorded.get() {
			return true;
		}

		let recorded = self.preview_in_history(preview);

		preview.recorded.set(recorded);

		recorded
	}

	fn preview_in_history(&self, preview: &Preview) -> bool {
		let native = self
			.snapshot
			.as_ref()
			.and_then(|s| s.work_items.iter().find(|w| w.id == preview.owner))
			.is_some_and(|w| self.native_history_active(w));

		if native && !self.native_history.summary_only() {
			return self.native_history.entries.iter().filter(|e| preview.native_after.is_none_or(|position| e.position > position) && matches!(&e.content,
                decodex_protocol::AgentTimelineContent::Item { kind, text, .. } if kind == "userMessage" && text == &preview.text)).count() >= preview.native_ordinal;
		}

		let history = self
			.history
			.as_ref()
			.filter(|(id, _)| id == &preview.owner)
			.map(|(_, h)| h)
			.or_else(|| self.history_cache.get(&preview.owner));

		matches!(history, Some(AgentHistoryResult::Available { entries, .. }) if entries.iter().filter(|e| e.id > preview.local_after && matches!(e.kind.as_str(), "user" | "instruction") && e.text == preview.text).count() >= preview.local_ordinal)
	}

	pub(super) fn finish_send_preview(&mut self, pending: &PendingCommand, accepted: bool) {
		if !accepted {
			self.submission
				.previews
				.retain(|p| pending.key.as_ref().is_none_or(|key| key.as_str() != p.key));
		}
	}

	pub(super) fn preview_covers_receipt(&self, work: &str, id: i64, text: &str) -> bool {
		self.submission
			.previews
			.iter()
			.any(|p| p.owner == work && p.text == text && id > p.local_after)
	}

	pub(super) fn send_previews(&self, work: &str) -> Vec<gpui::AnyElement> {
		self.submission
			.previews
			.iter()
			.filter(|p| p.owner == work && !self.preview_recorded(p))
			.map(|p| {
				let entry = decodex_protocol::AgentHistoryEntryDto {
					id: 0,
					kind: "user".into(),
					text: p.text.clone(),
					created_at_micros: 0,
					native_source: None,
					duration_ms: None,
					usage: None,
					activity: None,
					receipt: None,
					turn_id: None,
					weather: vec![],
				};
				let native = self
					.snapshot
					.as_ref()
					.and_then(|s| s.work_items.iter().find(|w| w.id == work))
					.is_some_and(|w| self.native_history_active(w));

				div()
					.debug_selector(|| "sending-message-preview".into())
					.child(history_entry_with_key(&entry, &format!("sending-{}", p.key)))
					.when(native, |row| row.child(div().h(px(ui_theme::USER_MESSAGE_ACTION_SIZE))))
					.into_any_element()
			})
			.collect()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	#[gpui::test]
	fn native_echo_replaces_preview_without_changing_bubble_bounds(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(px(1_400.), px(1_000.)));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.snapshot
				.as_mut()
				.unwrap()
				.work_items
				.iter_mut()
				.find(|w| w.id == "agent")
				.unwrap()
				.codex_thread_id = Some("thread".into());
			s.native_history.binding = Some(native_timeline::Binding {
				work: "agent".into(),
				thread: "thread".into(),
				account: "account".into(),
			});

			s.capture_send_preview(&PendingCommand {
				recovery: None,
				key: Some(IdempotencyKey::new("native-preview").unwrap()),
				steer: None,
				execution_intent: None,
				epoch: 0,
				attachments: None,
				references: None,
				owner: Some("agent".into()),
				draft: Some("hello".into()),
			});
			cx.notify();
		});

		visual.update(|w, cx| w.draw(cx).clear());

		std::thread::sleep(std::time::Duration::from_millis(240));

		visual.update(|w, cx| w.draw(cx).clear());

		let preview = visual.debug_bounds("sending-message-preview").unwrap();

		surface.update(visual, |s, cx| {
			s.native_history.entries.push(decodex_protocol::AgentTimelineEntry {
				position: 0,
				content: decodex_protocol::AgentTimelineContent::Item {
					turn_id: "turn".into(),
					item_id: "input".into(),
					kind: "userMessage".into(),
					text: "hello".into(),
					phase: None,
					truncated: false,
					app_ui: false,
					activity: None,
					attachments: vec![],
				},
			});
			cx.notify();
		});
		visual.update(|w, cx| w.draw(cx).clear());

		assert!(visual.debug_bounds("sending-message-preview").is_none());
		assert_eq!(visual.debug_bounds("native-promotion-content").unwrap(), preview);
	}

	#[gpui::test]
	fn acceptance_does_not_override_reading_position(cx: &mut gpui::TestAppContext) {
		let surface = cx.new(AgentSurface::new);

		surface.update(cx, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.latest_follow_work = None;

			s.history_follow_paused.insert("agent".into());
			s.apply_command_result(
				Ok(AgentCommandResponse::Accepted { work_id: EntityId::new("agent").unwrap() }),
				Some("sent text"),
				cx,
			);

			assert!(s.latest_follow_work.is_none());
			assert!(s.history_follow_paused.contains("agent"));
		});
	}

	#[gpui::test]
	fn preview_is_immediate_and_only_new_records_replace_repeated_inputs(
		cx: &mut gpui::TestAppContext,
	) {
		let surface = cx.new(AgentSurface::new);

		surface.update(cx, |s, cx| {
			s.visual_workspace_fixture(cx);

			let (_, AgentHistoryResult::Available { entries, .. }) = s.history.as_mut().unwrap()
			else {
				panic!("fixture")
			};
			let mut record = entries[0].clone();

			record.kind = "user".into();
			record.text = "hello".into();

			entries.push(record.clone());

			record.id = entries.iter().map(|e| e.id).max().unwrap() + 1;

			let pending = |key: &str| PendingCommand {
				recovery: None,
				key: Some(IdempotencyKey::new(key).unwrap()),
				steer: None,
				execution_intent: None,
				epoch: 0,
				attachments: None,
				references: None,
				owner: Some("agent".into()),
				draft: Some("hello".into()),
			};
			let first = pending("preview-first");

			s.capture_send_preview(&first);

			assert_eq!(s.send_previews("agent").len(), 1, "visible before any server response");
			assert!(s.send_previews("release").is_empty());

			s.finish_send_preview(&first, true);

			let second = pending("preview-second");

			s.capture_send_preview(&second);

			assert_eq!(
				s.send_previews("agent").len(),
				2,
				"old equal text is not an acknowledgement"
			);

			let (_, AgentHistoryResult::Available { entries, .. }) = s.history.as_mut().unwrap()
			else {
				unreachable!()
			};

			entries.push(record);

			assert_eq!(s.send_previews("agent").len(), 1, "one record replaces only one preview");

			s.finish_send_preview(&second, false);

			assert!(
				s.send_previews("agent").is_empty(),
				"rejection removes preview, draft recovery owns the text"
			);

			s.snapshot
				.as_mut()
				.unwrap()
				.work_items
				.iter_mut()
				.find(|w| w.id == "agent")
				.unwrap()
				.codex_thread_id = Some("thread".into());
			s.native_history.binding = Some(native_timeline::Binding {
				work: "agent".into(),
				thread: "thread".into(),
				account: "account".into(),
			});

			assert!(
				s.send_previews("agent").is_empty(),
				"source changes must not resurrect confirmed previews"
			);

			s.capture_send_preview(&pending("first-native"));

			assert_eq!(s.send_previews("agent").len(), 1);

			s.native_history.entries.push(decodex_protocol::AgentTimelineEntry {
				position: 0,
				content: decodex_protocol::AgentTimelineContent::Item {
					turn_id: "new-turn".into(),
					item_id: "input".into(),
					kind: "userMessage".into(),
					text: "hello".into(),
					phase: None,
					truncated: false,
					app_ui: false,
					activity: None,
					attachments: vec![],
				},
			});

			assert!(s.send_previews("agent").is_empty(), "the first native position can be zero");
		});
	}
}
