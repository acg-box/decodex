//! On-demand archive browser. Native identities and retained relations stay authoritative.
use gpui::{
	AnyElement, AppContext as _, Entity, IntoElement as _, KeyDownEvent, Role, Task, prelude::*,
};
use std::{
	rc::Rc,
	time::{Duration, Instant},
};
use tokio::runtime::Builder;

use crate::{
	composer_input::ComposerInput,
	shell::{
		ControlTooltip,
		agent_surface::{AgentSurface, Context},
		workspace_symbols::{self, Symbol},
	},
	ui_feedback::{self, Feedback},
	ui_loading,
	ui_motion::{Choice, Segmented, SmoothControl as _},
	ui_scroll::SmoothScrollArea as _,
	ui_theme,
};
use decodex_protocol::{
	AgentActionDto, AgentArchiveResult, AgentClient, AgentSnapshotDto, AgentSnapshotResult,
	AgentWorkItemDto, EntityId, IdempotencyKey, WireText,
};

#[derive(Default)]
pub(in crate::shell::agent_surface) struct Catalog {
	pub open: bool,
	pub viewed: Option<String>,
	data: Option<AgentSnapshotDto>,
	search: Option<Entity<ComposerInput>>,
	read: Option<Task<()>>,
	mutation: Option<Task<()>>,
	busy: Option<String>,
	feedback: Option<String>,
	sort: Sort,
	rows: Rc<[Row]>,
	prepared: Option<(String, Sort, u64)>,
	revision: u64,
	confirmation: Option<(String, Instant)>,
	confirmation_task: Option<Task<()>>,
	deleting: bool,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Sort {
	#[default]
	Recent,
	Oldest,
	Name,
}

#[derive(Clone)]
struct Row {
	id: String,
	title: String,
	metadata: String,
	updated: i64,
}

impl Catalog {
	fn prepare_rows(&mut self, query: &str) {
		let key = (query.to_owned(), self.sort, self.revision);
		if self.prepared.as_ref() == Some(&key) {
			return;
		}
		let mut rows: Vec<_> = self
			.data
			.iter()
			.flat_map(|data| {
				data.work_items.iter().filter(|item| data.archived_work_ids.contains(&item.id))
			})
			.filter_map(|item| {
				let project = self.project_name(&item.id).map(str::to_owned);
				let text = format!("{} {}", item.title, project.as_deref().unwrap_or_default())
					.to_lowercase();
				query.split_whitespace().all(|word| text.contains(word)).then(|| Row {
					id: item.id.clone(),
					title: item.title.clone(),
					metadata: archive_metadata(project.as_deref(), item.updated_at_micros),
					updated: item.updated_at_micros,
				})
			})
			.collect();
		rows.sort_by(|a, b| match self.sort {
			Sort::Recent => b.updated.cmp(&a.updated).then(a.title.cmp(&b.title)),
			Sort::Oldest => a.updated.cmp(&b.updated).then(a.title.cmp(&b.title)),
			Sort::Name => a.title.to_lowercase().cmp(&b.title.to_lowercase()).then(a.id.cmp(&b.id)),
		});
		self.rows = rows.into();
		self.prepared = Some(key);
	}

	fn confirming(&self, id: &str) -> bool {
		self.confirmation
			.as_ref()
			.is_some_and(|(owner, at)| owner == id && at.elapsed() < Duration::from_secs(5))
	}

	pub(in crate::shell::agent_surface) fn archived_work(
		&self,
		id: &str,
	) -> Option<&AgentWorkItemDto> {
		let data = self.data.as_ref()?;
		data.work_items.iter().find(|work| work.id == id && data.archived_work_ids.contains(id))
	}

	pub(in crate::shell::agent_surface) fn viewed_work(&self) -> Option<&AgentWorkItemDto> {
		self.archived_work(self.viewed.as_deref()?)
	}

	fn project_name(&self, id: &str) -> Option<&str> {
		self.data
			.as_ref()?
			.projects
			.iter()
			.find(|project| project.agent_ids.iter().any(|agent| agent == id))
			.map(|project| project.name.as_str())
	}

}

impl AgentSurface {
	pub(in crate::shell::agent_surface) fn open_archived_conversation(
		&mut self,
		id: &str,
		cx: &mut Context<Self>,
	) {
		if self.workspace.archives.archived_work(id).is_none() {
			return;
		}
		self.workspace.archives.viewed = Some(id.into());
		self.workspace.archives.confirmation = None;
		self.workspace.archives.confirmation_task = None;
		self.workspace.archives.open = false;
		self.open_work_conversation(id, None, cx);
	}

	pub(in crate::shell::agent_surface) fn open_archive_catalog(&mut self, cx: &mut Context<Self>) {
		self.cancel_dictation(cx);
		self.workspace.browsing = true;
		self.workspace.archives.open = true;
		if self.workspace.archives.search.is_none() {
			let search = cx.new(|cx| {
				ComposerInput::with_placeholder(
					48,
					"Search by agent or project…",
					"Search archive",
					cx,
				)
			});
			cx.observe(&search, |s, _, cx| {
				s.workspace.archives.confirmation = None;
				s.workspace.archives.confirmation_task = None;
				cx.notify();
			})
			.detach();
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
					Some(AgentSnapshotResult::Available(data)) if !data.connection_initializing => {
						s.retire_archived_messages(&data, cx);
						s.workspace.archives.data = Some(data);
						s.workspace.archives.revision += 1;
					},
					_ =>
						s.workspace.archives.feedback =
							Some("Could not load the archive. Reopen Archive to retry.".into()),
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
		self.workspace.archives.deleting = false;
		self.workspace.archives.confirmation = None;
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
					s.workspace.archives.revision += 1;
					if let Some(snapshot) = snapshot {
						s.apply_refreshed_snapshot(Ok(snapshot), cx);
					}
				} else {
					s.workspace.archives.feedback =
						Some("Unarchive not confirmed. Reopen Archive to check.".into());
				}
				cx.notify();
			});
		}));
		cx.notify();
	}

	fn delete_archive_item(&mut self, id: &str, cx: &mut Context<Self>) {
		if self.workspace.archives.mutation.is_some()
			|| self.workspace.archives.archived_work(id).is_none()
		{
			return;
		}
		if !self.workspace.archives.confirming(id) {
			let now = Instant::now();
			self.workspace.archives.confirmation = Some((id.into(), now));
			let id = id.to_owned();
			self.workspace.archives.confirmation_task = Some(cx.spawn(async move |surface, cx| {
				cx.background_executor().timer(Duration::from_secs(5)).await;
				let _ = surface.update(cx, |s, cx| {
					if s.workspace.archives.confirmation.as_ref() == Some(&(id, now)) {
						s.workspace.archives.confirmation = None;
						cx.notify();
					}
				});
			}));
			cx.notify();
			return;
		}
		let (Some(profile), Some(thread), Ok(work_id)) = (
			self.profile.clone(),
			self.workspace.archives.archived_work(id).and_then(|w| w.codex_thread_id.clone()),
			EntityId::new(id.to_owned()),
		) else {
			return;
		};
		let Ok(thread_id) = WireText::new(thread) else {
			return;
		};
		let id = id.to_owned();
		self.workspace.archives.confirmation = None;
		self.workspace.archives.confirmation_task = None;
		self.workspace.archives.busy = Some(id.clone());
		self.workspace.archives.deleting = true;
		self.workspace.archives.read = None;
		self.workspace.archives.feedback = None;
		let request = cx.background_executor().spawn(async move {
			let runtime = Builder::new_current_thread().enable_all().build().ok()?;
			let client = AgentClient::new(profile);
			let key = IdempotencyKey::new(super::super::unique_command()).ok()?;
			let response = runtime
				.block_on(
					client
						.execute(AgentActionDto::DeleteArchivedThread { work_id, thread_id }, key),
				)
				.ok()?;
			if !matches!(response, decodex_protocol::AgentCommandResponse::Accepted { .. }) {
				return None;
			}
			Some((
				runtime.block_on(client.archive_catalog()).ok(),
				runtime.block_on(client.query()).ok(),
			))
		});
		self.workspace.archives.mutation = Some(cx.spawn(async move |surface, cx| {
			let result = request.await;
			let _ = surface.update(cx, |s, cx| {
				s.workspace.archives.mutation = None;
				s.workspace.archives.busy = None;
				if let Some((catalog, active)) = result {
					if let Some(AgentSnapshotResult::Available(data)) = catalog {
						s.workspace.archives.data = Some(data);
					} else if let Some(data) = &mut s.workspace.archives.data {
						data.work_items.retain(|w| w.id != id);
						data.archived_work_ids.remove(&id);
					}
					s.workspace.archives.revision += 1;
					if s.workspace.archives.viewed.as_ref() == Some(&id) {
						s.workspace.archives.viewed = None;
					}
					if let Some(active) = active {
						s.apply_refreshed_snapshot(Ok(active), cx);
					}
				} else {
					s.workspace.archives.feedback =
						Some("Deletion not confirmed. Reopen Archive to check.".into());
				}
				cx.notify();
			});
		}));
		cx.notify();
	}

	pub(in crate::shell::agent_surface) fn render_archive_catalog(
		&mut self,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let query = self
			.workspace
			.archives
			.search
			.as_ref()
			.map(|s| s.read(cx).content().trim().to_lowercase())
			.unwrap_or_default();
		self.workspace.archives.prepare_rows(&query);
		let catalog = &self.workspace.archives;
		let rows = catalog.rows.clone();
		let mut list = gpui::div()
			.id("archive-list")
			.debug_selector(|| "archive-list".into())
			.size_full()
			.min_h_0()
			.overflow_y_scroll()
			.flex()
			.flex_col()
			.pr(gpui::px(10.));
		for item in rows.iter() {
			let id = item.id.clone();
			let open_id = id.clone();
			let open_key = id.clone();
			let restore_id = id.clone();
			let restore_key = id.clone();
			let delete_id = id.clone();
			let delete_key = id.clone();
			let confirming = catalog.confirming(&id);
			let busy = catalog.busy.as_ref() == Some(&id);
			let unavailable = catalog.mutation.is_some();
			let restore = archive_action(
				format!("unarchive-{id}"),
				Symbol::Unarchive,
				if busy && !catalog.deleting { "Restoring…" } else { "Unarchive" },
			)
			.aria_label(format!("Unarchive {}", item.title))
			.when(unavailable, |d| d.opacity(0.45))
			.on_click(cx.listener(move |s, _, _, cx| {
				cx.stop_propagation();
				s.unarchive_catalog_item(&restore_id, cx);
			}))
			.on_key_down(cx.listener(move |s, e: &KeyDownEvent, _, cx| {
				if !e.is_held && ["enter", "space"].contains(&e.keystroke.key.as_str()) {
					cx.stop_propagation();
					s.unarchive_catalog_item(&restore_key, cx);
				}
			}));
			let delete = archive_action(
				format!("delete-archive-{id}"),
				Symbol::Trash,
				if busy && catalog.deleting {
					"Deleting…"
				} else if confirming {
					"Click again to delete permanently"
				} else {
					"Delete"
				},
			)
			.aria_label(format!(
				"{} {}",
				if confirming { "Confirm permanent deletion of" } else { "Delete" },
				item.title
			))
			.when(unavailable, |d| d.opacity(0.45))
			.when(confirming, |d| {
				d.text_color(ui_theme::rgb(ui_theme::ERROR))
					.bg(ui_theme::rgb(ui_theme::ERROR).opacity(0.12))
					.border_color(ui_theme::rgb(ui_theme::ERROR).opacity(0.35))
			})
			.on_click(cx.listener(move |s, _, _, cx| {
				cx.stop_propagation();
				s.delete_archive_item(&delete_id, cx);
			}))
			.on_key_down(cx.listener(move |s, e: &KeyDownEvent, _, cx| {
				if !e.is_held && ["enter", "space"].contains(&e.keystroke.key.as_str()) {
					cx.stop_propagation();
					s.delete_archive_item(&delete_key, cx);
				}
			}));
			let delete = delete.smooth();
			let metadata = if confirming {
				"Click again to delete this agent and its subagents".into()
			} else {
				item.metadata.clone()
			};
			list = list.child(
				gpui::div()
					.id(gpui::SharedString::from(format!("archived-{id}")))
					.debug_selector(move || format!("archived-{id}"))
					.role(Role::Button)
					.aria_label(format!("Open archived {}", item.title))
					.tab_index(0)
					.flex_none()
					.min_w_0()
					.h(gpui::px(48.))
					.px(gpui::px(10.))
					.flex()
					.items_center()
					.gap(gpui::px(12.))
					.border_b_1()
					.border_color(ui_theme::rgb(ui_theme::TEXT).opacity(0.08))
					.cursor_pointer()
					.hover(|d| d.bg(ui_theme::rgba(ui_theme::HOVER_FILL)))
					.active(|d| d.bg(ui_theme::rgba(ui_theme::PRESSED_FILL)))
					.focus_visible(|d| d.bg(ui_theme::rgba(ui_theme::HOVER_FILL)))
					.on_click(
						cx.listener(move |s, _, _, cx| s.open_archived_conversation(&open_id, cx)),
					)
					.on_key_down(cx.listener(move |s, e: &KeyDownEvent, _, cx| {
						if !e.is_held && ["enter", "space"].contains(&e.keystroke.key.as_str()) {
							cx.stop_propagation();
							s.open_archived_conversation(&open_key, cx);
						}
					}))
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
									.line_height(gpui::px(16.))
									.text_color(ui_theme::rgb(ui_theme::TEXT))
									.truncate()
									.child(item.title.clone()),
							)
							.child(
								gpui::div()
									.text_size(gpui::px(ui_theme::CAPTION_SIZE))
									.line_height(gpui::px(ui_theme::CAPTION_LINE_HEIGHT))
									.text_color(ui_theme::rgb(if confirming {
										ui_theme::ERROR
									} else {
										ui_theme::TEXT_MUTED
									}))
									.truncate()
									.child(metadata),
							),
					)
					.child(
						gpui::div()
							.flex_none()
							.flex()
							.items_center()
							.gap(gpui::px(6.))
							.child(restore.smooth())
							.child(delete),
					),
			);
		}
		if rows.is_empty() && catalog.read.is_none() && catalog.feedback.is_none() {
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
		let feedback = catalog.feedback.clone();
		let initial_read = catalog.read.is_some() && catalog.data.is_none();
		gpui::div()
			.id("archive-browser-content")
			.debug_selector(|| "archive-browser-content".into())
			.flex_1()
			.min_w_0()
			.min_h_0()
			.flex()
			.flex_col()
			.p_3()
			.gap_2()
			.on_key_down(cx.listener(|s, e: &KeyDownEvent, _, cx| {
				if e.keystroke.key == "escape" && s.workspace.archives.confirmation.is_some() {
					s.workspace.archives.confirmation = None;
					s.workspace.archives.confirmation_task = None;
					cx.stop_propagation();
					cx.notify();
				}
			}))
			.child(self.archive_catalog_toolbar(cx))
			.children(feedback.as_ref().and_then(|message| {
				ui_feedback::inline_feedback("archive-feedback", "", &Feedback::error(message))
			}))
			.children(initial_read.then(|| ui_loading::loading("Reading archive…")))
			.child(
				list.smooth_scroll("archive-list-motion")
					.contain_scroll()
					.with_scrollbar("archive-scrollbar", "Scroll archive"),
			)
			.into_any_element()
	}

	fn archive_catalog_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
		gpui::div()
			.flex()
			.items_center()
			.gap_3()
			.child(
				gpui::div()
					.flex_1()
					.min_w_0()
					.h(gpui::px(ui_theme::CONTROL_SIZE))
					.children(self.workspace.archives.search.clone()),
			)
			.child(Segmented::new(
				"archive-sort",
				"Sort archive",
				[
					("Recent", "Newest updates first"),
					("Oldest", "Oldest updates first"),
					("Name", "Sort by name"),
				]
				.map(|(label, description)| {
					Choice::new(format!("archive-sort-{label}"), label, description)
				}),
				match self.workspace.archives.sort {
					Sort::Recent => 0,
					Sort::Oldest => 1,
					Sort::Name => 2,
				},
				cx.listener(|s, index: &usize, _, cx| {
					s.workspace.archives.sort = [Sort::Recent, Sort::Oldest, Sort::Name][*index];
					s.workspace.archives.confirmation = None;
					s.workspace.archives.confirmation_task = None;
					cx.notify();
				}),
			))
	}
}

fn archive_action(
	id: impl Into<gpui::SharedString>,
	symbol: Symbol,
	label: &'static str,
) -> gpui::Stateful<gpui::Div> {
	let id = id.into();
	gpui::div()
		.id(id.clone())
		.debug_selector(move || id.to_string())
		.role(Role::Button)
		.tab_index(0)
		.size(gpui::px(ui_theme::CHROME_CONTROL_SIZE))
		.flex_none()
		.flex()
		.items_center()
		.justify_center()
		.rounded(gpui::px(ui_theme::SETTINGS_CONTROL_RADIUS))
		.border(gpui::px(0.5))
		.border_color(ui_theme::rgb(ui_theme::TEXT).opacity(0.10))
		.bg(ui_theme::rgb(ui_theme::TEXT).opacity(0.035))
		.text_color(ui_theme::rgb(ui_theme::TEXT))
		.cursor_pointer()
		.hover(|d| d.bg(ui_theme::rgba(ui_theme::HOVER_FILL)))
		.active(|d| d.bg(ui_theme::rgba(ui_theme::PRESSED_FILL)))
		.focus_visible(|d| d.bg(ui_theme::rgba(ui_theme::HOVER_FILL)))
		.tooltip(move |_, cx| cx.new(|_| ControlTooltip(label)).into())
		.child(workspace_symbols::icon_sized(symbol, 16.))
}

fn archive_metadata(project: Option<&str>, micros: i64) -> String {
	let seconds = (micros / 1_000_000) as libc::time_t;
	let mut local = std::mem::MaybeUninit::<libc::tm>::zeroed();
	// SAFETY: localtime_r writes a tm into valid storage for the supplied timestamp.
	let local = unsafe {
		libc::localtime_r(&seconds, local.as_mut_ptr());
		local.assume_init()
	};
	let date = format!(
		"Updated {:04}-{:02}-{:02} · {:02}:{:02}",
		local.tm_year + 1900,
		local.tm_mon + 1,
		local.tm_mday,
		local.tm_hour,
		local.tm_min
	);
	project.map_or_else(|| date.clone(), |name| format!("{name} · {date}"))
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
	fn archive_search_matches_visible_project_and_agent_names(cx: &mut gpui::TestAppContext) {
		let (view, visual) = cx.add_window_view(|_, cx| {
			let surface = cx.new(AgentSurface::new);
			cx.observe(&surface, |_, _, cx| cx.notify()).detach();
			surface.update(cx, |s, cx| {
				s.visual_workspace_fixture(cx);
				s.composer.update(cx, |input, cx| input.set_content("Keep this draft", cx));
				let mut data = s.snapshot.clone().unwrap();
				for (id, title) in [
					("design", "Design review"),
					("reading", "Reading plan"),
					("other", "Another design"),
				] {
					let mut item = data.work_items[0].clone();
					item.id = id.into();
					item.title = title.into();
					data.work_items.push(item);
					data.archived_work_ids.insert(id.into());
				}
				data.projects = vec![decodex_protocol::ProjectDto {
					id: "books".into(),
					name: "Reading corner".into(),
					native_project_id: None,
					lead_agent_id: None,
					directory: "/tmp/books".into(),
					agent_ids: vec!["design".into(), "reading".into()],
				}];
				s.workspace.archives.data = Some(data);
				s.workspace.archives.search =
					Some(cx.new(|cx| {
						ComposerInput::with_placeholder(48, "Search", "Search archive", cx)
					}));
			});
			ArchiveView(surface)
		});
		let surface = view.read_with(visual, |v, _| v.0.clone());
		for (query, expected) in [
			("READING CORNER", [true, true, false]),
			("design reading", [true, false, false]),
			("missing", [false; 3]),
		] {
			surface.update(visual, |s, cx| {
				s.workspace
					.archives
					.search
					.as_ref()
					.unwrap()
					.update(cx, |input, cx| input.set_content(query, cx));
				cx.notify();
			});
			visual.update(crate::ui_frame::draw_current_frame);
			for (selector, present) in ["unarchive-design", "unarchive-reading", "unarchive-other"]
				.into_iter()
				.zip(expected)
			{
				assert_eq!(visual.debug_bounds(selector).is_some(), present, "query: {query}");
			}
		}
		surface.read_with(visual, |s, cx| {
			assert_eq!(s.composer.read(cx).content(), "Keep this draft");
			assert!(s.workspace.archives.mutation.is_none());
		});
	}

	#[gpui::test]
	fn archive_rows_open_directly_and_delete_confirmation_does_not_open_the_row(
		cx: &mut gpui::TestAppContext,
	) {
		let (view, visual) = cx.add_window_view(|_, cx| {
			let surface = cx.new(AgentSurface::new);
			cx.observe(&surface, |_, _, cx| cx.notify()).detach();
			surface.update(cx, |s, cx| {
				s.visual_workspace_fixture(cx);
				s.composer.update(cx, |i, cx| i.set_content("Keep the draft", cx));
				let mut data = s.snapshot.clone().unwrap();
				for (id, title, time) in [("z", "Zeta", 3), ("a", "Alpha", 2), ("b", "Beta", 1)] {
					let mut item = data.work_items[0].clone();
					item.id = id.into();
					item.title = title.into();
					item.updated_at_micros = time;
					data.work_items.push(item);
					data.archived_work_ids.insert(id.into());
				}
				s.workspace.archives.data = Some(data);
			});
			ArchiveView(surface)
		});
		let surface = view.read_with(visual, |v, _| v.0.clone());
		visual.update(crate::ui_frame::draw_current_frame);
		assert!(visual.debug_bounds("open-archive-z").is_none());
		let button = visual.debug_bounds("delete-archive-z").unwrap();
		visual.simulate_click(button.center(), Default::default());
		surface.read_with(visual, |s, cx| {
			assert!(s.workspace.archives.confirming("z"));
			assert!(s.workspace.archives.viewed.is_none());
			assert!(s.workspace.archives.mutation.is_none());
			assert_eq!(s.composer.read(cx).content(), "Keep the draft");
		});
		visual.update(crate::ui_frame::draw_current_frame);
		let name = visual.debug_bounds("archive-sort-Name").unwrap();
		visual.simulate_click(name.center(), Default::default());
		visual.update(crate::ui_frame::draw_current_frame);
		surface.read_with(visual, |s, _| {
			assert!(s.workspace.archives.confirmation.is_none());
			assert_eq!(
				s.workspace.archives.rows.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
				vec!["a", "b", "z"]
			);
		});
		let row = visual.debug_bounds("archived-a").unwrap();
		visual.simulate_click(
			gpui::point(row.left() + gpui::px(24.), row.center().y),
			Default::default(),
		);
		surface.read_with(visual, |s, _| {
			assert_eq!(s.workspace.archives.viewed.as_deref(), Some("a"))
		});
	}

	#[gpui::test]
	fn archive_second_delete_click_sends_once_and_removes_only_the_confirmed_row(
		cx: &mut gpui::TestAppContext,
	) {
		cx.background_executor.allow_parking();
		let (view, visual) = cx.add_window_view(|_, cx| {
			let surface = cx.new(AgentSurface::new);
			cx.observe(&surface, |_, _, cx| cx.notify()).detach();
			surface.update(cx, |s, cx| {
				s.visual_workspace_fixture(cx);
				s.composer.update(cx, |i, cx| i.set_content("Preserve Main draft", cx));
				let mut data = s.snapshot.clone().unwrap();
				let mut item = data.work_items[0].clone();
				item.id = "discard".into();
				item.codex_thread_id = Some("native-discard".into());
				data.work_items.push(item);
				data.archived_work_ids.insert("discard".into());
				s.workspace.archives.data = Some(data);
			});
			ArchiveView(surface)
		});
		let surface = view.read_with(visual, |v, _| v.0.clone());
		let active = surface.read_with(visual, |s, _| s.snapshot.clone().unwrap());
		let expected = active.clone();
		let (_root, profile, server) = wire_test_support::fixture(move |listener| async move {
			for step in 0..3 {
				let mut socket = wire_test_support::accept(&listener).await;
				let Message::Text(text) = socket.next().await.unwrap().unwrap() else {
					panic!("request")
				};
				let request: ClientMessage = serde_json::from_str(&text).unwrap();
				let result = if step == 0 {
					let ClientMessage::Command(command) = request else { panic!("delete command") };
					assert!(
						matches!(&command.payload,CommandPayload::Agent {action} if matches!(&**action,AgentActionDto::DeleteArchivedThread {work_id,thread_id} if work_id.as_str()=="discard" && thread_id.as_str()=="native-discard"))
					);
					let receipt = ServerMessage::CommandReceipt(decodex_protocol::CommandReceipt {
						version: CURRENT_VERSION,
						server_id: ServerId::new(SERVER).unwrap(),
						client_command_id: command.client_command_id.clone(),
						idempotency_key: command.idempotency_key.clone(),
						disposition: decodex_protocol::ReceiptDisposition::Executed,
						original_client_command_id: command.client_command_id.clone(),
					});
					socket
						.send(Message::Text(serde_json::to_string(&receipt).unwrap().into()))
						.await
						.unwrap();
					ServerMessage::CommandResult(decodex_protocol::CommandResultEnvelope {
						version: CURRENT_VERSION,
						server_id: ServerId::new(SERVER).unwrap(),
						client_command_id: command.client_command_id,
						idempotency_key: command.idempotency_key,
						outcome: decodex_protocol::CommandOutcome::Succeeded,
						entity_revision: Some(decodex_protocol::EntityRevision(0)),
						payload: Some(decodex_protocol::ResultPayload::AgentAccepted {
							work_id: EntityId::new("discard").unwrap(),
						}),
						error: None,
					})
				} else {
					let ClientMessage::Query(query) = request else { panic!("refresh") };
					assert!(matches!(
						(&query.payload, step),
						(QueryPayload::GetAgentArchiveCatalog, 1)
							| (QueryPayload::GetAgentSnapshot, 2)
					));
					ServerMessage::QueryResult(QueryResultEnvelope {
						version: CURRENT_VERSION,
						server_id: ServerId::new(SERVER).unwrap(),
						query_id: query.query_id,
						payload: QueryResultPayload::AgentSnapshot(AgentSnapshotResult::Available(
							active.clone(),
						)),
					})
				};
				socket
					.send(Message::Text(serde_json::to_string(&result).unwrap().into()))
					.await
					.unwrap();
			}
		});
		surface.update(visual, |s, _| s.profile = Some(profile));
		visual.update(crate::ui_frame::draw_current_frame);
		let button = visual.debug_bounds("delete-archive-discard").unwrap();
		visual.simulate_click(button.center(), Default::default());
		surface.read_with(visual, |s, _| {
			assert!(s.workspace.archives.mutation.is_none());
			assert!(s.workspace.archives.confirming("discard"));
		});
		visual.update(crate::ui_frame::draw_current_frame);
		let confirmed = visual.debug_bounds("delete-archive-discard").unwrap();
		assert!(
			(confirmed.center().x - button.center().x).abs() < gpui::px(4.),
			"confirmation must keep its click target"
		);
		surface.read_with(visual, |s, _| {
			assert!(
				s.workspace.archives.confirming("discard"),
				"confirmation expired during render"
			)
		});
		visual.simulate_click(confirmed.center(), Default::default());
		surface.read_with(visual, |s, _| {
			assert!(
				s.workspace.archives.confirmation.is_none(),
				"second click must leave confirmation"
			)
		});
		let until = Instant::now() + Duration::from_secs(5);
		loop {
			visual.run_until_parked();
			if surface.read_with(visual, |s, _| s.workspace.archives.mutation.is_none()) {
				break;
			}
			assert!(Instant::now() < until, "delete request timed out");
			thread::sleep(Duration::from_millis(5));
		}
		server.join().unwrap();
		visual.update(crate::ui_frame::draw_current_frame);
		assert!(visual.debug_bounds("delete-archive-discard").is_none());
		surface.read_with(visual, |s, cx| {
			assert_eq!(s.snapshot.as_ref(), Some(&expected));
			assert_eq!(s.composer.read(cx).content(), "Preserve Main draft");
			assert!(s.workspace.archives.viewed.is_none());
			assert!(s.workspace.archives.feedback.is_none());
		});
	}

	#[gpui::test]
	fn archive_scrollbar_moves_rows_within_the_bounded_viewport(cx: &mut gpui::TestAppContext) {
		let (_view, visual) = cx.add_window_view(|_, cx| {
			let surface = cx.new(AgentSurface::new);
			cx.observe(&surface, |_, _, cx| cx.notify()).detach();
			surface.update(cx, |s, cx| {
				s.visual_workspace_fixture(cx);
				let mut data = s.snapshot.clone().unwrap();
				for index in 0..100 {
					let mut item = data.work_items[0].clone();
					item.id = format!("scroll-{index:02}");
					item.title = item.id.clone();
					data.archived_work_ids.insert(item.id.clone());
					data.work_items.push(item);
				}
				s.workspace.archives.data = Some(data);
			});
			ArchiveView(surface)
		});
		visual.update(crate::ui_frame::draw_current_frame);
		let before = visual.debug_bounds("archived-scroll-00").unwrap();
		let bar = visual.debug_bounds("archive-scrollbar").unwrap();
		// Keep enough compact rows to exercise scrolling, rather than grabbing a large thumb.
		assert!(visual.debug_bounds("archived-scroll-99").unwrap().bottom() > bar.bottom());
		visual.simulate_click(bar.center(), Default::default());
		visual.update(crate::ui_frame::draw_current_frame);
		let after = visual.debug_bounds("archived-scroll-00").unwrap();
		assert!(
			after.top() < before.top(),
			"scrollbar must move overflowing archive rows: bar={bar:?}, before={before:?}, after={after:?}"
		);
	}

	#[gpui::test]
	fn opening_archive_is_read_only_and_retains_the_active_draft(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);
			let active = s.snapshot.clone().unwrap();
			let selected = s.selected.clone().unwrap();
			s.open_page(&selected, cx);
			s.composer.update(cx, |input, cx| input.set_content("Keep the active draft", cx));
			let mut catalog = active.clone();
			let mut archived = catalog.work_items[0].clone();
			archived.id = "archived-test".into();
			archived.codex_thread_id = Some("native-archived-test".into());
			archived.parent_goal_id = None;
			catalog.work_items.push(archived);
			catalog.archived_work_ids.insert("archived-test".into());
			s.workspace.archives.data = Some(catalog);

			s.open_archived_conversation("archived-test", cx);
			assert_eq!(s.selected.as_deref(), Some("archived-test"));
			assert!(s.selected_is_archived());
			assert_eq!(
				s.conversation_work().unwrap().codex_thread_id.as_deref(),
				Some("native-archived-test")
			);
			assert!(s.build_submission("Do not send this", cx).is_err());
			assert_eq!(s.snapshot.as_ref(), Some(&active));
			assert!(s.workspace.archives.mutation.is_none());

			s.apply_refreshed_snapshot(Ok(AgentSnapshotResult::Available(active)), cx);
			assert_eq!(s.selected.as_deref(), Some("archived-test"));
			assert!(
				s.selected_is_archived(),
				"background refresh must preserve an explicit archive read"
			);
			let location = s.navigation_location();
			s.open_page(&selected, cx);
			assert!(!s.selected_is_archived());
			assert_eq!(s.composer.read(cx).content(), "Keep the active draft");
			assert!(s.restore_location(&location, cx));
			assert!(s.selected_is_archived(), "navigation restores the read-only archive view");
		});
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
