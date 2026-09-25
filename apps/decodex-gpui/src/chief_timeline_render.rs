//! Render native history without converting provider IDs into local inbox IDs.
use super::{
	ChiefSurface, ChiefTimelineEntry, ChiefWorkItemDto, Content, Context, FluentBuilder,
	InteractiveElement, IntoElement, ParentElement, SharedString, Styled, div, key, markdown,
	muted,
};

impl ChiefSurface {
	pub(super) fn native_timeline_row(
		&self,
		work: &ChiefWorkItemDto,
		entry: &ChiefTimelineEntry,
		cx: &mut Context<Self>,
	) -> gpui::AnyElement {
		let identity = serde_json::json!([work.id, work.codex_thread_id, key(entry)]).to_string();
		let selector = format!("native-history-{identity}");
		let content = self.native_timeline_content(work, entry, &identity, cx);
		let content = self.anchored_native_history_entry(work, entry, content);
		let content = self.native_scroll_row(work, entry, content, cx);
		div()
			.debug_selector(move || selector)
			.id(SharedString::from(identity.clone()))
			.w_full()
			.min_w_0()
			.child(content)
			.into_any_element()
	}

	fn native_live_message(
		&self,
		work: &ChiefWorkItemDto,
		turn: &str,
		item: &str,
	) -> Option<&decodex_protocol::ChiefLiveMessageDto> {
		if self.native_history.entries.iter().any(|entry| {
			matches!(&entry.content,
			Content::TurnBoundary { turn_id, completed: true, .. } if turn_id == turn)
		}) {
			return None;
		}
		let (_, super::ChiefHistoryResult::Available { live, .. }) =
			self.history.as_ref().filter(|(id, _)| id == &work.id)?
		else {
			return None;
		};
		self.streamed_output(work)
			.unwrap_or(live.as_slice())
			.iter()
			.find(|message| message.turn_id == turn && message.item_id == item)
	}

	fn native_message_entry(
		&self,
		work: &ChiefWorkItemDto,
		turn_id: &str,
		text: &str,
		kind: &str,
	) -> decodex_protocol::ChiefHistoryEntryDto {
		let saved =
			self.history.as_ref().filter(|(id, _)| id == &work.id).and_then(|(_, history)| {
				match history {
					super::ChiefHistoryResult::Available { entries, .. } =>
						entries.iter().find(|entry| {
							!matches!(entry.kind.as_str(), "partial_answer" | "partial_plan")
								&& entry.turn_id.as_deref() == Some(turn_id)
								&& entry.text == text
						}),
					_ => None,
				}
			});
		let mut message =
			saved.cloned().unwrap_or_else(|| decodex_protocol::ChiefHistoryEntryDto {
				native_source: None,
				id: 0,
				kind: if kind == "userMessage" { "user" } else { "assistant" }.into(),
				text: text.into(),
				created_at_micros: 0,
				duration_ms: None,
				usage: None,
				activity: None,
				receipt: None,
				turn_id: Some(turn_id.into()),
				weather: Vec::new(),
			});
		message.text = text.into();
		message
	}

	fn native_timeline_content(
		&self,
		work: &ChiefWorkItemDto,
		entry: &ChiefTimelineEntry,
		identity: &str,
		cx: &mut Context<Self>,
	) -> gpui::AnyElement {
		let row = div().w_full().min_w_0().flex().flex_col().gap_1();
		match &entry.content {
			Content::Item { text, kind, truncated, activity, turn_id, item_id, attachments } => {
				let draft = if matches!(kind.as_str(), "agentMessage" | "plan") {
					self.native_live_message(work, turn_id, item_id)
				} else {
					None
				};
				let (text, truncated) = draft.map_or((text.as_str(), *truncated), |message| {
					(message.text.as_str(), message.truncated)
				});
				if matches!(kind.as_str(), "userMessage" | "agentMessage") {
					let message = self.native_message_entry(work, turn_id, text, kind);
					let mut body = div().debug_selector(|| "native-promotion-content".into());
					for attachment in attachments {
						body = body
							.child(self.native_attachment(work, turn_id, item_id, attachment, cx));
					}
					body = if draft.is_some() {
						body.child(super::super::text_reveal::StreamingText {
							text: text.into(),
							key: identity.into(),
						})
					} else {
						body.child(super::super::history_entry_with_key(&message, identity))
					};
					if truncated {
						body = body
							.child(muted("Some content was omitted from this history preview."));
					}
					return body.into_any_element();
				}
				let label = match kind.as_str() {
					"userMessage" => "You",
					"agentMessage" => "Assistant",
					"plan" => "Proposed plan",
					"functionCallOutput" => "Tool result",
					_ => kind,
				};
				let mut row = row.child(muted(label));
				for attachment in attachments {
					row = row.child(self.native_attachment(work, turn_id, item_id, attachment, cx));
				}
				if !text.is_empty() {
					let is_plan = kind == "plan";
					row = row.child(
						div()
							.debug_selector(move || {
								if is_plan {
									"native-plan-content"
								} else {
									"native-promotion-content"
								}
								.into()
							})
							.child(markdown::render(text, identity)),
					);
				}
				if truncated {
					row = row.child(muted("Some content was omitted from this history preview."));
				}
				if matches!(kind.as_str(), "agentMessage" | "plan") && !text.is_empty() {
					row = row.child(markdown::response_copy_button(
						&format!("copy-{identity}"),
						if kind == "plan" { "Copy plan" } else { "Copy response" },
						text.to_owned(),
					));
				}
				if let Some(activity) = activity {
					return self.detail_row(
						work,
						activity,
						row.child(format!("{} · {}", activity.label, activity.status)),
						cx,
					);
				}
				row.into_any_element()
			},
			Content::Speech { role, text, truncated, .. } => row
				.child(muted(if role == "user" { "You · Voice" } else { "Assistant · Voice" }))
				.child(text.clone())
				.when(*truncated, |r| r.child(muted("Voice transcript shortened.")))
				.into_any_element(),
			Content::VoiceBoundary { kind, outcome, .. } => row
				.child(muted(if kind == "realtimeSessionStarted" {
					"Voice conversation started"
				} else if outcome.as_deref() == Some("failed") {
					"Voice conversation failed"
				} else {
					"Voice conversation ended"
				}))
				.into_any_element(),
			Content::TurnBoundary {
				completed, status, duration_ms, error, usage_summary, ..
			} => {
				let label = if *completed {
					format!(
						"Turn {}{}",
						status.as_deref().unwrap_or("ended"),
						duration_ms.map(|ms| format!(" · {ms} ms")).unwrap_or_default()
					)
				} else {
					"Turn started".into()
				};
				row.child(muted(&label))
					.children(usage_summary.as_ref().map(|summary| {
						div().debug_selector(|| "native-turn-usage".into()).child(muted(summary))
					}))
					.children(error.as_ref().map(|error| {
						div().child(error.message.clone()).children(
							error.truncated.then(|| muted("Error details shortened or omitted.")),
						)
					}))
					.into_any_element()
			},
			content @ Content::Promotion { .. } =>
				self.native_promotion(work, content, identity, cx),
		}
	}

	fn native_promotion(
		&self,
		work: &ChiefWorkItemDto,
		content: &Content,
		identity: &str,
		cx: &mut Context<Self>,
	) -> gpui::AnyElement {
		let Content::Promotion { turn_id, agent_item_id, presentation, index, resolved, .. } =
			content
		else {
			unreachable!("promotion renderer")
		};
		let row = div().w_full().min_w_0().flex().flex_col().gap_1();
		let target = resolved
			.as_ref()
			.map(|item| {
				(
					item.text.as_str(),
					item.truncated,
					item.activity.as_ref(),
					item.attachments.as_slice(),
				)
			})
			.or_else(|| {
				let mut matches =
					self.native_history.entries.iter().filter_map(|entry| match &entry.content {
						Content::Item {
							turn_id: turn,
							item_id,
							text,
							truncated,
							activity,
							attachments,
							..
						} if turn == turn_id && item_id == agent_item_id => Some((
							text.as_str(),
							*truncated,
							activity.as_ref(),
							attachments.as_slice(),
						)),
						_ => None,
					});
				let first = matches.next()?;
				matches.next().is_none().then_some(first)
			});
		let mut row = row.child(muted("Shared during voice conversation"));
		match target {
			Some((text, truncated, activity, attachments)) => {
				for attachment in attachments {
					row = row.child(self.native_attachment(
						work,
						turn_id,
						agent_item_id,
						attachment,
						cx,
					));
				}
				if presentation == "inlineVisualization" {
					row = row.child(muted(&format!(
						"Visualization {} is in the referenced message.",
						u64::from(index.unwrap_or_default()) + 1
					)));
				} else if !text.is_empty() {
					row = row.child(
						div()
							.debug_selector(|| "native-promotion-content".into())
							.child(markdown::render(text, identity)),
					);
				}
				if truncated {
					row = row.child(muted("Shared content preview shortened."));
				}
				if let Some(activity) = activity {
					return self.detail_row(work, activity, row.child(activity.label.clone()), cx);
				}
			},
			_ =>
				row = row.child(
					div().debug_selector(|| "native-promotion-unavailable".into()).child(muted(
						"Referenced result is temporarily unavailable. History refresh will retry.",
					)),
				),
		}
		row.into_any_element()
	}
}

pub(super) fn attachment_caption(attachment: &decodex_protocol::ChiefTimelineAttachment) -> String {
	let kind = match attachment.kind.as_str() {
		"image" | "localImage" | "imageView" | "imageGeneration" | "inputImage" => "Image",
		"audio" | "localAudio" | "inputAudio" => "Audio",
		"skill" => "Skill",
		"mention" => "Mention",
		"resource" | "resource_link" => "Resource",
		_ => "Attachment",
	};
	if attachment.label == kind { kind.into() } else { format!("{kind} · {}", attachment.label) }
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::shell::chief_surface::native_timeline::{Binding, Timeline};
	use decodex_protocol::ChiefTimelinePage;
	use gpui::{px, size};

	fn plan_entry() -> ChiefTimelineEntry {
		ChiefTimelineEntry {
			position: 6,
			content: Content::Item {
				turn_id: "plan-turn".into(),
				item_id: "plan-item".into(),
				kind: "plan".into(),
				text: "## Proposed work\n1. Inspect source\n2. Verify changes".into(),
				truncated: false,
				activity: None,
				attachments: vec![],
			},
		}
	}

	fn rendered_entries() -> Vec<ChiefTimelineEntry> {
		vec![
			ChiefTimelineEntry {
				position: 0,
				content: Content::Item {
					turn_id: "turn".into(),
					item_id: "input".into(),
					kind: "userMessage".into(),
					text: "Describe this image.".into(),
					truncated: false,
					activity: None,
					attachments: vec![decodex_protocol::ChiefTimelineAttachment {
						index: 1,
						kind: "localImage".into(),
						label: "photo.png".into(),
						source: decodex_protocol::ChiefTimelineAttachmentSource::Local,
					}],
				},
			},
			ChiefTimelineEntry {
				position: 1,
				content: Content::Item {
					turn_id: "turn".into(),
					item_id: "message".into(),
					kind: "agentMessage".into(),
					text: "**Native result**\n\n```rust\nlet value = 1;\n```".into(),
					truncated: true,
					activity: None,
					attachments: vec![],
				},
			},
			ChiefTimelineEntry {
				position: 2,
				content: Content::Speech {
					item_id: "speech".into(),
					session_id: "voice".into(),
					role: "user".into(),
					text: "Explain the result.".into(),
					truncated: false,
				},
			},
			ChiefTimelineEntry {
				position: 3,
				content: Content::Promotion {
					item_id: "promotion".into(),
					session_id: "voice".into(),
					turn_id: "turn".into(),
					agent_item_id: "message".into(),
					presentation: "inlineVisualization".into(),
					resolved: None,
					index: Some(u32::MAX),
				},
			},
			ChiefTimelineEntry {
				position: 4,
				content: Content::Promotion {
					item_id: "off-page-promotion".into(),
					session_id: "voice".into(),
					turn_id: "older-turn".into(),
					agent_item_id: "off-page-message".into(),
					presentation: "inlineMarkdown".into(),
					index: None,
					resolved: Some(decodex_protocol::ChiefTimelinePromotedContent {
						text: "**Resolved older message**".into(),
						truncated: false,
						activity: None,
						attachments: vec![],
					}),
				},
			},
			ChiefTimelineEntry {
				position: 5,
				content: Content::TurnBoundary {
					turn_id: "turn".into(),
					completed: true,
					status: Some("failed".into()),
					duration_ms: Some(100),
					usage_summary: Some(
						"Turn tokens: input 120, output 30.\nThread total tokens: 900.\nObserved responses: 1. Showing 1 recorded amounts; units are provider-defined.\nResponse fixture: 0.12345678901234567890.".into(),
					),
					error: Some(decodex_protocol::ChiefTimelineError {
						message: "Model is overloaded. Try again.".into(),
						truncated: true,
					}),
				},
			},
			plan_entry(),
		]
	}

	#[gpui::test]
	fn native_messages_and_promotions_render_with_distinct_provider_identities(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		visual.simulate_resize(size(px(1400.), px(1400.)));
		let selectors = surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.graph_visible = false;
			let work = s
				.snapshot
				.as_mut()
				.unwrap()
				.work_items
				.iter_mut()
				.find(|work| Some(&work.id) == s.selected.as_ref())
				.unwrap();
			work.codex_thread_id = Some("native-thread".into());
			let entries = rendered_entries();
			let selectors = entries
				.iter()
				.map(|entry| {
					format!(
						"native-history-{}",
						serde_json::json!([work.id, work.codex_thread_id, super::key(entry)])
					)
				})
				.collect::<Vec<_>>();
			let mut history = Timeline::default();
			assert!(history.replace(
				Binding {
					work: work.id.clone(),
					thread: "native-thread".into(),
					account: "account".into()
				},
				ChiefTimelinePage {
					thread_id: "native-thread".into(),
					entries,
					next_cursor: None,
					active_realtime_session_at_page_start: None
				}
			));
			s.native_history = history;
			cx.notify();
			selectors
		});
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		let bounds = selectors
			.into_iter()
			.map(|selector| visual.debug_bounds(Box::leak(selector.into_boxed_str())).unwrap())
			.collect::<Vec<_>>();
		assert!(bounds.iter().all(|bounds| bounds.size.height > px(0.)));
		assert!(bounds.windows(2).all(|pair| pair[0].bottom() <= pair[1].top()));
		assert!(visual.debug_bounds("native-turn-usage").is_some());
		assert!(visual.debug_bounds("native-plan-content").is_some());
		assert!(visual.debug_bounds("native-promotion-content").is_some());
		assert!(visual.debug_bounds("saved-local-history").is_none());
		let toggle = visual.debug_bounds("native-history-source-toggle").unwrap();
		visual.simulate_click(toggle.center(), Default::default());
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		assert!(visual.debug_bounds("saved-local-history").is_some());
		surface.read_with(visual, |s, _| assert!(s.native_history.show_saved));
		let toggle = visual.debug_bounds("native-history-source-toggle").unwrap();
		visual.simulate_click(toggle.center(), Default::default());
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		assert!(visual.debug_bounds("saved-local-history").is_none());
		surface.update(visual, |s, cx| {
			let mut duplicate = s.native_history.entries[1].clone();
			duplicate.position = 7;
			s.native_history.entries.push(duplicate);
			cx.notify();
		});
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		assert!(visual.debug_bounds("native-promotion-unavailable").is_some());
	}
	#[gpui::test]
	fn unfinished_plan_stays_copyable_until_exact_native_history_is_complete(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		visual.simulate_resize(size(px(1400.), px(1400.)));
		let original = "## Draft\n\n$$\n\\frac{a}{b}";
		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.graph_visible = false;
			let work = s
				.snapshot
				.as_mut()
				.unwrap()
				.work_items
				.iter_mut()
				.find(|work| Some(&work.id) == s.selected.as_ref())
				.unwrap();
			work.codex_thread_id = Some("native-thread".into());
			let work = work.clone();
			let mut partial = s.native_message_entry(&work, "plan-turn", original, "agentMessage");
			partial.id = 991;
			partial.kind = "partial_plan".into();
			partial.native_source = Some(decodex_protocol::ChiefHistorySourceDto {
				thread_id: "native-thread".into(),
				turn_id: "plan-turn".into(),
				item_id: "plan-item".into(),
			});
			let (_, super::super::ChiefHistoryResult::Available { entries, .. }) =
				s.history.as_mut().unwrap()
			else {
				panic!("fixture history");
			};
			*entries = vec![partial];
			let mut history = Timeline::default();
			assert!(history.replace(
				Binding {
					work: work.id.clone(),
					thread: "native-thread".into(),
					account: "account".into()
				},
				ChiefTimelinePage {
					thread_id: "native-thread".into(),
					entries: vec![],
					next_cursor: None,
					active_realtime_session_at_page_start: None
				}
			));
			s.native_history = history;
			cx.notify();
		});
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		let copy = visual.debug_bounds("copy-partial-991").expect("retained plan is visible");
		visual.simulate_click(copy.center(), Default::default());
		visual.update(|_, cx| {
			assert_eq!(cx.read_from_clipboard().and_then(|item| item.text()), Some(original.into()))
		});
		for truncated in [true, false] {
			surface.update(visual, |s, cx| {
				let mut entry = plan_entry();
				if let Content::Item { truncated: value, .. } = &mut entry.content {
					*value = truncated;
				}
				s.native_history.entries = vec![entry];
				cx.notify();
			});
			visual.update(|window, cx| {
				window.draw(cx).clear();
			});
			assert_eq!(visual.debug_bounds("copy-partial-991").is_some(), truncated);
			assert!(visual.debug_bounds("native-plan-content").is_some());
		}
	}
}
