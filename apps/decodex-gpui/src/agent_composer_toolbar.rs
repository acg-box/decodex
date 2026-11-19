//! Fit the composer actions to the width allocated by the conversation.
use std::panic::Location;

use gpui::{
	AnyElement, App, AvailableSpace, Bounds, Div, Element, ElementId, Entity, FontWeight,
	GlobalElementId, InspectorElementId, LayoutId, Pixels, Stateful, Style, Window,
};

use super::{
	AgentSurface, Context, MODEL_CONTROL_MAX_WIDTH, SmoothControl, Symbol, workspace_symbols,
};
use crate::{
	ui_motion::HoverHint as _,
	ui_theme::{BODY_SIZE, CHROME_CONTROL_SIZE, CONTROL_GAP, CONTROL_SIZE},
};
use gpui::prelude::{FluentBuilder, InteractiveElement, IntoElement, ParentElement, Styled};

const MODEL_TIP: &str = "Model and reasoning for the next turn";
const WRAPPED_COLUMN_GAP: f32 = CONTROL_GAP / 2.;

pub(super) fn action_row() -> Stateful<Div> {
	gpui::div()
		.id("composer-action-row")
		.debug_selector(|| "composer-action-row".into())
		.w_full()
		.min_w_0()
		.flex()
		.items_center()
		.gap(gpui::px(CONTROL_GAP))
}

pub(super) struct TextToolbar {
	owner: Entity<AgentSurface>,
	model: AnyElement,
	usage: Option<AnyElement>,
	attach: bool,
	dictate: bool,
}

impl TextToolbar {
	pub(super) fn new(surface: &AgentSurface, cx: &mut Context<AgentSurface>) -> Self {
		let model = gpui::div()
			.min_w(gpui::px(CHROME_CONTROL_SIZE))
			.max_w(gpui::px(MODEL_CONTROL_MAX_WIDTH))
			.text_size(gpui::px(BODY_SIZE))
			.font_weight(FontWeight::NORMAL)
			.child(surface.composer_control_content("model", surface.composer_model_label(cx), cx))
			.into_any_element();
		let dictate = surface.async_question_composer().is_none();
		Self {
			owner: cx.entity(),
			model,
			usage: surface.usage_line(cx),
			attach: dictate && !surface.voice_on_current_conversation(),
			dictate,
		}
	}
}

impl IntoElement for TextToolbar {
	type Element = Self;

	fn into_element(self) -> Self {
		self
	}
}

#[derive(Clone, Copy)]
pub(super) struct ToolbarWidths {
	labels: Pixels,
	with_usage: Option<Pixels>,
	attach: bool,
	dictate: bool,
}

#[derive(Clone, Copy)]
enum Arrangement {
	Labels { usage: bool },
	Icons { size: f32 },
	Wrapped { rows: usize },
}

impl Arrangement {
	fn height(self) -> Pixels {
		gpui::px(match self {
			Self::Labels { .. } => CONTROL_SIZE,
			Self::Icons { size } => size,
			Self::Wrapped { rows } =>
				rows as f32 * (CHROME_CONTROL_SIZE + CONTROL_GAP) - CONTROL_GAP,
		})
	}

	fn icon_size(self) -> Option<Pixels> {
		match self {
			Self::Labels { .. } => None,
			Self::Icons { size } => Some(gpui::px(size)),
			Self::Wrapped { .. } => Some(gpui::px(CHROME_CONTROL_SIZE)),
		}
	}
}

impl ToolbarWidths {
	fn button_count(&self) -> usize {
		3 + usize::from(self.attach) + usize::from(self.dictate)
	}

	fn arrangement(&self, width: Pixels) -> Arrangement {
		if width >= self.labels {
			return Arrangement::Labels {
				usage: self.with_usage.is_some_and(|needed| width >= needed),
			};
		}
		let count = self.button_count() as f32;
		for size in [CONTROL_SIZE, CHROME_CONTROL_SIZE] {
			if width >= gpui::px(count * size + (count - 1.) * CONTROL_GAP) {
				return Arrangement::Icons { size };
			}
		}
		let columns = ((f32::from(width) + WRAPPED_COLUMN_GAP)
			/ (CHROME_CONTROL_SIZE + WRAPPED_COLUMN_GAP)) as usize;
		let columns = columns.max(1);
		let settings = 2 + usize::from(self.dictate);
		let primary = 1 + usize::from(self.attach);
		Arrangement::Wrapped { rows: settings.div_ceil(columns) + primary.div_ceil(columns) }
	}
}

impl Element for TextToolbar {
	type PrepaintState = AnyElement;
	type RequestLayoutState = ToolbarWidths;

	fn id(&self) -> Option<ElementId> {
		None
	}

	fn source_location(&self) -> Option<&'static Location<'static>> {
		None
	}

	fn request_layout(
		&mut self,
		_: Option<&GlobalElementId>,
		_: Option<&InspectorElementId>,
		window: &mut Window,
		cx: &mut App,
	) -> (LayoutId, ToolbarWidths) {
		let natural = gpui::size(AvailableSpace::MaxContent, AvailableSpace::MinContent);
		let model = self.model.layout_as_root(natural, window, cx).width;
		let mut widths = ToolbarWidths {
			labels: model,
			with_usage: None,
			attach: self.attach,
			dictate: self.dictate,
		};
		widths.labels +=
			gpui::px((widths.button_count() - 1) as f32 * (CONTROL_SIZE + CONTROL_GAP));
		widths.with_usage = self
			.usage
			.as_mut()
			.map(|usage| widths.labels + usage.layout_as_root(natural, window, cx).width);
		let style = Style {
			size: gpui::size(gpui::relative(1.).into(), gpui::Length::Auto),
			flex_shrink: 0.,
			..Default::default()
		};
		let layout = window.request_measured_layout(style, move |known, available, _, _| {
			let width = known.width.unwrap_or_else(|| match available.width {
				AvailableSpace::Definite(width) => width,
				AvailableSpace::MaxContent => widths.with_usage.unwrap_or(widths.labels),
				AvailableSpace::MinContent => gpui::px(CHROME_CONTROL_SIZE),
			});
			gpui::size(width, widths.arrangement(width).height())
		});
		(layout, widths)
	}

	fn prepaint(
		&mut self,
		_: Option<&GlobalElementId>,
		_: Option<&InspectorElementId>,
		bounds: Bounds<Pixels>,
		widths: &mut ToolbarWidths,
		window: &mut Window,
		cx: &mut App,
	) -> AnyElement {
		let arrangement = widths.arrangement(bounds.size.width);
		let mut actions = self.owner.update(cx, |surface, cx| {
			surface.render_text_toolbar(arrangement, widths.attach, widths.dictate, cx)
		});
		actions.prepaint_as_root(bounds.origin, bounds.size.into(), window, cx);
		actions
	}

	fn paint(
		&mut self,
		_: Option<&GlobalElementId>,
		_: Option<&InspectorElementId>,
		_: Bounds<Pixels>,
		_: &mut ToolbarWidths,
		actions: &mut AnyElement,
		window: &mut Window,
		cx: &mut App,
	) {
		actions.paint(window, cx);
	}
}

impl AgentSurface {
	fn render_text_toolbar(
		&self,
		arrangement: Arrangement,
		show_attach: bool,
		show_dictate: bool,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let icon_size = arrangement.icon_size();
		let attach = show_attach.then(|| {
			self.toolbar_icon(
				"attach",
				"+",
				"Attachments and microphone",
				icon_size,
				|s, _, cx| s.toggle_composer_menu("attachments", cx),
				cx,
			)
		});
		let permissions = self.toolbar_icon(
			"permissions",
			&self.permission_label(),
			"Permissions for this Agent",
			icon_size,
			|s, _, cx| s.toggle_composer_menu("permissions", cx),
			cx,
		);
		let dictation = show_dictate.then(|| {
			self.toolbar_icon(
				"dictation",
				"",
				"Dictate into the draft",
				icon_size,
				|s, window, cx| s.start_dictation(window, cx),
				cx,
			)
		});
		let send = self.toolbar_icon(
			"send",
			"",
			self.toolbar_send_tip(cx),
			icon_size,
			Self::activate_toolbar_send,
			cx,
		);
		let model = self.toolbar_model(icon_size, cx);

		if matches!(arrangement, Arrangement::Wrapped { .. }) {
			let row = || {
				gpui::div()
					.flex()
					.flex_wrap()
					.gap_x(gpui::px(WRAPPED_COLUMN_GAP))
					.gap_y(gpui::px(CONTROL_GAP))
			};
			return action_row()
				.flex_col()
				.items_stretch()
				.child(row().justify_center().child(model).child(permissions).children(dictation))
				.child(row().children(attach).child(gpui::div().flex_none().ml_auto().child(send)))
				.into_any_element();
		}

		let usage = matches!(arrangement, Arrangement::Labels { usage: true })
			.then(|| self.usage_line(cx))
			.flatten();
		action_row()
			.gap_0()
			.child(
				gpui::div()
					.flex_none()
					.flex()
					.items_center()
					.gap(gpui::px(CONTROL_GAP))
					.children(attach)
					.child(model)
					.child(permissions),
			)
			.child(
				gpui::div().flex().min_w_0().ml_auto().items_center().children(usage).child(
					gpui::div()
						.flex_none()
						.flex()
						.items_center()
						.pl(gpui::px(CONTROL_GAP))
						.gap(gpui::px(CONTROL_GAP))
						.children(dictation)
						.child(send),
				),
			)
			.into_any_element()
	}

	fn toolbar_icon(
		&self,
		id: &'static str,
		label: &str,
		tip: &'static str,
		size: Option<Pixels>,
		action: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + Copy + 'static,
		cx: &mut Context<Self>,
	) -> AnyElement {
		self.composer_button(id, label, tip, action, cx)
			.when_some(size, |d, size| d.size(size).px_0())
			.child(self.composer_control_content(id, label.to_owned(), cx))
			.smooth()
			.into_any_element()
	}

	fn toolbar_model(&self, icon_size: Option<Pixels>, cx: &mut Context<Self>) -> AnyElement {
		let label = self.composer_model_label(cx);
		let button = self.composer_button(
			"model",
			&label,
			MODEL_TIP,
			|s, _, cx| s.toggle_composer_menu("model", cx),
			cx,
		);
		let button = if let Some(size) = icon_size {
			let tip = self.composer_control_tip(
				"model",
				&label,
				MODEL_TIP,
				self.composer_models(cx).is_none(),
				cx,
			);
			button
				.size(size)
				.flex_none()
				.child(
					gpui::div()
						.debug_selector(|| "composer-model-icon".into())
						.child(workspace_symbols::icon(Symbol::Settings)),
				)
				.when(self.composer_menu != Some("model"), |d| d.hover_hint(tip))
		} else {
			button
				.min_w(gpui::px(CHROME_CONTROL_SIZE))
				.child(self.composer_control_content("model", label, cx))
		};
		button.smooth().into_any_element()
	}

	fn toolbar_send_tip(&self, cx: &Context<Self>) -> &'static str {
		if self.async_question_composer().is_some() {
			"Send answer (Enter)"
		} else if self.interrupting.is_some() {
			"Stopping response"
		} else if self.awaiting_start(cx) {
			"Starting response"
		} else if self.stop_button(cx) {
			"Stop response (Esc twice)"
		} else if self.composer.read(cx).content().trim().is_empty()
			&& self.attachments.is_empty()
			&& self.task_references.is_empty()
		{
			"Start voice chat"
		} else {
			"Send (Enter)"
		}
	}

	fn activate_toolbar_send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		if self.async_question_composer().is_some() {
			self.submit(cx);
			return;
		}
		if self.awaiting_start(cx) || self.interrupting.is_some() {
			return;
		}
		if self.stop_button(cx) {
			self.interrupt_current(cx);
		} else if self.composer.read(cx).content().trim().is_empty()
			&& self.attachments.is_empty()
			&& self.task_references.is_empty()
		{
			self.start_voice(window, cx);
		} else {
			self.submit(cx);
		}
	}
}
