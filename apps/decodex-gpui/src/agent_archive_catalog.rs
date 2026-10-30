//! On-demand archive browser. Native identities and retained relations stay authoritative.
use gpui::{
	AnyElement, AppContext as _, Entity, IntoElement as _, KeyDownEvent, Role, Task, prelude::*,
};
use std::{
	collections::BTreeSet,
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
	ui_motion::{self, Choice, ConfirmationNudge, Segmented, SmoothControl as _},
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
	confirmation: Option<(Vec<String>, Instant)>,
	confirmation_task: Option<Task<()>>,
	deleting: bool,
	selected: BTreeSet<String>,
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
	project: Option<String>,
	day: String,
	clock: String,
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
				query.split_whitespace().all(|word| text.contains(word)).then(|| {
					let (day, clock) = archive_timestamp(item.updated_at_micros);
					let date = format!("{day} · {clock}");
					Row {
						id: item.id.clone(),
						title: item.title.clone(),
						metadata: project
							.as_ref()
							.map_or_else(|| date.clone(), |name| format!("{name} · {date}")),
						project,
						day,
						clock,
						updated: item.updated_at_micros,
					}
				})
			})
			.collect();
		rows.sort_by(|a, b| match self.sort {
			Sort::Recent => b.updated.cmp(&a.updated).then(a.title.cmp(&b.title)),
			Sort::Oldest => a.updated.cmp(&b.updated).then(a.title.cmp(&b.title)),
			Sort::Name => a.title.to_lowercase().cmp(&b.title.to_lowercase()).then(a.id.cmp(&b.id)),
		});
		self.rows = rows.into();
		self.selected.retain(|id| self.rows.iter().any(|row| &row.id == id));
		self.prepared = Some(key);
	}

	fn confirming(&self, id: &str) -> bool {
		self.confirmation.as_ref().is_some_and(|(owners, at)| {
			owners.len() == 1 && owners[0] == id && at.elapsed() < Duration::from_secs(5)
		})
	}

	fn confirming_selection(&self) -> bool {
		self.confirmation.as_ref().is_some_and(|(owners, at)| {
			!owners.is_empty()
				&& owners.len() == self.selected.len()
				&& owners.iter().all(|id| self.selected.contains(id))
				&& at.elapsed() < Duration::from_secs(5)
		})
	}

	fn cancel_confirmation(&mut self) {
		self.confirmation = None;
		self.confirmation_task = None;
	}

	fn toggle_selected(&mut self, id: &str) {
		self.cancel_confirmation();
		if !self.selected.remove(id) {
			self.selected.insert(id.into());
		}
	}

	fn parent_depth(&self, id: &str) -> usize {
		let mut seen = BTreeSet::new();
		let mut parent = Some(id);
		while let Some(id) = parent {
			if !seen.insert(id) {
				break;
			}
			parent = self.data.as_ref().and_then(|data| {
				data.work_items.iter().find(|work| work.id == id)?.parent_goal_id.as_deref()
			});
		}
		seen.len()
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
		self.workspace.archives.cancel_confirmation();
		self.workspace.archives.selected.clear();
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
				s.workspace.archives.cancel_confirmation();
				s.workspace.archives.selected.clear();
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
		self.delete_archive_items(vec![id.into()], cx);
	}

	fn delete_archive_items(&mut self, mut ids: Vec<String>, cx: &mut Context<Self>) {
		if self.workspace.archives.mutation.is_some()
			|| ids.is_empty()
			|| ids.iter().any(|id| self.workspace.archives.archived_work(id).is_none())
		{
			return;
		}
		// Native deletion includes descendants. Delete selected children first so
		// their individual receipts do not race an earlier parent deletion.
		ids.sort_by_cached_key(|id| std::cmp::Reverse(self.workspace.archives.parent_depth(id)));
		if !self
			.workspace
			.archives
			.confirmation
			.as_ref()
			.is_some_and(|(owners, at)| owners == &ids && at.elapsed() < Duration::from_secs(5))
		{
			let now = Instant::now();
			self.workspace.archives.confirmation = Some((ids.clone(), now));
			self.workspace.archives.confirmation_task = Some(cx.spawn(async move |surface, cx| {
				cx.background_executor().timer(Duration::from_secs(5)).await;
				let _ = surface.update(cx, |s, cx| {
					if s.workspace.archives.confirmation.as_ref() == Some(&(ids, now)) {
						s.workspace.archives.confirmation = None;
						cx.notify();
					}
				});
			}));
			cx.notify();
			return;
		}
		let Some(profile) = self.profile.clone() else {
			return;
		};
		let targets: Vec<_> = ids
			.iter()
			.filter_map(|id| {
				let thread = self.workspace.archives.archived_work(id)?.codex_thread_id.clone()?;
				Some((id.clone(), EntityId::new(id.clone()).ok()?, WireText::new(thread).ok()?))
			})
			.collect();
		if targets.len() != ids.len() {
			return;
		}
		self.workspace.archives.cancel_confirmation();
		self.workspace.archives.busy = ids.first().cloned();
		self.workspace.archives.deleting = true;
		self.workspace.archives.read = None;
		self.workspace.archives.feedback = None;
		let request = cx.background_executor().spawn(async move {
			let runtime = Builder::new_current_thread().enable_all().build().ok()?;
			let client = AgentClient::new(profile);
			let mut deleted = Vec::new();
			for (id, work_id, thread_id) in targets {
				let key = IdempotencyKey::new(super::super::unique_command()).ok()?;
				let response = runtime.block_on(
					client
						.execute(AgentActionDto::DeleteArchivedThread { work_id, thread_id }, key),
				);
				match response {
					Ok(decodex_protocol::AgentCommandResponse::Accepted { .. }) => deleted.push(id),
					Ok(_) => {},
					Err(_) => break,
				}
			}
			Some((
				deleted,
				runtime.block_on(client.archive_catalog()).ok(),
				runtime.block_on(client.query()).ok(),
			))
		});
		self.workspace.archives.mutation = Some(cx.spawn(async move |surface, cx| {
			let result = request.await;
			let _ = surface.update(cx, |s, cx| {
				s.workspace.archives.mutation = None;
				s.workspace.archives.busy = None;
				if let Some((deleted, catalog, active)) = result {
					if let Some(AgentSnapshotResult::Available(data)) = catalog {
						s.workspace.archives.data = Some(data);
					} else if let Some(data) = &mut s.workspace.archives.data {
						data.work_items.retain(|w| !deleted.contains(&w.id));
						data.archived_work_ids.retain(|id| !deleted.contains(id));
					}
					s.workspace.archives.revision += 1;
					s.workspace.archives.selected.retain(|id| !deleted.contains(id));
					if s.workspace.archives.viewed.as_ref().is_some_and(|id| deleted.contains(id)) {
						s.workspace.archives.viewed = None;
					}
					let remaining = ids.len() - deleted.len();
					if remaining > 0 {
						s.workspace.archives.feedback = Some(format!(
							"{remaining} not deleted. Check the selection before retrying."
						));
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
		let selecting = !catalog.selected.is_empty();
		let rows = catalog.rows.clone();
		let mut list = gpui::div()
			.id("archive-list")
			.debug_selector(|| "archive-list".into())
			.size_full()
			.min_h_0()
			.overflow_y_scroll()
			.flex()
			.flex_col()
			.gap(gpui::px(16.))
			.pr(gpui::px(10.))
			.pb(gpui::px(6.));
		for group in rows.chunk_by(|a, b| catalog.sort == Sort::Name || a.day == b.day) {
			let first = &group[0];
			let mut column = gpui::div()
				.id(gpui::SharedString::from(format!("archive-section-{}", first.id)))
				.w_full()
				.flex_none()
				.min_w_0()
				.flex()
				.flex_col()
				.gap(gpui::px(0.));
			for (index, item) in group.iter().enumerate() {
				let id = item.id.clone();
				let open_id = id.clone();
				let open_key = id.clone();
				let restore_id = id.clone();
				let restore_key = id.clone();
				let delete_id = id.clone();
				let delete_key = id.clone();
				let confirming = catalog.confirming(&id);
				let selected = catalog.selected.contains(&id);
				let check_reveal_id: gpui::SharedString = format!("archive-select-{id}").into();
				let check_id = id.clone();
				let check_key = id.clone();
				let check = archive_checkbox(
					format!("select-archive-{id}"),
					format!("Select {}", item.title),
					selected,
					false,
				)
				.on_click(cx.listener(move |s, _, _, cx| {
					cx.stop_propagation();
					if s.workspace.archives.mutation.is_none() {
						s.workspace.archives.toggle_selected(&check_id);
						cx.notify();
					}
				}))
				.on_key_down(cx.listener(move |s, e: &KeyDownEvent, _, cx| {
					if !e.is_held && ["enter", "space"].contains(&e.keystroke.key.as_str()) {
						cx.stop_propagation();
						if s.workspace.archives.mutation.is_none() {
							s.workspace.archives.toggle_selected(&check_key);
							cx.notify();
						}
					}
				}));
				let busy = catalog.busy.as_ref() == Some(&id);
				let unavailable = catalog.mutation.is_some();
				let restore = archive_action(
					format!("unarchive-{id}"),
					Symbol::Unarchive,
					if busy && !catalog.deleting { "Restoring…" } else { "Unarchive" },
					None,
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
						"Delete permanently"
					} else {
						"Delete"
					},
					catalog
						.confirmation
						.as_ref()
						.filter(|(owners, _)| owners.as_slice() == [item.id.as_str()])
						.map(|(_, at)| *at),
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
				let open = archive_action(
					format!("open-archive-{id}"),
					Symbol::Forward,
					"Open conversation",
					None,
				)
				.aria_label(format!("Open archived {}", item.title))
				.on_click(cx.listener(move |s, _, _, cx| {
					cx.stop_propagation();
					s.open_archived_conversation(&open_id, cx);
				}))
				.on_key_down(cx.listener(move |s, e: &KeyDownEvent, _, cx| {
					if !e.is_held && ["enter", "space"].contains(&e.keystroke.key.as_str()) {
						cx.stop_propagation();
						s.open_archived_conversation(&open_key, cx);
					}
				}));
				let row_id = id.clone();
				let row_key = id.clone();
				let detail = item.metadata.clone();
				let content = gpui::div()
					.min_w_0()
					.flex_1()
					.flex()
					.items_center()
					.gap(gpui::px(8.))
					.child(
						gpui::div()
							.min_w_0()
							.flex_1()
							.flex()
							.flex_wrap()
							.items_baseline()
							.gap_x(gpui::px(8.))
							.child(
								gpui::div()
									.debug_selector({
										let id = item.id.clone();
										move || format!("archive-title-{id}")
									})
									.flex_1()
									.flex_basis(gpui::px(280.))
									.min_w_0()
									.text_size(gpui::px(ui_theme::BODY_SIZE))
									.line_height(gpui::px(ui_theme::BODY_LINE_HEIGHT))
									.font_weight(gpui::FontWeight::NORMAL)
									.text_color(ui_theme::rgb(ui_theme::TEXT))
									.truncate()
									.child(item.title.clone()),
							)
							.children(item.project.as_ref().map(|project| {
								gpui::div()
									.debug_selector({
										let id = item.id.clone();
										move || format!("archive-project-{id}")
									})
									.flex_none()
									.max_w(gpui::px(100.))
									.text_size(gpui::px(ui_theme::CAPTION_SIZE))
									.line_height(gpui::px(ui_theme::CAPTION_LINE_HEIGHT))
									.text_color(ui_theme::rgb(ui_theme::TEXT_MUTED))
									.truncate()
									.child(project.clone())
							})),
					)
					.child(
						gpui::div()
							.id(gpui::SharedString::from(format!("archive-time-{}", item.id)))
							.flex_none()
							.min_w(gpui::px(if catalog.sort == Sort::Name { 78. } else { 36. }))
							.text_right()
							.font_features(ui_theme::tabular_figures())
							.text_size(gpui::px(ui_theme::CAPTION_SIZE))
							.line_height(gpui::px(ui_theme::CAPTION_LINE_HEIGHT))
							.text_color(ui_theme::rgb(ui_theme::TEXT_MUTED))
							.tooltip(move |_, cx| cx.new(|_| ControlTooltip(detail.clone())).into())
							.child(if catalog.sort == Sort::Name {
								item.day.clone()
							} else {
								item.clock.clone()
							}),
					)
					.child(
						gpui::div()
							.debug_selector({
								let id = item.id.clone();
								move || format!("archive-actions-{id}")
							})
							.flex_none()
							.flex()
							.items_center()
							.gap(gpui::px(4.))
							.children((!selecting).then(|| restore.smooth().enabled(!unavailable)))
							.children((!selecting).then(|| delete.smooth().enabled(!unavailable)))
							.child(open.smooth().enabled(!unavailable)),
					);
				column = column.child(ArchiveRow {
					id: check_reveal_id,
					active: selecting,
					checkbox: check.smooth().enabled(!unavailable).into_any_element(),
					content: content.into_any_element(),
					row: gpui::div()
						.id(gpui::SharedString::from(format!("archived-{id}")))
						.debug_selector(move || format!("archived-{id}"))
						.role(Role::CheckBox)
						.aria_label(format!("Select {}", item.title))
						.aria_toggled(if selected {
							gpui::Toggled::True
						} else {
							gpui::Toggled::False
						})
						.aria_description(item.metadata.clone())
						.tab_index(0)
						.flex_none()
						.min_w_0()
						.min_h(gpui::px(40.))
						.px(gpui::px(8.))
						.py(gpui::px(7.))
						.flex()
						.items_center()
						.gap(gpui::px(8.))
						.rounded(gpui::px(4.))
						.bg(ui_theme::rgb(ui_theme::TEXT).opacity(if index % 2 == 0 {
							0.018
						} else {
							0.
						}))
						.relative()
						.when(selected, |row| row.bg(ui_theme::rgb(ui_theme::ACCENT).opacity(0.18)))
						.cursor_pointer()
						.hover(move |d| {
							d.bg(if selected {
								ui_theme::rgb(ui_theme::ACCENT).opacity(0.24)
							} else {
								ui_theme::rgb(ui_theme::ACCENT).opacity(0.075)
							})
						})
						.active(|d| d.bg(ui_theme::rgb(ui_theme::ACCENT).opacity(0.24)))
						.focus_visible(|d| d.bg(ui_theme::rgb(ui_theme::ACCENT).opacity(0.075)))
						.on_click(cx.listener(move |s, _, _, cx| {
							s.select_archive_row(&row_id, cx);
						}))
						.on_key_down(cx.listener(move |s, e: &KeyDownEvent, _, cx| {
							if !e.is_held && ["enter", "space"].contains(&e.keystroke.key.as_str())
							{
								cx.stop_propagation();
								s.select_archive_row(&row_key, cx);
							}
						})),
				});
			}
			let heading_id = first.id.clone();
			list = list.child(
				gpui::div()
					.w_full()
					.flex_none()
					.flex()
					.flex_col()
					.gap(gpui::px(6.))
					.when(catalog.sort != Sort::Name, |section| {
						section.child(
							gpui::div()
								.id(gpui::SharedString::from(format!("archive-date-{}", first.id)))
								.debug_selector(move || format!("archive-date-{heading_id}"))
								.role(Role::Heading)
								.aria_label(first.day.clone())
								.w_full()
								.flex_none()
								.pl(gpui::px(40.))
								.pt(gpui::px(6.))
								.text_size(gpui::px(ui_theme::CONTROL_LABEL_SIZE))
								.font_weight(gpui::FontWeight::NORMAL)
								.line_height(gpui::px(ui_theme::CAPTION_LINE_HEIGHT))
								.text_color(ui_theme::rgb(ui_theme::TEXT).opacity(0.78))
								.child(first.day.clone()),
						)
					})
					.child(column),
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
		let body = gpui::div()
			.id("archive-browser-content")
			.debug_selector(|| "archive-browser-content".into())
			.flex_1()
			.w_full()
			.max_w(gpui::px(960.))
			.min_w_0()
			.min_h_0()
			.flex()
			.flex_col()
			.p_3()
			.gap_2()
			.on_key_down(cx.listener(|s, e: &KeyDownEvent, _, cx| {
				if e.keystroke.key == "escape"
					&& (s.workspace.archives.confirmation.is_some()
						|| !s.workspace.archives.selected.is_empty())
				{
					if s.workspace.archives.confirmation.is_some() {
						s.workspace.archives.cancel_confirmation();
					} else if s.workspace.archives.mutation.is_none() {
						s.workspace.archives.selected.clear();
					}
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
				gpui::div()
					.id("archive-viewport")
					.debug_selector(|| "archive-viewport".into())
					.flex_1()
					.min_h_0()
					.w_full()
					.overflow_hidden()
					.child(
						list.smooth_scroll("archive-list-motion")
							.contain_scroll()
							.with_scrollbar("archive-scrollbar", "Scroll archive"),
					),
			)
			.child(self.archive_selection_bar(cx));
		gpui::div()
			.size_full()
			.min_w_0()
			.min_h_0()
			.flex()
			.flex_col()
			.items_center()
			.child(body)
			.into_any_element()
	}

	fn select_all_archived(&mut self, cx: &mut Context<Self>) {
		let catalog = &mut self.workspace.archives;
		if catalog.mutation.is_none() {
			catalog.cancel_confirmation();
			if catalog.selected.len() == catalog.rows.len() {
				catalog.selected.clear();
			} else {
				catalog.selected = catalog.rows.iter().map(|row| row.id.clone()).collect();
			}
			cx.notify();
		}
	}

	fn delete_selected_archives(&mut self, cx: &mut Context<Self>) {
		self.delete_archive_items(self.workspace.archives.selected.iter().cloned().collect(), cx);
	}

	fn select_archive_row(&mut self, id: &str, cx: &mut Context<Self>) {
		if self.workspace.archives.mutation.is_none() {
			self.workspace.archives.toggle_selected(id);
			cx.notify();
		}
	}

	fn clear_archive_selection(&mut self, cx: &mut Context<Self>) {
		if self.workspace.archives.mutation.is_none() {
			self.workspace.archives.cancel_confirmation();
			self.workspace.archives.selected.clear();
			cx.notify();
		}
	}

	fn archive_selection_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
		let catalog = &self.workspace.archives;
		let confirming = catalog.confirming_selection();
		let unavailable = catalog.mutation.is_some();
		let all = !catalog.rows.is_empty() && catalog.selected.len() == catalog.rows.len();
		let check = archive_checkbox(
			"select-all-archives".into(),
			"Select all matching agents".into(),
			all,
			!all && !catalog.selected.is_empty(),
		)
		.on_click(cx.listener(|s, _, _, cx| s.select_all_archived(cx)))
		.on_key_down(cx.listener(|s, e: &KeyDownEvent, _, cx| {
			if !e.is_held && ["enter", "space"].contains(&e.keystroke.key.as_str()) {
				cx.stop_propagation();
				s.select_all_archived(cx);
			}
		}));
		let delete = archive_action(
			"delete-archive-selection",
			Symbol::Trash,
			"Delete selected agents and their subagents",
			catalog.confirmation.as_ref().filter(|_| confirming).map(|(_, at)| *at),
		)
		.aria_label(if confirming {
			"Confirm deletion of selected agents"
		} else {
			"Delete selected agents"
		})
		.when(confirming, |d| {
			d.text_color(ui_theme::rgb(ui_theme::ERROR))
				.bg(ui_theme::rgb(ui_theme::ERROR).opacity(0.12))
				.border_color(ui_theme::rgb(ui_theme::ERROR).opacity(0.35))
		})
		.on_click(cx.listener(|s, _, _, cx| s.delete_selected_archives(cx)))
		.on_key_down(cx.listener(|s, e: &KeyDownEvent, _, cx| {
			if !e.is_held && ["enter", "space"].contains(&e.keystroke.key.as_str()) {
				cx.stop_propagation();
				s.delete_selected_archives(cx);
			}
		}));
		let selected_count = catalog.selected.len();
		let clear =
			archive_action("clear-archive-selection", Symbol::Close, "Clear selection", None)
				.aria_label("Clear selection")
				.on_click(cx.listener(|s, _, _, cx| s.clear_archive_selection(cx)))
				.on_key_down(cx.listener(|s, e: &KeyDownEvent, _, cx| {
					if !e.is_held && ["enter", "space"].contains(&e.keystroke.key.as_str()) {
						cx.stop_propagation();
						s.clear_archive_selection(cx);
					}
				}));
		let selection = ui_theme::canvas_control_group()
			.id("archive-batch-bar")
			.debug_selector(|| "archive-batch-bar".into())
			.h(gpui::px(38.))
			.px(gpui::px(10.))
			.gap(gpui::px(10.))
			.text_color(ui_theme::rgb(ui_theme::TEXT))
			.child(
				check
					.w_auto()
					.gap(gpui::px(6.))
					.px(gpui::px(4.))
					.hover(|d| d.bg(ui_theme::rgba(ui_theme::HOVER_FILL)))
					.active(|d| d.bg(ui_theme::rgba(ui_theme::PRESSED_FILL)))
					.text_size(gpui::px(ui_theme::CONTROL_LABEL_SIZE))
					.child(if all { "Clear all" } else { "Select all" })
					.smooth()
					.enabled(!unavailable),
			)
			.child(
				gpui::div()
					.min_w(gpui::px(66.))
					.text_size(gpui::px(ui_theme::CONTROL_LABEL_SIZE))
					.text_color(ui_theme::rgb(ui_theme::TEXT))
					.font_features(ui_theme::tabular_figures())
					.child(format!(
						"{selected_count} {}",
						if unavailable { "deleting…" } else { "selected" }
					)),
			)
			.child(delete.smooth().enabled(!unavailable))
			.child(clear.smooth().enabled(!unavailable));
		ArchiveSelectionActions { visible: selected_count > 0, child: selection.into_any_element() }
	}

	fn archive_catalog_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
		let catalog = &self.workspace.archives;
		gpui::div()
			.id("archive-toolbar")
			.debug_selector(|| "archive-toolbar".into())
			.flex()
			.flex_wrap()
			.items_center()
			.gap(gpui::px(8.))
			.child(
				gpui::div()
					.id("archive-search")
					.debug_selector(|| "archive-search".into())
					.flex_1()
					.flex_basis(gpui::px(200.))
					.min_w_0()
					.h(gpui::px(ui_theme::CONTROL_SIZE))
					.children(catalog.search.clone()),
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
				match catalog.sort {
					Sort::Recent => 0,
					Sort::Oldest => 1,
					Sort::Name => 2,
				},
				cx.listener(|s, index: &usize, _, cx| {
					s.workspace.archives.sort = [Sort::Recent, Sort::Oldest, Sort::Name][*index];
					s.workspace.archives.cancel_confirmation();
					cx.notify();
				}),
			))
	}
}

#[derive(gpui::IntoElement)]
struct ArchiveSelectionActions {
	visible: bool,
	child: AnyElement,
}
impl gpui::RenderOnce for ArchiveSelectionActions {
	fn render(self, window: &mut gpui::Window, cx: &mut gpui::App) -> impl IntoElement {
		let progress =
			ui_motion::value("archive-selection-actions", f32::from(self.visible), window, cx);
		gpui::div()
			.id("archive-batch-footer")
			.debug_selector(|| "archive-batch-footer".into())
			.w_full()
			.h(gpui::px(48. * progress))
			.flex_none()
			.flex()
			.items_center()
			.justify_center()
			.overflow_hidden()
			.opacity(progress)
			.children(self.visible.then_some(self.child))
	}
}

#[derive(gpui::IntoElement)]
struct ArchiveRow {
	id: gpui::SharedString,
	active: bool,
	row: gpui::Stateful<gpui::Div>,
	checkbox: AnyElement,
	content: AnyElement,
}
impl gpui::RenderOnce for ArchiveRow {
	fn render(self, window: &mut gpui::Window, cx: &mut gpui::App) -> impl IntoElement {
		let state = window.use_keyed_state(self.id.clone(), cx, |_, cx| (false, cx.focus_handle()));
		let (hovered, focus) = {
			let (hovered, focus) = state.read(cx);
			(*hovered, focus.clone())
		};
		let visible = self.active || hovered || focus.contains_focused(window, cx);
		let selector = format!("{}-{}", self.id, if visible { "visible" } else { "hidden" });
		let progress = ui_motion::value(
			gpui::SharedString::from(format!("{}-reveal", self.id)),
			f32::from(visible),
			window,
			cx,
		);
		self.row
			.track_focus(&focus)
			.on_hover(move |hovered, _, cx| {
				state.update(cx, |s, cx| {
					s.0 = *hovered;
					cx.notify();
				});
			})
			.child(
				gpui::div()
					.id(self.id)
					.debug_selector(move || selector.clone())
					.size(gpui::px(ui_theme::CHROME_CONTROL_SIZE))
					.flex_none()
					.opacity(progress)
					.child(self.checkbox),
			)
			.child(self.content)
	}
}

fn archive_checkbox(
	id: String,
	label: String,
	checked: bool,
	mixed: bool,
) -> gpui::Stateful<gpui::Div> {
	gpui::div()
		.id(gpui::SharedString::from(id.clone()))
		.debug_selector({
			let id = id.clone();
			move || id.clone()
		})
		.role(Role::CheckBox)
		.aria_label(label)
		.tab_index(0)
		.aria_toggled(if mixed {
			gpui::Toggled::Mixed
		} else if checked {
			gpui::Toggled::True
		} else {
			gpui::Toggled::False
		})
		.flex_none()
		.size(gpui::px(ui_theme::CHROME_CONTROL_SIZE))
		.flex()
		.items_center()
		.justify_center()
		.cursor_pointer()
		.rounded(gpui::px(4.))
		.focus_visible(|d| d.bg(ui_theme::rgba(ui_theme::HOVER_FILL)))
		.child(ArchiveCheck { id, checked, mixed })
}

#[derive(gpui::IntoElement)]
struct ArchiveCheck {
	id: String,
	checked: bool,
	mixed: bool,
}
impl gpui::RenderOnce for ArchiveCheck {
	fn render(self, window: &mut gpui::Window, cx: &mut gpui::App) -> impl IntoElement {
		let progress = ui_motion::value(
			gpui::SharedString::from(format!("{}-check", self.id)),
			f32::from(self.checked || self.mixed),
			window,
			cx,
		);
		gpui::div()
			.size(gpui::px(14.))
			.rounded(gpui::px(3.))
			.border_1()
			.border_color(
				ui_theme::rgb(ui_theme::TEXT_MUTED)
					.blend(ui_theme::rgb(ui_theme::ACCENT).opacity(progress)),
			)
			.bg(ui_theme::rgb(ui_theme::ACCENT).opacity(0.18 * progress))
			.flex()
			.items_center()
			.justify_center()
			.child(gpui::div().opacity(progress).text_color(ui_theme::rgb(ui_theme::TEXT)).child(
				workspace_symbols::icon_sized(
					if self.mixed { Symbol::Minus } else { Symbol::Confirm },
					11.,
				),
			))
	}
}

fn archive_action(
	id: impl Into<gpui::SharedString>,
	symbol: Symbol,
	label: &'static str,
	confirmation_started: Option<Instant>,
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
		.rounded(gpui::px(4.))
		.border(gpui::px(0.5))
		.border_color(gpui::transparent_black())
		.bg(gpui::transparent_black())
		.text_color(ui_theme::rgb(ui_theme::TEXT))
		.cursor_pointer()
		.hover(|d| d.bg(ui_theme::rgba(ui_theme::HOVER_FILL)))
		.active(|d| d.bg(ui_theme::rgba(ui_theme::PRESSED_FILL)))
		.focus_visible(|d| {
			d.bg(ui_theme::rgba(ui_theme::HOVER_FILL)).border_color(ui_theme::rgb(ui_theme::ACCENT))
		})
		.tooltip(move |_, cx| cx.new(|_| ControlTooltip(label)).into())
		.child(ConfirmationNudge {
			started: confirmation_started,
			child: workspace_symbols::icon_sized(symbol, 16.),
		})
}

fn archive_timestamp(micros: i64) -> (String, String) {
	let seconds = (micros / 1_000_000) as libc::time_t;
	let mut local = std::mem::MaybeUninit::<libc::tm>::zeroed();
	// SAFETY: localtime_r writes a tm into valid storage for the supplied timestamp.
	let local = unsafe {
		libc::localtime_r(&seconds, local.as_mut_ptr());
		local.assume_init()
	};
	let month =
		["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"]
			[local.tm_mon as usize];
	(
		format!("{month} {}, {}", local.tm_mday, local.tm_year + 1900),
		format!("{:02}:{:02}", local.tm_hour, local.tm_min),
	)
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
	fn archive_bulk_delete_confirms_exact_filtered_selection_and_keeps_failures_selected(
		cx: &mut gpui::TestAppContext,
	) {
		cx.background_executor.allow_parking();
		let (view, visual) = cx.add_window_view(|_, cx| {
			let surface = cx.new(AgentSurface::new);
			cx.observe(&surface, |_, _, cx| cx.notify()).detach();
			surface.update(cx, |s, cx| {
				s.visual_workspace_fixture(cx);
				s.composer.update(cx, |i, cx| i.set_content("Keep Main draft", cx));
				s.open_archive_catalog(cx);
				s.workspace.archives.feedback = None;
				let mut data = s.snapshot.clone().unwrap();
				for (id, title, parent) in [
					("parent", "Remove parent", None),
					("child", "Remove child", Some("parent")),
					("keep", "Keep this one", None),
				] {
					let mut item = data.work_items[0].clone();
					item.id = id.into();
					item.title = title.into();
					item.parent_goal_id = parent.map(str::to_owned);
					item.codex_thread_id = Some(format!("native-{id}"));
					data.work_items.push(item);
					data.archived_work_ids.insert(id.into());
				}
				s.workspace.archives.data = Some(data);
			});
			ArchiveView(surface)
		});
		let surface = view.read_with(visual, |v, _| v.0.clone());
		let active = surface.read_with(visual, |s, _| s.snapshot.clone().unwrap());
		let expected = active.clone();
		let mut remaining =
			surface.read_with(visual, |s, _| s.workspace.archives.data.clone().unwrap());
		remaining.work_items.retain(|w| w.id != "parent");
		remaining.archived_work_ids.remove("parent");
		let (_root, profile, server) = wire_test_support::fixture(move |listener| async move {
			for (id, accepted) in [("child", false), ("parent", true)] {
				let mut socket = wire_test_support::accept(&listener).await;
				let Message::Text(text) = socket.next().await.unwrap().unwrap() else {
					panic!("command")
				};
				let ClientMessage::Command(command) = serde_json::from_str(&text).unwrap() else {
					panic!("delete")
				};
				assert!(
					matches!(&command.payload, CommandPayload::Agent {action} if matches!(&**action, AgentActionDto::DeleteArchivedThread {work_id,thread_id} if work_id.as_str()==id && thread_id.as_str()==format!("native-{id}")))
				);
				let receipt = ServerMessage::CommandReceipt(decodex_protocol::CommandReceipt {
					version: CURRENT_VERSION,
					server_id: ServerId::new(SERVER).unwrap(),
					client_command_id: command.client_command_id.clone(),
					idempotency_key: command.idempotency_key.clone(),
					disposition: decodex_protocol::ReceiptDisposition::Executed,
					original_client_command_id: command.client_command_id.clone(),
				});
				let result =
					ServerMessage::CommandResult(decodex_protocol::CommandResultEnvelope {
						version: CURRENT_VERSION,
						server_id: ServerId::new(SERVER).unwrap(),
						client_command_id: command.client_command_id,
						idempotency_key: command.idempotency_key,
						outcome: if accepted {
							decodex_protocol::CommandOutcome::Succeeded
						} else {
							decodex_protocol::CommandOutcome::Rejected
						},
						entity_revision: accepted.then_some(decodex_protocol::EntityRevision(0)),
						payload: accepted.then(|| decodex_protocol::ResultPayload::AgentAccepted {
							work_id: EntityId::new(id).unwrap(),
						}),
						error: (!accepted).then(|| {
							decodex_protocol::CommandError::ApplicationUnavailable {
								message: WireText::new("Still in use").unwrap(),
							}
						}),
					});
				for message in [receipt, result] {
					socket
						.send(Message::Text(serde_json::to_string(&message).unwrap().into()))
						.await
						.unwrap();
				}
			}
			for (kind, data) in [(0, remaining), (1, active)] {
				let mut socket = wire_test_support::accept(&listener).await;
				let Message::Text(text) = socket.next().await.unwrap().unwrap() else {
					panic!("query")
				};
				let ClientMessage::Query(query) = serde_json::from_str(&text).unwrap() else {
					panic!("refresh")
				};
				assert!(matches!(
					(kind, &query.payload),
					(0, QueryPayload::GetAgentArchiveCatalog) | (1, QueryPayload::GetAgentSnapshot)
				));
				wire_test_support::answer_query(
					&mut socket,
					query.query_id,
					QueryResultPayload::AgentSnapshot(AgentSnapshotResult::Available(data)),
				)
				.await;
			}
		});
		surface.update(visual, |s, _| s.profile = Some(profile));
		visual.update(crate::ui_frame::draw_current_frame);
		let sort_before = visual.debug_bounds("archive-sort-Name").unwrap();
		let search_before = visual.debug_bounds("archive-search").unwrap();
		let search = surface.read_with(visual, |s, _| s.workspace.archives.search.clone().unwrap());
		search.update(visual, |input, cx| input.set_content("Remove", cx));
		visual.update(crate::ui_frame::draw_current_frame);
		assert!(visual.debug_bounds("select-all-archives").is_none());
		let first = visual.debug_bounds("select-archive-parent").unwrap();
		visual.simulate_click(first.center(), Default::default());
		visual.update(crate::ui_frame::draw_current_frame);
		let child_row = visual.debug_bounds("archive-title-child").unwrap();
		visual.simulate_click(child_row.center(), Default::default());
		surface.read_with(visual, |s, _| {
			assert!(s.workspace.archives.viewed.is_none(), "selection must not open a chat");
			assert_eq!(s.workspace.archives.selected.len(), 2);
		});
		thread::sleep(Duration::from_millis(240));
		visual.update(crate::ui_frame::draw_current_frame);
		let all = visual.debug_bounds("select-all-archives").unwrap();
		visual.simulate_click(all.center(), Default::default());
		visual.update(crate::ui_frame::draw_current_frame);
		assert!(visual.debug_bounds("select-all-archives").is_none());
		let first = visual.debug_bounds("select-archive-parent").unwrap();
		visual.simulate_click(first.center(), Default::default());
		thread::sleep(Duration::from_millis(240));
		visual.update(crate::ui_frame::draw_current_frame);
		let all = visual.debug_bounds("select-all-archives").unwrap();
		visual.simulate_click(all.center(), Default::default());
		surface.read_with(visual, |s, _| {
			assert_eq!(
				s.workspace.archives.selected,
				BTreeSet::from(["child".into(), "parent".into()])
			)
		});
		visual.update(crate::ui_frame::draw_current_frame);
		assert_eq!(
			visual.debug_bounds("archive-sort-Name").unwrap(),
			sort_before,
			"selection must not move the toolbar"
		);
		assert_eq!(visual.debug_bounds("archive-search").unwrap(), search_before);
		let footer = visual.debug_bounds("archive-batch-footer").unwrap();
		let viewport = visual.debug_bounds("archive-viewport").unwrap();
		assert!(footer.top() >= viewport.bottom(), "batch controls do not cover the list");
		let delete = visual.debug_bounds("delete-archive-selection").unwrap();
		visual.simulate_click(delete.center(), Default::default());
		surface.read_with(visual, |s, _| {
			assert!(s.workspace.archives.confirming_selection());
			assert!(s.workspace.archives.mutation.is_none());
		});
		visual.update(crate::ui_frame::draw_current_frame);
		let child = visual.debug_bounds("select-archive-child").unwrap();
		visual.simulate_click(child.center(), Default::default());
		surface.read_with(visual, |s, _| {
			assert!(s.workspace.archives.confirmation.is_none());
			assert!(s.workspace.archives.viewed.is_none(), "checking a row must not open it");
		});
		visual.update(crate::ui_frame::draw_current_frame);
		visual.simulate_click(child.center(), Default::default());
		visual.update(crate::ui_frame::draw_current_frame);
		visual.simulate_click(delete.center(), Default::default());
		visual.update(crate::ui_frame::draw_current_frame);
		visual.simulate_click(delete.center(), Default::default());
		let until = Instant::now() + Duration::from_secs(5);
		while surface.read_with(visual, |s, _| s.workspace.archives.mutation.is_some()) {
			visual.run_until_parked();
			assert!(Instant::now() < until, "batch timed out");
			thread::sleep(Duration::from_millis(5));
		}
		server.join().unwrap();
		visual.update(crate::ui_frame::draw_current_frame);
		surface.read_with(visual, |s, cx| {
			assert_eq!(s.workspace.archives.selected, BTreeSet::from(["child".into()]));
			assert!(s.workspace.archives.feedback.as_ref().unwrap().starts_with("1 not deleted"));
			assert_eq!(s.snapshot.as_ref(), Some(&expected));
			assert_eq!(s.composer.read(cx).content(), "Keep Main draft");
			assert!(s.workspace.archives.archived_work("keep").is_some());
		});
		assert!(visual.debug_bounds("archived-parent").is_none());
	}

	#[gpui::test]
	fn archive_layout_adapts_without_hiding_or_separating_actions(cx: &mut gpui::TestAppContext) {
		let (_view, visual) = cx.add_window_view(|_, cx| {
			let surface = cx.new(AgentSurface::new);
			surface.update(cx, |s, cx| {
				s.visual_workspace_fixture(cx);
				let mut data = s.snapshot.clone().unwrap();
				let mut item = data.work_items[0].clone();
				item.id = "layout".into();
				item.title =
					"Review the release plan and check the remaining acceptance work".into();
				data.work_items.push(item);
				data.archived_work_ids.insert("layout".into());
				data.projects = vec![decodex_protocol::ProjectDto {
					id: "books".into(),
					name: "Reading corner".into(),
					native_project_id: None,
					lead_agent_id: None,
					directory: "/tmp/books".into(),
					agent_ids: vec!["layout".into()],
				}];
				s.workspace.archives.data = Some(data);
			});
			ArchiveView(surface)
		});
		for width in [1600., 900., 500., 360.] {
			visual.simulate_resize(gpui::size(gpui::px(width), gpui::px(720.)));
			visual.update(crate::ui_frame::draw_current_frame);
			let body = visual.debug_bounds("archive-browser-content").unwrap();
			let search = visual.debug_bounds("archive-search").unwrap();
			let sort = visual.debug_bounds("archive-sort").unwrap();
			assert_eq!(search.size.height, sort.size.height, "matching control heights");
			if width >= 500. {
				assert_eq!(search.center().y, sort.center().y, "aligned search and sorting");
			}
			let row = visual.debug_bounds("archived-layout").unwrap();
			let heading = visual.debug_bounds("archive-date-layout").unwrap();
			let title = visual.debug_bounds("archive-title-layout").unwrap();
			let project = visual.debug_bounds("archive-project-layout").unwrap();
			let actions = visual.debug_bounds("archive-actions-layout").unwrap();
			let restore = visual.debug_bounds("unarchive-layout").unwrap();
			let delete = visual.debug_bounds("delete-archive-layout").unwrap();
			assert!(body.size.width <= gpui::px(960.), "bounded reading column at {width}");
			assert!(row.left() >= body.left() && row.right() <= body.right());
			assert!(title.right() < actions.left(), "text does not cross actions at {width}");
			assert!(project.bottom() <= row.bottom(), "metadata remains inside the row at {width}");
			assert!(restore.left() >= actions.left() && delete.right() <= actions.right());
			assert!(delete.left() - restore.right() <= gpui::px(4.));
			assert!(heading.bottom() <= row.top(), "date labels stay above their group");
			assert!(title.left() - body.left() <= gpui::px(54.), "no separate left date column");
			if width == 1600. {
				assert!((body.left() - gpui::px((width - 960.) / 2.)).abs() < gpui::px(1.));
			}
			if width == 360. {
				assert!(project.top() >= title.bottom(), "narrow rows wrap project metadata");
			}
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
	fn archive_rows_select_and_open_only_from_the_explicit_button(cx: &mut gpui::TestAppContext) {
		let (view, visual) = cx.add_window_view(|_, cx| {
			let surface = cx.new(AgentSurface::new);
			cx.observe(&surface, |_, _, cx| cx.notify()).detach();
			surface.update(cx, |s, cx| {
				s.visual_workspace_fixture(cx);
				s.composer.update(cx, |i, cx| i.set_content("Keep the draft", cx));
				let mut data = s.snapshot.clone().unwrap();
				for (id, title, time) in
					[("z", "Zeta", 3), ("a", "Alpha", 2), ("b", "Beta", -86_400_000_000)]
				{
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
		assert!(visual.debug_bounds("open-archive-z").is_some());
		assert!(visual.debug_bounds("archive-date-z").is_some());
		assert!(visual.debug_bounds("archive-date-a").is_none());
		assert!(visual.debug_bounds("archive-date-b").is_some());
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
		assert!(visual.debug_bounds("archive-date-z").is_none());
		assert!(visual.debug_bounds("archive-date-b").is_none());
		surface.read_with(visual, |s, _| {
			assert!(s.workspace.archives.confirmation.is_none());
			assert_eq!(
				s.workspace.archives.rows.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
				vec!["a", "b", "z"]
			);
		});
		let title = visual.debug_bounds("archive-title-a").unwrap();
		visual.simulate_mouse_move(title.center(), None, Default::default());
		visual.update(crate::ui_frame::draw_current_frame);
		assert!(
			visual.debug_bounds("archive-select-a-visible").is_some(),
			"hovering the title reveals its row checkbox"
		);
		visual.simulate_click(title.center(), Default::default());
		surface.read_with(visual, |s, _| {
			assert!(s.workspace.archives.selected.contains("a"));
			assert!(s.workspace.archives.viewed.is_none(), "row activation only selects");
		});
		visual.update(crate::ui_frame::draw_current_frame);
		let open = visual.debug_bounds("open-archive-a").unwrap();
		visual.simulate_click(open.center(), Default::default());
		surface.read_with(visual, |s, _| {
			assert_eq!(s.workspace.archives.viewed.as_deref(), Some("a"));
			assert!(
				s.workspace.archives.selected.contains("a"),
				"opening does not toggle selection"
			);
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
		let body = visual.debug_bounds("archive-browser-content").unwrap();
		assert!(
			bar.bottom() <= body.bottom(),
			"scroll viewport must fit below the toolbar: {bar:?} {body:?}"
		);
		// Keep enough compact rows to exercise scrolling, rather than grabbing a large thumb.
		assert!(visual.debug_bounds("archived-scroll-99").unwrap().bottom() > bar.bottom());
		visual.simulate_click(bar.center(), Default::default());
		visual.update(crate::ui_frame::draw_current_frame);
		let after = visual.debug_bounds("archived-scroll-00").unwrap();
		assert!(
			after.top() < before.top(),
			"scrollbar must move overflowing archive rows: bar={bar:?}, before={before:?}, after={after:?}"
		);
		visual.simulate_click(
			gpui::point(bar.center().x, bar.bottom() - gpui::px(1.)),
			Default::default(),
		);
		visual.update(crate::ui_frame::draw_current_frame);
		let last = visual.debug_bounds("archived-scroll-99").unwrap();
		assert!(
			last.bottom() <= body.bottom(),
			"last row must be fully visible at the end: {last:?} {body:?}"
		);
		visual.simulate_click(last.center(), Default::default());
		thread::sleep(Duration::from_millis(240));
		visual.update(crate::ui_frame::draw_current_frame);
		let viewport = visual.debug_bounds("archive-viewport").unwrap();
		let footer = visual.debug_bounds("archive-batch-footer").unwrap();
		assert!(footer.top() >= viewport.bottom(), "selection bar has its own space");
		let bar = visual.debug_bounds("archive-scrollbar").unwrap();
		visual.simulate_click(
			gpui::point(bar.center().x, bar.bottom() - gpui::px(1.)),
			Default::default(),
		);
		visual.update(crate::ui_frame::draw_current_frame);
		let last = visual.debug_bounds("archived-scroll-99").unwrap();
		assert!(last.bottom() <= viewport.bottom(), "batch actions must not cover the last row");
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
