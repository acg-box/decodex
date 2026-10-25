//! One selection and clipboard range for a rendered message.
#[cfg(all(target_os = "macos", not(test)))]
#[path = "agent_selection_menu.rs"]
mod menu;
#[path = "agent_selection_source.rs"] mod source;

use std::{
	cell::{OnceCell, RefCell},
	ops::Range,
	rc::Rc,
};

use gpui::{
	AnyElement, App, DispatchPhase, Div, Entity, IntoElement, KeyDownEvent, MouseButton,
	MouseDownEvent, MouseMoveEvent, Pixels, Point, RenderOnce, TextLayout,
};

use crate::shell::agent_surface::selectable_text::{
	ClipboardItem, FocusHandle, InteractiveElement, ParentElement, SelectableText, SharedString,
	Styled, TextBlock, Window,
};

#[derive(Default)]
pub(in crate::shell::agent_surface) struct Document {
	text: String,
	blocks: Vec<usize>,
	separator: Option<&'static str>,
	scope: Rc<OnceCell<Scope>>,
	source: source::Source,
}
impl Document {
	pub(in crate::shell::agent_surface) fn push(
		&mut self,
		text: SelectableText,
	) -> impl IntoElement + use<> {
		self.push_mapped(text, Vec::new())
	}

	pub(in crate::shell::agent_surface) fn markdown(original: Rc<str>) -> Self {
		Self {
			source: source::Source { original: Some(original), spans: Vec::new() },
			..Default::default()
		}
	}

	pub(in crate::shell::agent_surface) fn checkpoint(&self) -> usize {
		self.blocks.len()
	}

	pub(in crate::shell::agent_surface) fn mark(
		&mut self,
		start: usize,
		source: Option<Range<usize>>,
	) {
		if let (Some(start), Some(source)) = (self.blocks.get(start), source) {
			self.source.spans.push((*start..self.text.len(), source));
		}
	}

	pub(in crate::shell::agent_surface) fn push_mapped(
		&mut self,
		text: SelectableText,
		spans: Vec<(Range<usize>, Range<usize>)>,
	) -> impl IntoElement + use<> {
		let separator = self.separator.take().unwrap_or("\n\n");

		if !self.text.is_empty() {
			let trailing = self.text.chars().rev().take_while(|c| *c == '\n').count();

			self.text.push_str(if separator.starts_with('\n') {
				&separator[trailing.min(separator.len())..]
			} else {
				separator
			});
		}

		let start = self.text.len();

		self.blocks.push(start);
		self.text.push_str(&text.text);
		self.source.spans.extend(
			spans
				.into_iter()
				.map(|(visible, source)| (start + visible.start..start + visible.end, source)),
		);

		TextBlock { text, range: start..self.text.len(), scope: self.scope.clone() }
	}

	pub(in crate::shell::agent_surface) fn separate(&mut self, separator: &'static str) {
		self.separator = Some(separator);
	}

	pub(in crate::shell::agent_surface) fn finish(self, key: &str, child: Div) -> AnyElement {
		DocumentView { document: self, key: key.into(), child }.into_any_element()
	}
}

pub(super) struct Scope {
	pub state: Entity<Selection>,
	pub text: SharedString,
	pub layouts: RefCell<Vec<(Range<usize>, TextLayout)>>,
	source: source::Source,
}
impl Scope {
	pub(super) fn context_menu(&self, _window: &Window, cx: &mut App) {
		let range = self.state.read(cx).range(&self.text);

		if !range.is_empty() {
			#[cfg(all(target_os = "macos", not(test)))]
			menu::show(
				_window,
				cx,
				self.source.copy(&self.text, range.clone()),
				self.text[range].into(),
			);
		}
	}

	fn copy(&self, cx: &mut App) {
		let range = self.state.read(cx).range(&self.text);

		if !range.is_empty() {
			cx.write_to_clipboard(ClipboardItem::new_string(self.source.copy(&self.text, range)));
		}
	}

	fn index(&self, position: Point<Pixels>) -> Option<usize> {
		self.layouts
			.borrow()
			.iter()
			.min_by_key(|(_, layout)| {
				let bounds = layout.bounds();
				let dx =
					(bounds.left() - position.x).max(position.x - bounds.right()).max(gpui::px(0.));
				let dy =
					(bounds.top() - position.y).max(position.y - bounds.bottom()).max(gpui::px(0.));

				(f32::from(dx).powi(2) + f32::from(dy).powi(2)).to_bits()
			})
			.map(|(range, layout)| {
				range.start
					+ layout.index_for_position(position).unwrap_or_else(|ix| ix).min(range.len())
			})
	}
}

pub(super) struct Selection {
	pub anchor: usize,
	pub head: usize,
	pub dragging: bool,
	pub focus: FocusHandle,
}
impl Selection {
	pub fn range(&self, text: &str) -> Range<usize> {
		text.floor_char_boundary(self.anchor.min(self.head).min(text.len()))
			..text.floor_char_boundary(self.anchor.max(self.head).min(text.len()))
	}
}

#[derive(IntoElement)]
struct DocumentView {
	document: Document,
	key: String,
	child: Div,
}
impl RenderOnce for DocumentView {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let state = window.use_keyed_state(
			SharedString::from(format!("selection-{}", self.key)),
			cx,
			|_, cx| Selection { anchor: 0, head: 0, dragging: false, focus: cx.focus_handle() },
		);
		let focus = state.read(cx).focus.clone();
		let text: SharedString = self.document.text.into();
		let scope = self.document.scope;
		let _ = scope.set(Scope {
			state: state.clone(),
			text: text.clone(),
			layouts: RefCell::new(Vec::new()),
			source: self.document.source,
		});
		let up = state.clone();
		let outside = state.clone();
		let keys = scope.clone();
		let menu = scope.clone();

		self.child
			.id(SharedString::from(format!("text-document-{}", self.key)))
			.relative()
			.track_focus(&focus)
			.on_mouse_up(MouseButton::Left, move |_, _, cx| {
				up.update(cx, |s, _| s.dragging = false)
			})
			.on_mouse_up_out(MouseButton::Left, move |_, _, cx| {
				outside.update(cx, |s, _| s.dragging = false)
			})
			.on_mouse_down(MouseButton::Right, move |_, window, cx| {
				menu.get().expect("selection scope").context_menu(window, cx);
				cx.stop_propagation();
			})
			.on_key_down(move |event: &KeyDownEvent, _, cx| {
				if !event.keystroke.modifiers.platform {
					return;
				}

				match event.keystroke.key.as_str() {
					"c" => keys.get().expect("selection scope").copy(cx),
					"a" => state.update(cx, |s, cx| {
						s.anchor = 0;
						s.head = text.len();

						cx.notify();
					}),
					_ => return,
				}

				cx.stop_propagation();
			})
			.child(drag(scope))
	}
}

fn drag(scope: Rc<OnceCell<Scope>>) -> impl IntoElement {
	gpui::canvas(
		|_, _, _| (),
		move |_, _, window, _| {
			let down = scope.clone();

			window.on_mouse_event(move |event: &MouseDownEvent, phase, _, cx| {
				let scope = down.get().expect("selection scope");
				let extend = event.modifiers.shift
					&& scope
						.layouts
						.borrow()
						.iter()
						.any(|(_, layout)| layout.bounds().contains(&event.position));

				if phase == DispatchPhase::Capture
					&& event.button == MouseButton::Left
					&& !extend
					&& !scope.state.read(cx).range(&scope.text).is_empty()
				{
					scope.state.update(cx, |s, cx| {
						s.anchor = 0;
						s.head = 0;
						s.dragging = false;

						cx.notify();
					});
				}
			});

			let scope = scope.clone();

			window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
				let scope = scope.get().expect("selection scope");

				if phase == DispatchPhase::Capture
					&& scope.state.read(cx).dragging
					&& event.pressed_button == Some(MouseButton::Left)
					&& let Some(index) = scope.index(event.position)
					&& scope.state.read(cx).head != index
				{
					scope.state.update(cx, |s, cx| {
						s.head = index;

						cx.notify();
					});
				}
			});
		},
	)
	.absolute()
	.inset_0()
}
