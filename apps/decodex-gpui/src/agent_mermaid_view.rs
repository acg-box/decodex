//! Prepare diagrams in the background and reuse bounded images across virtualized rows.
use std::{
	collections::VecDeque,
	ops::Range,
	sync::{Arc, OnceLock},
};

use gpui::{
	AnyElement, App, Context, InteractiveElement, IntoElement, ParentElement, RenderOnce,
	SharedString, StatefulInteractiveElement, Styled, Task, Window,
};

use crate::{
	shell::agent_surface::{
		markdown::{self, Node, mermaid, mermaid::Diagram},
		selectable_text::Document,
	},
	ui_theme::appearance::Theme,
};

// Adapted from the upstream closing-fence check; see agent_mermaid/NOTICE.md.
pub(super) fn has_closing_fence(input: &str, range: Range<usize>, content_end: usize) -> bool {
	let Some(block) = input.get(range.clone()) else { return false };
	let Some(marker @ (b'`' | b'~')) = block.as_bytes().first().copied() else { return false };
	let opening_len = block.bytes().take_while(|byte| *byte == marker).count();
	let Some(suffix) = input.get(content_end..range.end) else { return false };

	suffix
		.trim_end_matches([' ', '\t', '\r', '\n'])
		.bytes()
		.rev()
		.take_while(|byte| *byte == marker)
		.count()
		>= opening_len
}

pub(super) fn render(
	children: &[Node],
	key: &str,
	geometry: Arc<OnceLock<(f32, f32)>>,
	selection: &mut Document,
) -> AnyElement {
	// Register the source in message selection before the async view switches to its image.
	let fallback = markdown::code::render(children, Some("mermaid"), key, selection);
	Mermaid { source: markdown::code_text(children).into(), key: key.into(), geometry, fallback }
		.into_any_element()
}

#[derive(gpui::IntoElement)]
struct Mermaid {
	source: Arc<str>,
	key: String,
	geometry: Arc<OnceLock<(f32, f32)>>,
	fallback: AnyElement,
}

struct State {
	result: Option<Result<Arc<Diagram>, String>>,
	_task: Task<()>,
}

// Timeline rows are culled outside the viewport. Keep a bounded set of prepared images in the
// application so returning rows retain both their image and geometry on their first frame.
#[derive(Default)]
struct Images {
	entries: VecDeque<(Arc<str>, Theme, Arc<Diagram>)>,
	bytes: usize,
}
impl gpui::Global for Images {}
impl Images {
	fn get(&mut self, source: &str, theme: Theme) -> Option<Arc<Diagram>> {
		let index = self
			.entries
			.iter()
			.position(|(key, appearance, _)| &**key == source && *appearance == theme)?;
		let entry = self.entries.remove(index)?;
		let diagram = entry.2.clone();
		self.entries.push_back(entry);
		Some(diagram)
	}

	fn insert(
		&mut self,
		source: Arc<str>,
		theme: Theme,
		diagram: Arc<Diagram>,
	) -> Vec<Arc<Diagram>> {
		let limit = if cfg!(feature = "visual-capture")
			&& std::env::var("DECODEX_VISUAL_SCROLL_CACHE").as_deref() == Ok("off")
		{
			0
		} else {
			64 * 1_024 * 1_024
		};
		self.bytes += image_bytes(&source, &diagram);
		self.entries.push_back((source, theme, diagram));
		let mut evicted = Vec::new();
		while self.bytes > limit || self.entries.len() > 64 {
			let (source, _, diagram) = self.entries.pop_front().expect("cache exceeds its limit");
			self.bytes -= image_bytes(&source, &diagram);
			evicted.push(diagram);
		}
		evicted
	}
}

fn image_bytes(source: &str, diagram: &Diagram) -> usize {
	source.len() + diagram.image.as_bytes(0).map_or(0, <[u8]>::len)
}

fn release_image(diagram: &Arc<Diagram>, cx: &mut App) {
	// An evicted image can still be visible in another row or window. Its last owner releases
	// the atlas entry; cached images retain it until eviction.
	if Arc::strong_count(diagram) == 1 {
		cx.drop_image(diagram.image.clone(), None);
	}
}

impl State {
	fn new(source: Arc<str>, theme: Theme, cx: &mut Context<Self>) -> Self {
		let cached = cx.default_global::<Images>().get(&source, theme);
		let task =
			if cached.is_some() { Task::ready(()) } else { Self::prepare(source, theme, cx) };
		cx.on_release(|state, cx| {
			if let Some(Ok(diagram)) = &state.result {
				release_image(diagram, cx);
			}
		})
		.detach();

		Self { result: cached.map(Ok), _task: task }
	}

	fn prepare(source: Arc<str>, theme: Theme, cx: &mut Context<Self>) -> Task<()> {
		let renderer = cx.svg_renderer();
		let input = source.clone();
		let future = cx.background_executor().spawn(async move {
			let started = std::time::Instant::now();
			let result = mermaid::prepare(&input, &renderer, theme);
			if let Ok(diagram) = &result {
				crate::frame_trace::mermaid_prepared(
					started.elapsed(),
					diagram.width,
					diagram.height,
				);
			}
			result.map(Arc::new)
		});
		cx.spawn(async move |state, cx| {
			let result = future.await;
			let _ = state.update(cx, |state, cx| {
				let result = result.map(|diagram| {
					if let Some(cached) = cx.default_global::<Images>().get(&source, theme) {
						return cached;
					}
					for evicted in
						cx.default_global::<Images>().insert(source, theme, diagram.clone())
					{
						release_image(&evicted, cx);
					}
					diagram
				});
				state.result = Some(result);
				cx.notify();
			});
		})
	}
}

impl RenderOnce for Mermaid {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		// Reply text can keep streaming after a closed diagram. Its source, rather than the whole
		// reply, owns the cache key. A changed diagram gets new state and cannot show a stale
		// result.
		let state_key = SharedString::from(format!(
			"mermaid-state-{}-{:016x}-{:?}",
			self.key,
			gpui::hash(&self.source),
			Theme::current()
		));
		let state = window.use_keyed_state(state_key, cx, |_, cx| {
			State::new(self.source.clone(), Theme::current(), cx)
		});
		let diagram = match &state.read(cx).result {
			Some(Ok(diagram)) => {
				let _ = self.geometry.set((diagram.width, diagram.height));
				Some(diagram)
			},
			Some(Err(_)) => return self.fallback,
			None => None,
		};
		// Parsed Markdown retains only this small geometry record after image eviction. A row
		// must not shrink back to source-code height while its image is prepared again.
		let Some(&(width, height)) = self.geometry.get() else {
			return self.fallback;
		};
		let selector = format!("mermaid-{}", self.key);
		let image_selector = format!("mermaid-image-{}", self.key);
		let image = if let Some(diagram) = diagram {
			gpui::img(diagram.image.clone())
				.id(SharedString::from(image_selector.clone()))
				.debug_selector(move || image_selector.clone())
				.aria_label("Mermaid diagram")
				.w(gpui::px(width))
				.h(gpui::px(height))
				.flex_shrink_0()
				.into_any_element()
		} else {
			gpui::div()
				.w(gpui::px(width))
				.h(gpui::px(height))
				.flex_shrink_0()
				.flex()
				.items_center()
				.justify_center()
				.text_color(crate::ui_theme::rgb(crate::ui_theme::TEXT_MUTED))
				.child("Rendering diagram…")
				.into_any_element()
		};
		frame(&self.key, self.source, selector, width, image)
	}
}

fn frame(
	key: &str,
	source: Arc<str>,
	selector: String,
	width: f32,
	image: AnyElement,
) -> AnyElement {
	gpui::div()
		.flex()
		.flex_col()
		.gap_1()
		.min_w_0()
		.w_full()
		.p_3()
		.rounded(gpui::px(10.))
		.border_1()
		.border_color(crate::ui_theme::rgba(0xffffff0c))
		.bg(crate::ui_theme::rgba(0xffffff03))
		.child(gpui::div().flex().w_full().justify_end().child(markdown::copy_button(
			&format!("mermaid-copy-{key}"),
			"Copy Mermaid source",
			source.to_string(),
		)))
		.child(
			gpui::div()
				.id(SharedString::from(selector.clone()))
				.debug_selector(move || selector.clone())
				.w_full()
				.min_w_0()
				.flex()
				.flex_col()
				.items_start()
				.overflow_x_scroll()
				.child(
					gpui::div()
						.flex()
						.w_full()
						.min_w(gpui::px(width))
						.justify_center()
						.child(image),
				),
		)
		.into_any_element()
}

#[cfg(test)]
#[path = "agent_mermaid_tests.rs"]
mod tests;
