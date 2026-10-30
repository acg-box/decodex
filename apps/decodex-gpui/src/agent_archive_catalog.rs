//! On-demand archive browser. Native identities and retained relations stay authoritative.
use gpui::{AnyElement, AppContext as _, Entity, IntoElement as _, KeyDownEvent, Task, prelude::*};
use tokio::runtime::Builder;

use crate::{
	composer_input::ComposerInput,
	shell::agent_surface::{AgentSurface, Context},
	ui_feedback::{self, Feedback},
	ui_loading, ui_theme,
};
use decodex_protocol::{
	AgentActionDto, AgentArchiveResult, AgentClient, AgentSnapshotDto, AgentSnapshotResult,
	EntityId, IdempotencyKey, WireText,
};

#[derive(Default)]
pub(in crate::shell::agent_surface) struct Catalog {
	pub open: bool,
	data: Option<AgentSnapshotDto>,
	search: Option<Entity<ComposerInput>>,
	read: Option<Task<()>>,
	mutation: Option<Task<()>>,
	busy: Option<String>,
	feedback: Option<String>,
}

impl AgentSurface {
	pub(in crate::shell::agent_surface) fn open_archive_catalog(&mut self, cx: &mut Context<Self>) {
		self.cancel_dictation(cx);
		self.workspace.browsing = true;
		self.workspace.archives.open = true;
		if self.workspace.archives.search.is_none() {
			let search = cx.new(|cx| {
				ComposerInput::with_placeholder(48, "Search archived agents…", "Search archive", cx)
			});
			cx.observe(&search, |_, _, cx| cx.notify()).detach();
			self.workspace.archives.search = Some(search);
		}
		self.load_archive_catalog(cx);
		cx.notify();
	}

	fn load_archive_catalog(&mut self, cx: &mut Context<Self>) {
		if self.workspace.archives.read.is_some() || self.workspace.archives.mutation.is_some() {
			return;
		}
		let Some(profile) = self.profile.clone() else {
			self.workspace.archives.feedback = Some("Connect to Codex to read the archive.".into());
			return;
		};
		self.workspace.archives.feedback = None;
		let request = cx.background_executor().spawn(async move {
			let runtime = Builder::new_current_thread().enable_all().build().ok()?;
			runtime.block_on(AgentClient::new(profile).archive_catalog()).ok()
		});
		self.workspace.archives.read = Some(cx.spawn(async move |surface, cx| {
			let result = request.await;
			let _ = surface.update(cx, |s, cx| {
				s.workspace.archives.read = None;
				match result {
					Some(AgentSnapshotResult::Available(data)) if !data.connection_initializing =>
						s.workspace.archives.data = Some(data),
					_ =>
						s.workspace.archives.feedback =
							Some("Could not read the archive. Try again.".into()),
				}
				cx.notify();
			});
		}));
	}

	fn unarchive_catalog_item(&mut self, id: &str, cx: &mut Context<Self>) {
		if self.workspace.archives.mutation.is_some() {
			return;
		}
		let Some(data) = &self.workspace.archives.data else { return };
		let Some(item) = data
			.work_items
			.iter()
			.find(|item| item.id == id && data.archived_work_ids.contains(id))
		else {
			return;
		};
		let (Some(profile), Some(thread), Ok(work_id)) =
			(self.profile.clone(), item.codex_thread_id.clone(), EntityId::new(id.to_owned()))
		else {
			return;
		};
		let Ok(thread_id) = WireText::new(thread.clone()) else { return };
		let id = id.to_owned();
		self.workspace.archives.busy = Some(id.clone());
		self.workspace.archives.feedback = None;
		// Supersede an older catalog read so it cannot put a restored row back.
		self.workspace.archives.read = None;
		let request = cx.background_executor().spawn(async move {
			let runtime = Builder::new_current_thread().enable_all().build().ok()?;
			let client = AgentClient::new(profile);
			let key =
				IdempotencyKey::new(super::super::unique_command()).expect("bounded identity");
			let _ = runtime.block_on(client.execute(
				AgentActionDto::RestoreArchivedThread { work_id: work_id.clone(), thread_id },
				key,
			));
			let state = runtime.block_on(client.archive_state(work_id)).ok();
			if matches!(state, Some(AgentArchiveResult::Active { thread_id }) if thread_id == thread)
			{
				Some(runtime.block_on(client.query()).ok())
			} else {
				None
			}
		});
		self.workspace.archives.mutation = Some(cx.spawn(async move |surface, cx| {
			let result = request.await;
			let _ = surface.update(cx, |s, cx| {
				s.workspace.archives.mutation = None;
				s.workspace.archives.busy = None;
				if let Some(snapshot) = result {
					if let Some(data) = &mut s.workspace.archives.data {
						data.archived_work_ids.remove(&id);
					}
					if let Some(snapshot) = snapshot {
						s.apply_refreshed_snapshot(Ok(snapshot), cx);
					}
				} else {
					s.workspace.archives.feedback =
						Some("Could not confirm unarchive. Refresh before trying again.".into());
				}
				cx.notify();
			});
		}));
		cx.notify();
	}

	pub(in crate::shell::agent_surface) fn render_archive_catalog(
		&self,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let catalog = &self.workspace.archives;
		let query = catalog
			.search
			.as_ref()
			.map(|s| s.read(cx).content().trim().to_lowercase())
			.unwrap_or_default();
		let mut items: Vec<_> = catalog
			.data
			.iter()
			.flat_map(|data| {
				data.work_items.iter().filter(|item| data.archived_work_ids.contains(&item.id))
			})
			.filter(|item| item.title.to_lowercase().contains(&query))
			.collect();
		items.sort_by_key(|item| std::cmp::Reverse(item.updated_at_micros));
		let count = items.len();
		let mut list = gpui::div()
			.id("archive-list")
			.debug_selector(|| "archive-list".into())
			.flex_1()
			.min_h_0()
			.overflow_y_scroll()
			.flex()
			.flex_col()
			.gap(gpui::px(2.));
		for item in items {
			let id = item.id.clone();
			let key_id = id.clone();
			let project = catalog
				.data
				.as_ref()
				.and_then(|data| data.projects.iter().find(|p| p.agent_ids.contains(&id)))
				.map(|p| p.name.clone());
			let busy = catalog.busy.as_ref() == Some(&id);
			list = list.child(
				gpui::div()
					.id(gpui::SharedString::from(format!("archived-{id}")))
					.flex_none()
					.min_w_0()
					.min_h(gpui::px(48.))
					.px(gpui::px(8.))
					.py(gpui::px(6.))
					.flex()
					.items_center()
					.gap(gpui::px(16.))
					.rounded(gpui::px(7.))
					.hover(|d| d.bg(ui_theme::rgba(ui_theme::HOVER_FILL)))
					.child(
						gpui::div()
							.min_w_0()
							.flex_1()
							.flex()
							.flex_col()
							.gap(gpui::px(2.))
							.child(
								gpui::div()
									.text_size(gpui::px(ui_theme::BODY_SIZE))
									.text_color(ui_theme::rgb(ui_theme::TEXT))
									.truncate()
									.child(item.title.clone()),
							)
							.children(project.map(|project| {
								gpui::div()
									.text_size(gpui::px(11.))
									.text_color(ui_theme::rgb(ui_theme::TEXT_MUTED))
									.child(project)
							})),
					)
					.child(
						ui_theme::settings_button(
							format!("unarchive-{id}"),
							if busy { "Unarchiving…" } else { "Unarchive" },
						)
						.when(catalog.mutation.is_some(), |d| d.opacity(0.45))
						.on_click(cx.listener(move |s, _, _, cx| s.unarchive_catalog_item(&id, cx)))
						.on_key_down(cx.listener(
							move |s, e: &KeyDownEvent, _, cx| {
								if !e.is_held
									&& ["enter", "space"].contains(&e.keystroke.key.as_str())
								{
									s.unarchive_catalog_item(&key_id, cx);
									cx.stop_propagation();
								}
							},
						)),
					),
			);
		}
		if count == 0 && catalog.read.is_none() && catalog.feedback.is_none() {
			list = list.child(
				gpui::div().p_4().text_color(ui_theme::rgb(ui_theme::TEXT_MUTED)).child(
					if query.is_empty() {
						"No archived agents."
					} else {
						"No matching archived agents."
					},
				),
			);
		}
		gpui::div()
			.id("archive-browser-content")
			.debug_selector(|| "archive-browser-content".into())
			.flex_1()
			.min_w_0()
			.min_h_0()
			.flex()
			.flex_col()
			.p_4()
			.gap_3()
			.child(
				gpui::div()
					.flex()
					.items_center()
					.gap_3()
					.child(
						gpui::div()
							.flex_1()
							.min_w_0()
							.h(gpui::px(ui_theme::CONTROL_SIZE))
							.children(catalog.search.clone()),
					)
					.child(ui_theme::settings_button("refresh-archive", "Refresh").on_click(
						cx.listener(|s, _, _, cx| {
							s.load_archive_catalog(cx);
							cx.notify();
						}),
					)),
			)
			.children(catalog.feedback.as_ref().and_then(|message| {
				ui_feedback::inline_feedback("archive-feedback", "", &Feedback::error(message))
			}))
			.children(catalog.read.is_some().then(|| ui_loading::loading("Reading archive…")))
			.child(list)
			.into_any_element()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::shell::agent_surface::wire_test_support::{self, SERVER};
	use decodex_protocol::{
		CURRENT_VERSION, ClientMessage, CommandPayload, QueryPayload, QueryResultEnvelope,
		QueryResultPayload, ServerId, ServerMessage,
	};
	use futures_util::{SinkExt as _, StreamExt as _};
	use std::{
		thread,
		time::{Duration, Instant},
	};
	use tokio_tungstenite::tungstenite::Message;

	struct ArchiveView(Entity<AgentSurface>);
	impl gpui::Render for ArchiveView {
		fn render(&mut self, _: &mut gpui::Window, cx: &mut Context<Self>) -> impl IntoElement {
			self.0.update(cx, |s, cx| s.render_archive_catalog(cx))
		}
	}

	#[gpui::test]
	fn archive_browser_restores_the_exact_thread_after_a_lost_reply_and_keeps_main(
		cx: &mut gpui::TestAppContext,
	) {
		cx.background_executor.allow_parking();
		let (view, visual) = cx.add_window_view(|_, cx| {
			let surface = cx.new(AgentSurface::new);
			cx.observe(&surface, |_, _, cx| cx.notify()).detach();
			ArchiveView(surface)
		});
		let surface = view.read_with(visual, |view, _| view.0.clone());
		let (catalog, active, selection) = surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.composer.update(cx, |input, cx| input.set_content("Keep the current Main draft", cx));
			let mut catalog = s.snapshot.clone().unwrap();
			let mut archived = catalog.work_items[0].clone();
			archived.id = "archived-test".into();
			archived.title = "Archived design review".into();
			archived.parent_goal_id = Some(catalog.work_items[0].id.clone());
			archived.codex_thread_id = Some("native-archived-test".into());
			catalog.work_items.push(archived);
			catalog.archived_work_ids.insert("archived-test".into());
			let mut active = catalog.clone();
			active.archived_work_ids.clear();
			(catalog, active, s.selected.clone())
		});
		let expected = active.clone();
		let (_root, profile, server) = wire_test_support::fixture(move |listener| async move {
			for index in 0..4 {
				let mut socket = wire_test_support::accept(&listener).await;
				let Message::Text(text) = socket.next().await.unwrap().unwrap() else {
					panic!("request")
				};
				let request: ClientMessage = serde_json::from_str(&text).unwrap();
				if index == 1 {
					let ClientMessage::Command(command) = request else {
						panic!("unarchive command")
					};
					let CommandPayload::Agent { action } = command.payload else {
						panic!("agent action")
					};
					assert!(
						matches!(*action, AgentActionDto::RestoreArchivedThread { work_id, thread_id } if work_id.as_str() == "archived-test" && thread_id.as_str() == "native-archived-test")
					);
					// The exact readback, rather than a second send, resolves this lost response.
					socket.close(None).await.unwrap();
					continue;
				}
				let ClientMessage::Query(query) = request else { panic!("query") };
				let payload = match (&query.payload, index) {
					(QueryPayload::GetAgentArchiveCatalog, 0) => QueryResultPayload::AgentSnapshot(
						AgentSnapshotResult::Available(catalog.clone()),
					),
					(QueryPayload::GetAgentArchiveState { work_id }, 2)
						if work_id.as_str() == "archived-test" =>
						QueryResultPayload::AgentArchiveState(AgentArchiveResult::Active {
							thread_id: "native-archived-test".into(),
						}),
					(QueryPayload::GetAgentSnapshot, 3) => QueryResultPayload::AgentSnapshot(
						AgentSnapshotResult::Available(active.clone()),
					),
					_ => panic!("unexpected archive request: {:?}", query.payload),
				};
				socket
					.send(Message::Text(
						serde_json::to_string(&ServerMessage::QueryResult(QueryResultEnvelope {
							version: CURRENT_VERSION,
							server_id: ServerId::new(SERVER).unwrap(),
							query_id: query.query_id,
							payload,
						}))
						.unwrap()
						.into(),
					))
					.await
					.unwrap();
			}
		});
		surface.update(visual, |s, cx| {
			s.profile = Some(profile);
			s.open_archive_catalog(cx);
		});
		let wait = |visual: &mut gpui::VisualTestContext, restoring: bool| {
			let deadline = Instant::now() + Duration::from_secs(5);
			loop {
				visual.run_until_parked();
				let pending = surface.read_with(visual, |s, _| {
					if restoring {
						s.workspace.archives.mutation.is_some()
					} else {
						s.workspace.archives.read.is_some()
					}
				});
				if !pending {
					break;
				}
				assert!(Instant::now() < deadline, "archive request timed out");
				thread::sleep(Duration::from_millis(5));
			}
		};
		wait(visual, false);
		visual.update(crate::ui_frame::draw_frame);
		let restore = visual.debug_bounds("unarchive-archived-test").unwrap();
		visual.simulate_click(restore.center(), Default::default());
		wait(visual, true);
		server.join().unwrap();
		visual.update(crate::ui_frame::draw_frame);
		assert!(visual.debug_bounds("unarchive-archived-test").is_none());
		surface.read_with(visual, |s, cx| {
			assert_eq!(s.selected, selection);
			assert_eq!(s.composer.read(cx).content(), "Keep the current Main draft");
			assert_eq!(s.snapshot.as_ref(), Some(&expected));
			assert!(s.workspace.archives.feedback.is_none());
		});
	}
}
