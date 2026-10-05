//! Work discovery is separate from the list of open conversations.
use super::{
	AgentSurface, ComposerInput, Context, Entity, InteractiveElement, IntoElement, ParentElement,
	Role, SharedString, StatefulInteractiveElement, Styled,
};
use crate::{
	shell::workspace_symbols::{self, Symbol},
	ui_theme::{HOVER_FILL, TEXT_MUTED},
};
use gpui::{AnyElement, AppContext as _, Focusable, KeyDownEvent, prelude::FluentBuilder};

struct WorkRow {
	id: String,
	title: String,
	workspace: String,
	status: String,
	color: u32,
	native: Option<(String, String)>,
}

impl AgentSurface {
	pub(super) fn new_work_search(cx: &mut Context<Self>) -> Entity<ComposerInput> {
		let input =
			cx.new(|cx| ComposerInput::with_placeholder(46, "Search work…", "Search all work", cx));
		cx.observe(&input, |_, _, cx| cx.notify()).detach();
		input
	}

	pub(super) fn is_new_conversation(&self) -> bool {
		self.workspace.new_conversation.is_some()
			&& self.workspace.new_conversation == self.selected
	}

	pub(super) fn new_work_conversation(&mut self, cx: &mut Context<Self>) {
		if self.sending || self.uncertain {
			return;
		}
		if self.workspace.new_conversation.is_none() {
			self.workspace.new_conversation_workspace = self.workspace.workspace_filter.clone();
		}
		self.workspace.browsing = false;
		if self.root_id().is_none() {
			cx.notify();
			return;
		}
		self.close_native_agent(cx);
		self.stop_voice(cx);
		let id = self
			.workspace
			.new_conversation
			.get_or_insert_with(|| format!("conversation-{}", super::unique_command()))
			.clone();
		self.restore_manager_composer(&id, cx);
		self.selected = Some(id);
		self.composer.update(cx, |input, cx| {
			input.set_placeholder("Describe your goal, or explore an idea…", cx)
		});
		self.history = None;
		self.timeline.native.reset();
		self.timeline.marks.clear();
		self.feedback.clear();
		self.workspace.details_visible = false;
		cx.notify();
	}

	fn add_workspace_folder(&mut self, cx: &mut Context<Self>) {
		let Some(profile) = self.profile.clone() else { return };
		self.workspace.folder_error = None;
		let selected = cx.prompt_for_paths(gpui::PathPromptOptions {
			files: false,
			directories: true,
			multiple: false,
			prompt: Some("Add workspace".into()),
		});
		cx.spawn(async move |surface, cx| {
			let Ok(Ok(Some(paths))) = selected.await else { return };
			let Some(path) = paths.into_iter().next() else { return };
			let request_profile = profile.clone();
			let result = cx
				.background_executor()
				.spawn(async move {
					let directory =
						decodex_protocol::WireText::new(path.to_string_lossy().into_owned())
							.map_err(|_| "Folder path is too long".to_owned())?;
					let action = super::AgentActionDto::AddWorkspace {
						workspace_id: decodex_protocol::EntityId::new(format!(
							"workspace-{}",
							super::unique_command()
						))
						.unwrap(),
						directory,
					};
					let runtime = tokio::runtime::Builder::new_current_thread()
						.enable_all()
						.build()
						.map_err(|e| e.to_string())?;
					runtime
						.block_on(decodex_protocol::AgentClient::new(request_profile).execute(
							action,
							decodex_protocol::IdempotencyKey::new(super::unique_command()).unwrap(),
						))
						.map_err(|e| format!("Could not add folder: {e:?}"))
				})
				.await;
			let _ = surface.update(cx, |s, cx| {
				if s.profile.as_ref() != Some(&profile) {
					return;
				}
				match result {
					Ok(decodex_protocol::AgentCommandResponse::Accepted { work_id }) => {
						let id = work_id.as_str().to_owned();
						if s.is_new_conversation() {
							s.workspace.new_conversation_workspace = Some(id.clone());
							s.save_draft_document(cx);
						}
						s.workspace.workspace_filter = Some(id);
						s.refresh(cx);
					},
					_ => {
						s.workspace.folder_error = Some(
							"Could not add this folder. Check that it is available and try again."
								.into(),
						);
						cx.notify();
					},
				}
			});
		})
		.detach();
	}

	pub(super) fn workspace_choices(&self, draft: bool, cx: &mut Context<Self>) -> AnyElement {
		let selected = if draft {
			&self.workspace.new_conversation_workspace
		} else {
			&self.workspace.workspace_filter
		};
		let fallback = if draft { "No workspace" } else { "All workspaces" };
		let workspaces = self.snapshot.as_ref().map(|s| s.workspaces.as_slice()).unwrap_or(&[]);
		let name = workspaces
			.iter()
			.find(|w| Some(&w.id) == selected.as_ref())
			.map(|w| w.name.as_str())
			.unwrap_or(fallback)
			.to_owned();
		let open = self.workspace.workspace_picker == Some(draft);
		let trigger_bounds =
			std::rc::Rc::new(std::cell::Cell::new(None::<gpui::Bounds<gpui::Pixels>>));
		let measured_bounds = trigger_bounds.clone();
		let trigger = gpui::div()
			.id("workspace-picker-trigger")
			.relative()
			.child(
				gpui::canvas(
					move |bounds, _, _| measured_bounds.set(Some(bounds)),
					|_, _, _, _| {},
				)
				.absolute()
				.inset_0(),
			)
			.role(Role::Button)
			.aria_label(format!("Choose workspace: {name}"))
			.tab_index(0)
			.w(gpui::px(176.))
			.when(draft, |d| d.w_auto())
			.h(gpui::px(crate::ui_theme::CONTROL_SIZE))
			.px_3()
			.flex()
			.items_center()
			.gap_2()
			.rounded(gpui::px(7.))
			.text_size(gpui::px(crate::ui_theme::BODY_SIZE))
			.cursor_pointer()
			.bg(gpui::rgba(if draft { 0x00000000 } else { 0xffffff08 }))
			.hover(|s| s.bg(gpui::rgba(HOVER_FILL)))
			.child(
				gpui::div()
					.when(!draft, |d| d.flex_1())
					.min_w_0()
					.max_w(gpui::px(160.))
					.text_ellipsis()
					.child(name),
			)
			.child(workspace_symbols::icon_sized(Symbol::ChevronDown, 12.))
			.on_click(
				cx.listener(move |s, _, window, cx| s.toggle_workspace_picker(draft, window, cx)),
			)
			.on_key_down(cx.listener(move |s, e: &KeyDownEvent, window, cx| {
				if !e.is_held && ["enter", "space"].contains(&e.keystroke.key.as_str()) {
					s.toggle_workspace_picker(draft, window, cx);
					cx.stop_propagation();
				}
			}));
		let mut anchor = gpui::div()
			.id("workspace-picker")
			.relative()
			.w(gpui::px(176.))
			.when(draft, |d| d.w_auto())
			.child(trigger);
		if open {
			let search = self.workspace.workspace_search.as_ref().unwrap();
			let query = search.read(cx).content().trim().to_lowercase();
			let mut choices = vec![(None, fallback.to_owned(), String::new())];
			choices.extend(
				workspaces
					.iter()
					.filter(|w| {
						w.name.to_lowercase().contains(&query)
							|| w.directory.to_lowercase().contains(&query)
					})
					.map(|w| (Some(w.id.clone()), w.name.clone(), w.directory.clone())),
			);
			let no_matches = choices.len() == 1 && !query.is_empty();
			let mut list = gpui::div()
				.id("workspace-picker-list")
				.max_h(gpui::px(260.))
				.overflow_y_scroll()
				.flex()
				.flex_col();
			for (id, name, path) in choices {
				let active = &id == selected;
				let keyboard_id = id.clone();
				list = list.child(
					gpui::div()
						.id(SharedString::from(format!(
							"workspace-choice-{}",
							id.as_deref().unwrap_or("all")
						)))
						.role(Role::Button)
						.aria_label(format!("Workspace: {name}"))
						.tab_index(0)
						.flex_none()
						.px_2()
						.py_1()
						.flex()
						.items_center()
						.gap_2()
						.rounded(gpui::px(6.))
						.cursor_pointer()
						.hover(|s| s.bg(gpui::rgba(HOVER_FILL)))
						.child(gpui::div().w(gpui::px(16.)).flex_none().children(
							active.then(|| workspace_symbols::icon_sized(Symbol::Confirm, 13.)),
						))
						.child(
							gpui::div()
								.min_w_0()
								.flex_1()
								.flex()
								.flex_col()
								.gap_1()
								.child(
									gpui::div()
										.text_size(gpui::px(crate::ui_theme::BODY_SIZE))
										.text_ellipsis()
										.child(name),
								)
								.children((!path.is_empty()).then(|| {
									gpui::div()
										.text_size(gpui::px(11.))
										.text_color(gpui::rgb(TEXT_MUTED))
										.text_ellipsis()
										.child(path)
								})),
						)
						.on_click(
							cx.listener(move |s, _, _, cx| {
								s.choose_workspace(draft, id.clone(), cx)
							}),
						)
						.on_key_down(cx.listener(move |s, e: &KeyDownEvent, _, cx| {
							if !e.is_held && ["enter", "space"].contains(&e.keystroke.key.as_str())
							{
								s.choose_workspace(draft, keyboard_id.clone(), cx);
								cx.stop_propagation();
							}
						})),
				);
			}
			let menu = gpui::div()
				.id("workspace-picker-menu")
				.occlude()
				.on_mouse_down_out(cx.listener(move |s, event: &gpui::MouseDownEvent, _, cx| {
					if !trigger_bounds.get().is_some_and(|b| b.contains(&event.position)) {
						s.workspace.workspace_picker = None;
						cx.notify();
					}
				}))
				.p_2()
				.flex()
				.flex_col()
				.gap_1()
				.on_key_down(cx.listener(|s, e: &KeyDownEvent, _, cx| {
					if e.keystroke.key == "escape" {
						s.workspace.workspace_picker = None;
						cx.stop_propagation();
						cx.notify();
					}
				}))
				.child(gpui::div().h(gpui::px(crate::ui_theme::CONTROL_SIZE)).child(search.clone()))
				.child(list)
				.children(no_matches.then(|| {
					gpui::div()
						.px_2()
						.py_1()
						.text_size(gpui::px(12.))
						.text_color(gpui::rgb(TEXT_MUTED))
						.child("No matching workspaces")
				}))
				.child(
					gpui::div()
						.id("add-workspace-folder")
						.role(Role::Button)
						.aria_label("Add workspace folder")
						.tab_index(0)
						.px_2()
						.py_1()
						.rounded(gpui::px(6.))
						.cursor_pointer()
						.text_size(gpui::px(crate::ui_theme::BODY_SIZE))
						.hover(|s| s.bg(gpui::rgba(HOVER_FILL)))
						.child("+ Add folder")
						.on_click(cx.listener(|s, _, _, cx| {
							s.workspace.workspace_picker = None;
							s.add_workspace_folder(cx);
							cx.notify();
						}))
						.on_key_down(cx.listener(|s, e: &KeyDownEvent, _, cx| {
							if !e.is_held && ["enter", "space"].contains(&e.keystroke.key.as_str())
							{
								s.workspace.workspace_picker = None;
								s.add_workspace_folder(cx);
								cx.stop_propagation();
								cx.notify();
							}
						})),
				);
			anchor = anchor.child(
				gpui::deferred(
					gpui::div()
						.absolute()
						.top(gpui::px(32.))
						.left_0()
						.when(draft, |d| d.left(gpui::relative(0.5)).ml(gpui::px(-140.)))
						.w(gpui::px(280.))
						.child(crate::ui_motion::popover(true, menu)),
				)
				.priority(3),
			);
		}
		gpui::div()
			.flex()
			.flex_col()
			.gap_2()
			.child(anchor)
			.children(self.workspace.folder_error.as_ref().map(|error| {
				gpui::div()
					.text_size(gpui::px(12.))
					.text_color(gpui::rgb(crate::ui_theme::ERROR))
					.child(error.clone())
			}))
			.into_any_element()
	}

	fn toggle_workspace_picker(
		&mut self,
		draft: bool,
		window: &mut gpui::Window,
		cx: &mut Context<Self>,
	) {
		if self.workspace.workspace_picker == Some(draft) {
			self.workspace.workspace_picker = None;
		} else {
			let input = cx.new(|cx| {
				ComposerInput::with_placeholder(47, "Search folders…", "Search workspaces", cx)
			});
			cx.observe(&input, |_, _, cx| cx.notify()).detach();
			window.focus(&input.read(cx).focus_handle(cx), cx);
			self.workspace.workspace_search = Some(input);
			self.workspace.workspace_picker = Some(draft);
		}
		cx.notify();
	}

	fn choose_workspace(&mut self, draft: bool, id: Option<String>, cx: &mut Context<Self>) {
		if draft {
			self.workspace.new_conversation_workspace = id;
			self.save_draft_document(cx);
		} else {
			self.workspace.workspace_filter = id;
		}
		self.workspace.workspace_picker = None;
		cx.notify();
	}

	fn open_work_preview(
		&mut self,
		id: &str,
		native: Option<&(String, String)>,
		cx: &mut Context<Self>,
	) {
		self.keep_edited_preview(cx);
		let existing = self.workspace.pages.contains(&id.to_owned())
			&& !self.workspace.closing_pages.contains(id);
		let before = self.workspace.pages.clone();
		if let Some((owner, thread)) = native {
			self.open_native_agent(owner, thread, cx);
		} else {
			self.open_page(id, cx);
		}
		let Some(page) = self.conversation_page() else { return };
		if page != id {
			return;
		}
		if existing && self.workspace.preview_page.as_deref() != Some(id) {
			return;
		}
		if let Some(previous) = self.workspace.preview_page.take() {
			if previous != page {
				self.workspace.pages.retain(|p| p != &previous);
				self.workspace.closing_pages.remove(&previous);
			}
		}
		// Opening a native child also selects its owner internally; that is not a second tab.
		if let Some((owner, _)) = native {
			if !before.contains(owner) {
				self.workspace.pages.retain(|p| p != owner);
			}
		}
		if self.root_id().as_deref() != Some(&page) {
			self.workspace.preview_page = Some(page);
		}
		cx.notify();
	}

	pub(super) fn keep_edited_preview(&mut self, cx: &Context<Self>) {
		if self.workspace.preview_page.is_some()
			&& self.workspace.preview_page == self.conversation_page()
			&& (!self.composer.read(cx).content().is_empty()
				|| !self.attachments.is_empty()
				|| !self.task_references.is_empty())
		{
			self.workspace.preview_page = None;
		}
	}

	pub(super) fn work_navigation(
		&self,
		id: &'static str,
		label: &'static str,
		symbol: Symbol,
		action: impl Fn(&mut Self, &mut Context<Self>) + 'static,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let action = std::rc::Rc::new(action);
		let keyboard = action.clone();
		gpui::div()
			.id(id)
			.role(Role::Button)
			.aria_label(label)
			.tab_index(0)
			.h(gpui::px(crate::ui_theme::CONVERSATION_TAB_SIZE))
			.mb(gpui::px(crate::ui_theme::CONVERSATION_TAB_GAP))
			.w_full()
			.flex_none()
			.overflow_hidden()
			.flex()
			.items_center()
			.gap(gpui::px(6.))
			.rounded(gpui::px(7.))
			.cursor_pointer()
			.hover(|s| s.bg(gpui::rgba(HOVER_FILL)))
			.child(
				gpui::div()
					.w(gpui::px(crate::ui_theme::CONVERSATION_TAB_SIZE))
					.h_full()
					.flex_none()
					.flex()
					.items_center()
					.justify_center()
					.child(workspace_symbols::icon_sized(symbol, 15.)),
			)
			.child(
				gpui::div()
					.min_w(gpui::px(120.))
					.whitespace_nowrap()
					.text_color(gpui::rgb(TEXT_MUTED))
					.child(label),
			)
			.on_click(cx.listener(move |s, _, _, cx| action(s, cx)))
			.on_key_down(cx.listener(move |s, e: &KeyDownEvent, _, cx| {
				if !e.is_held && ["enter", "space"].contains(&e.keystroke.key.as_str()) {
					keyboard(s, cx);
					cx.stop_propagation();
				}
			}))
			.into_any_element()
	}

	fn browsable_work(&self, query: &str, selected_workspace: Option<&str>) -> Vec<WorkRow> {
		let Some(snapshot) = &self.snapshot else {
			return Vec::new();
		};
		let mut rows = Vec::new();
		let mut seen = std::collections::BTreeSet::new();
		let mut work = snapshot.work_items.iter().collect::<Vec<_>>();
		work.sort_by_key(|w| std::cmp::Reverse(w.updated_at_micros));
		let workspace_for = |id: &str| {
			snapshot.workspaces.iter().find(|p| p.work_ids.iter().any(|work| work == id))
		};
		let matches = |title: &str, workspace: Option<&decodex_protocol::WorkspaceDto>| {
			!selected_workspace.is_some_and(|id| workspace.is_none_or(|p| p.id != id))
				&& (query.is_empty()
					|| format!("{title} {}", workspace.map(|p| p.name.as_str()).unwrap_or(""))
						.to_lowercase()
						.contains(query))
		};
		for item in work {
			if let Some(thread) = &item.codex_thread_id {
				seen.insert(thread.clone());
			}
			let workspace = workspace_for(&item.id);
			let title = self.work_label(item);
			if !matches(&title, workspace) {
				continue;
			}
			let (status, color) = super::graph::state_in(snapshot, item);
			rows.push(WorkRow {
				id: item.id.clone(),
				title,
				workspace: workspace.map(|p| p.name.clone()).unwrap_or_default(),
				status: status.to_owned(),
				color,
				native: None,
			});
		}
		for (owner, agents) in &self.native_agents.lists {
			for agent in agents {
				if !seen.insert(agent.thread_id.clone()) {
					continue;
				}
				let workspace = workspace_for(owner);
				if !matches(&agent.title, workspace) {
					continue;
				}
				rows.push(WorkRow {
					id: format!("native:{owner}:{}", agent.thread_id),
					title: agent.title.clone(),
					workspace: workspace.map(|p| p.name.clone()).unwrap_or_default(),
					status: agent.status.clone(),
					color: TEXT_MUTED,
					native: Some((owner.clone(), agent.thread_id.clone())),
				});
			}
		}
		rows
	}

	pub(super) fn render_work_browser(&self, cx: &mut Context<Self>) -> AnyElement {
		let query = self.work_search.read(cx).content().trim().to_lowercase();
		let selected_workspace = self.workspace.workspace_filter.as_deref();
		let filters = self.workspace_choices(false, cx);
		let mut list = gpui::div()
			.id("all-work-list")
			.flex_1()
			.min_h_0()
			.overflow_y_scroll()
			.flex()
			.flex_col();
		let mut count = 0;
		for WorkRow { id, title, workspace: workspace_name, status, color, native } in
			self.browsable_work(&query, selected_workspace)
		{
			let keyboard = id.clone();
			let native_keyboard = native.clone();
			count += 1;
			list = list.child(
				gpui::div()
					.id(SharedString::from(format!("browse-{id}")))
					.role(Role::Button)
					.aria_label(format!("Open {title}"))
					.tab_index(0)
					.w_full()
					.min_w_0()
					.h(gpui::px(36.))
					.flex_none()
					.px_3()
					.flex()
					.items_center()
					.gap_3()
					.rounded(gpui::px(7.))
					.cursor_pointer()
					.hover(|s| s.bg(gpui::rgba(HOVER_FILL)))
					.child(
						gpui::div()
							.flex_1()
							.min_w_0()
							.overflow_hidden()
							.text_ellipsis()
							.child(title),
					)
					.child(
						gpui::div()
							.w(gpui::px(120.))
							.overflow_hidden()
							.text_ellipsis()
							.text_size(gpui::px(11.))
							.text_color(gpui::rgb(TEXT_MUTED))
							.child(workspace_name.to_owned()),
					)
					.child(
						gpui::div()
							.w(gpui::px(110.))
							.text_size(gpui::px(11.))
							.text_color(gpui::rgb(color))
							.child(status),
					)
					.on_click(cx.listener(move |s, _, _, cx| {
						s.open_work_preview(&id, native.as_ref(), cx);
					}))
					.on_key_down(cx.listener(move |s, e: &KeyDownEvent, _, cx| {
						if !e.is_held && ["enter", "space"].contains(&e.keystroke.key.as_str()) {
							s.open_work_preview(&keyboard, native_keyboard.as_ref(), cx);
							cx.stop_propagation();
						}
					})),
			);
		}
		if count == 0 {
			list = list.child(
				gpui::div().p_4().text_color(gpui::rgb(TEXT_MUTED)).child("No matching work."),
			);
		}
		gpui::div()
			.id("all-work-browser")
			.debug_selector(|| "all-work-browser".into())
			.flex_1()
			.min_h_0()
			.min_w_0()
			.flex()
			.flex_col()
			.p_4()
			.gap_3()
			.child(
				gpui::div()
					.flex()
					.items_center()
					.gap_2()
					.flex_none()
					.child(
						gpui::div()
							.w(gpui::px(260.))
							.max_w_full()
							.h(gpui::px(crate::ui_theme::CONTROL_SIZE))
							.child(self.work_search.clone()),
					)
					.child(filters),
			)
			.child(list)
			.into_any_element()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	#[gpui::test]
	fn browsing_reuses_preview_but_preserves_edited_conversations(cx: &mut gpui::TestAppContext) {
		let surface = cx.new(AgentSurface::new);
		surface.update(cx, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.workspace.pages.clear();
			s.open_work_preview("verify", None, cx);
			assert_eq!(s.workspace.preview_page.as_deref(), Some("verify"));
			s.open_work_preview("release", None, cx);
			assert!(!s.workspace.pages.iter().any(|id| id == "verify"));
			assert_eq!(s.workspace.preview_page.as_deref(), Some("release"));
			s.composer.update(cx, |input, cx| input.set_content("Keep my draft", cx));
			s.open_work_preview("verify", None, cx);
			assert!(s.workspace.pages.iter().any(|id| id == "release"));
			s.open_page("release", cx);
			assert_eq!(s.composer.read(cx).content(), "Keep my draft");
		});
	}

	#[gpui::test]
	fn new_conversation_is_local_and_keeps_separate_unsent_drafts(cx: &mut gpui::TestAppContext) {
		let surface = cx.new(AgentSurface::new);
		surface.update(cx, |s, cx| {
            s.visual_workspace_fixture(cx);
            s.open_page("agent", cx);
            s.composer.update(cx, |input, cx| input.set_content("Existing draft", cx));
            let count = s.snapshot.as_ref().unwrap().work_items.len();
            // Startup can restore the active owner before its root snapshot arrives.
            s.workspace.pages.push("agent".into());
            assert_eq!(s.conversation_pages().iter().filter(|(id,_,_)| id == "agent").count(), 1);
            s.new_work_conversation(cx);
            assert!(s.is_new_conversation());
            assert!(!s.sending);
            assert!(s.submission.waiting.is_none());
            assert_eq!(s.snapshot.as_ref().unwrap().work_items.len(), count);
            assert!(!s.conversation_pages().iter().any(|(id,_,_)| Some(id) == s.selected.as_ref()));
            s.composer.update(cx, |input, cx| input.set_content("New draft", cx));
            let action = s.build_submission("New draft", cx).unwrap();
            assert!(matches!(action, super::super::AgentActionDto::NewConversation { text, .. } if text.as_str() == "New draft"));
            s.open_page("agent", cx);
            assert_eq!(s.composer.read(cx).content(), "Existing draft");
            s.new_work_conversation(cx);
            assert_eq!(s.composer.read(cx).content(), "New draft");
            assert_eq!(s.snapshot.as_ref().unwrap().work_items.len(), count);
        });
	}
	#[gpui::test]
	fn browser_finds_closed_work_and_opening_it_preserves_other_drafts(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.open_page("agent", cx);
			s.composer.update(cx, |input, cx| input.set_content("Keep this draft", cx));
			s.workspace.pages.clear();
			s.workspace.browsing = true;
			let child = decodex_protocol::NativeAgentDto {
				thread_id: "child-thread".into(),
				parent_thread_id: "parent".into(),
				title: "Native review".into(),
				status: "completed".into(),
			};
			s.native_agents.lists.insert("agent".into(), vec![child.clone()]);
			s.native_agents.lists.insert("release".into(), vec![child]);
			assert_eq!(s.browsable_work("native review", None).len(), 1);
			let rows = s.browsable_work("", None);
			assert!(rows.iter().any(|r| r.id == "verify"));
			assert!(s.browsable_work("unlikely-no-matching-title", None).is_empty());
			let title = rows.iter().find(|r| r.id == "verify").unwrap().title.to_lowercase();
			assert!(s.browsable_work(&title, None).iter().any(|r| r.id == "verify"));
			s.open_page("verify", cx);
			assert!(!s.workspace.browsing);
			assert!(s.workspace.pages.iter().any(|id| id == "verify"));
			s.open_page("agent", cx);
			assert_eq!(s.composer.read(cx).content(), "Keep this draft");
		});
	}
}
