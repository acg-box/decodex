//! A small troubleshooting summary with optional technical details.
use std::rc::Rc;

use gpui::{
	AnyElement, Bounds, Context, FontWeight, IntoElement as _, MouseButton, Pixels, Role,
	ScrollHandle, Window,
	prelude::{
		FluentBuilder as _, InteractiveElement as _, ParentElement as _,
		StatefulInteractiveElement as _, Styled as _,
	},
};

use super::{
	Destination, HEALTH_APP_SERVER_COMPONENTS, HEALTH_CORE_COMPONENTS, HEALTH_OPTIONAL_COMPONENTS,
	HealthLoadState, HealthSnapshot, Shell, WB_TEXT, WB_TEXT_MUTED, health_component_section,
	health_presentation, native_process_summary,
};
use crate::{
	ui_motion::SmoothControl as _,
	ui_theme::{BODY_SIZE, HOVER_FILL, SETTINGS_GROUP_GAP, SETTINGS_INSET, SETTINGS_WIDTH},
};
use decodex_protocol::{DoctorComponent, DoctorIssue, DoctorStatus};

#[derive(Default)]
pub(super) struct Diagnostics {
	pub(super) expanded: bool,
	pub(super) scroll: ScrollHandle,
	pub(super) copied: bool,
}

/// Unchecked and unused capabilities are not problems. An old report cannot
/// describe the current connection; keep it available only in technical details.
pub(super) fn issues(snapshot: &HealthSnapshot) -> Vec<DoctorIssue> {
	if snapshot.load != HealthLoadState::Ready {
		return vec![];
	}
	let mut issues = vec![];
	for check in snapshot.report.iter().flat_map(|report| report.checks()) {
		let issue = match check.status {
			DoctorStatus::Ready
			| DoctorStatus::Unknown(DoctorIssue::NotProbed | DoctorIssue::Disabled)
			| DoctorStatus::Unavailable(DoctorIssue::NotProbed | DoctorIssue::Disabled) => continue,
			DoctorStatus::Unknown(DoctorIssue::Plugin)
				if check.component == DoctorComponent::PluginReadiness =>
				continue,
			DoctorStatus::Unavailable(issue) | DoctorStatus::Unknown(issue) => issue,
		};
		if !issues.contains(&issue) {
			issues.push(issue);
		}
	}
	issues
}

fn issue_description(issue: DoctorIssue) -> (&'static str, &'static str) {
	match issue {
		DoctorIssue::Authentication =>
			("Account access needs attention", "Open Accounts to check sign-in and availability."),
		DoctorIssue::ConfigurationMissing | DoctorIssue::ConfigurationMalformed => (
			"App settings could not be read",
			"Restart Decodex, then check again. Copy the report if the problem continues.",
		),
		DoctorIssue::ConfigurationVersion | DoctorIssue::ProtocolVersionMismatch =>
			("App components do not match", "Check Updates & Components, then restart Decodex."),
		DoctorIssue::DatabaseNotConfigured
		| DoctorIssue::DatabaseMalformedConfig
		| DoctorIssue::DatabaseUnreachable
		| DoctorIssue::DatabaseIncompatible
		| DoctorIssue::UnsafeDatabaseAuthority
		| DoctorIssue::Integrity =>
			("Saved data could not be checked", "Copy the report for help with local storage."),
		DoctorIssue::ProtocolDisconnected
		| DoctorIssue::ServerIdentityMismatch
		| DoctorIssue::ServerIdentityUnavailable =>
			("Local connection needs attention", "Restart Decodex to reconnect, then check again."),
		DoctorIssue::UnsafeHostPath =>
			("A required folder is unavailable", "Copy the report for help with local file access."),
		DoctorIssue::Plugin => (
			"A required plugin is unavailable",
			"Copy the report for help with the configured plugins.",
		),
		DoctorIssue::NotProbed | DoctorIssue::Disabled => ("Check incomplete", "Check again."),
	}
}

fn report_text(snapshot: &HealthSnapshot) -> Option<String> {
	let report = serde_json::to_string_pretty(snapshot.report.as_ref()?).ok()?;
	Some(format!("Decodex diagnostics\nReport state: {:?}\n{report}", snapshot.load))
}

pub(super) fn content(shell: &Shell, standalone: bool, cx: &mut Context<Shell>) -> AnyElement {
	let snapshot = &shell.health;
	let presentation = health_presentation(snapshot);
	let expanded = shell.diagnostics.expanded;
	let report = report_text(snapshot);
	let mut content = gpui::div()
		.w_full()
		.max_w(gpui::px(SETTINGS_WIDTH))
		.mx_auto()
		.flex()
		.flex_col()
		.gap(gpui::px(SETTINGS_GROUP_GAP))
		.text_size(gpui::px(BODY_SIZE))
		.child(
			gpui::div()
				.id("health-query-status")
				.debug_selector(|| "health-query-status".into())
				.role(Role::Status)
				.aria_label(format!("Diagnostics: {}", presentation.label))
				.flex()
				.items_start()
				.gap_3()
				.py_2()
				.child(
					gpui::div()
						.mt(gpui::px(6.))
						.size(gpui::px(7.))
						.flex_none()
						.rounded_full()
						.bg(gpui::rgb(presentation.color)),
				)
				.child(
					gpui::div()
						.min_w_0()
						.flex_1()
						.flex()
						.flex_col()
						.gap_2()
						.child(
							gpui::div().font_weight(FontWeight::SEMIBOLD).child(presentation.label),
						)
						.child(
							gpui::div()
								.text_color(gpui::rgb(WB_TEXT_MUTED))
								.child(presentation.detail),
						),
				),
		);
	for (index, issue) in issues(snapshot).into_iter().enumerate() {
		let (title, detail) = issue_description(issue);
		content = content.child(
			gpui::div()
				.id(("health-issue", index))
				.debug_selector(move || format!("health-issue-{index}"))
				.flex()
				.flex_col()
				.gap_2()
				.child(gpui::div().font_weight(FontWeight::MEDIUM).child(title))
				.child(gpui::div().text_color(gpui::rgb(WB_TEXT_MUTED)).child(detail)),
		);
	}
	content = content
		.child(
			gpui::div()
				.flex()
				.flex_wrap()
				.gap_2()
				.child(
					control("health-accounts", "Accounts", cx, move |shell, cx| {
						shell.select_settings_destination(Destination::Accounts, standalone, cx);
					})
					.smooth(),
				)
				.when_some(report, |row, report| {
					row.child(
						control(
							"health-copy-report",
							if shell.diagnostics.copied { "Copied" } else { "Copy report" },
							cx,
							move |shell, cx| {
								cx.write_to_clipboard(gpui::ClipboardItem::new_string(
									report.clone(),
								));
								shell.diagnostics.copied = true;
							},
						)
						.w(gpui::px(108.))
						.smooth(),
					)
				}),
		)
		.child(
			gpui::div()
				.text_color(gpui::rgb(WB_TEXT_MUTED))
				.child("Checks cover local services. Chat errors appear in the input bar."),
		)
		.child(
			gpui::div()
				.id("health-details-toggle")
				.debug_selector(|| "health-details-toggle".into())
				.role(Role::Button)
				.tab_index(0)
				.aria_label("Technical details")
				.aria_expanded(expanded)
				.w_full()
				.py_3()
				.flex()
				.items_center()
				.gap_2()
				.border_t_1()
				.border_color(gpui::rgba(0xffffff10))
				.cursor_pointer()
				.text_color(gpui::rgb(WB_TEXT_MUTED))
				.hover(|el| el.text_color(gpui::rgb(WB_TEXT)))
				.focus_visible(|el| el.bg(gpui::rgba(HOVER_FILL)))
				.on_click(cx.listener(|shell, _, _, cx| {
					shell.diagnostics.expanded = !shell.diagnostics.expanded;
					cx.notify();
				}))
				.on_key_down(cx.listener(|shell, event: &gpui::KeyDownEvent, _, cx| {
					if !event.is_held && ["enter", "space"].contains(&event.keystroke.key.as_str())
					{
						shell.diagnostics.expanded = !shell.diagnostics.expanded;
						cx.notify();
						cx.stop_propagation();
					}
				}))
				.child(if expanded { "⌄" } else { "›" })
				.child("Technical details")
				.smooth(),
		);
	if expanded {
		content = content.child(
			gpui::div()
				.id("health-components")
				.debug_selector(|| "health-components".into())
				.flex()
				.flex_col()
				.gap_5()
				.child(
					gpui::div()
						.flex()
						.flex_col()
						.gap_2()
						.child(gpui::div().font_weight(FontWeight::MEDIUM).child("Agent process"))
						.child(gpui::div().text_color(gpui::rgb(WB_TEXT_MUTED)).child(
							native_process_summary(
								snapshot.report.as_ref().and_then(|report| report.native_process()),
							),
						)),
				)
				.child(health_component_section(
					"health-core-components",
					"Local services",
					"",
					0,
					&HEALTH_CORE_COMPONENTS,
					snapshot,
				))
				.child(health_component_section(
					"health-app-server-components",
					"Codex capabilities",
					"Not checked in this report",
					HEALTH_CORE_COMPONENTS.len(),
					&HEALTH_APP_SERVER_COMPONENTS,
					snapshot,
				))
				.child(health_component_section(
					"health-optional-components",
					"Optional features",
					"",
					HEALTH_CORE_COMPONENTS.len() + HEALTH_APP_SERVER_COMPONENTS.len(),
					&HEALTH_OPTIONAL_COMPONENTS,
					snapshot,
				)),
		);
	}

	// The viewport must not contribute the report's intrinsic height to Settings.
	// One bounded scroll area owns all content, including expanded details.
	gpui::div()
		.flex_1()
		.min_h_0()
		.relative()
		.child(
			gpui::div()
				.id("health-scroll-viewport")
				.debug_selector(|| "health-scroll-viewport".into())
				.absolute()
				.inset_0()
				.overflow_y_scroll()
				.track_scroll(&shell.diagnostics.scroll)
				.child(
					gpui::div()
						.px(gpui::px(SETTINGS_INSET))
						.pt(gpui::px(SETTINGS_GROUP_GAP))
						.pb(gpui::px(SETTINGS_INSET))
						.child(content),
				),
		)
		.child(Scrollbar(shell.diagnostics.scroll.clone()))
		.into_any_element()
}

fn control(
	id: &'static str,
	label: &'static str,
	cx: &Context<Shell>,
	action: impl Fn(&mut Shell, &mut Context<Shell>) + 'static,
) -> gpui::Stateful<gpui::Div> {
	let action = Rc::new(action);
	let click = action.clone();
	gpui::div()
		.id(id)
		.debug_selector(move || id.into())
		.role(Role::Button)
		.tab_index(0)
		.aria_label(label)
		.h(gpui::px(30.))
		.px_3()
		.rounded(gpui::px(7.))
		.flex()
		.items_center()
		.justify_center()
		.border_1()
		.border_color(gpui::rgba(0xffffff16))
		.cursor_pointer()
		.text_color(gpui::rgb(WB_TEXT_MUTED))
		.hover(|el| el.bg(gpui::rgba(HOVER_FILL)).text_color(gpui::rgb(WB_TEXT)))
		.active(|el| el.opacity(0.7))
		.focus_visible(|el| el.border_color(gpui::rgb(crate::ui_theme::BLUE)))
		.on_click(cx.listener(move |shell, _, _, cx| {
			click(shell, cx);
			cx.notify();
		}))
		.on_key_down(cx.listener(move |shell, event: &gpui::KeyDownEvent, _, cx| {
			if !event.is_held && ["enter", "space"].contains(&event.keystroke.key.as_str()) {
				action(shell, cx);
				cx.notify();
				cx.stop_propagation();
			}
		}))
		.child(label)
}

#[derive(gpui::IntoElement)]
struct Scrollbar(ScrollHandle);

impl gpui::RenderOnce for Scrollbar {
	fn render(self, window: &mut Window, cx: &mut gpui::App) -> impl gpui::IntoElement {
		let drag = window.use_keyed_state("health-scrollbar-drag", cx, |_, _| None::<Pixels>);
		let owner = window.current_view();
		let keys = self.0.clone();
		let canvas = gpui::canvas(
			move |bounds, window, _| {
				let viewport = self.0.bounds().size.height;
				let maximum = self.0.max_offset().y;
				if maximum <= gpui::px(0.) || viewport <= gpui::px(0.) {
					return None;
				}
				let height = (bounds.size.height * (viewport / (viewport + maximum)))
					.max(gpui::px(28.))
					.min(bounds.size.height);
				let travel = bounds.size.height - height;
				let y = travel * (-self.0.offset().y / maximum);
				let thumb = Bounds::new(
					gpui::point(bounds.left() + gpui::px(3.), bounds.top() + y),
					gpui::size(gpui::px(5.), height),
				);
				Some((
					self.0,
					thumb,
					travel,
					window.insert_hitbox(bounds, gpui::HitboxBehavior::Normal),
				))
			},
			move |bounds, state, window, _| {
				let Some((scroll, thumb, travel, hitbox)) = state else { return };
				window.paint_quad(
					gpui::fill(thumb, gpui::rgba(0xffffff55)).corner_radii(gpui::px(2.5)),
				);
				let down_scroll = scroll.clone();
				let down_drag = drag.clone();
				let down_thumb = thumb;
				let hitbox = hitbox.clone();
				window.on_mouse_event(move |event: &gpui::MouseDownEvent, phase, window, cx| {
					if phase == gpui::DispatchPhase::Bubble
						&& event.button == MouseButton::Left
						&& hitbox.is_hovered(window)
					{
						window.prevent_default();
						let grab = if down_thumb.contains(&event.position) {
							event.position.y - down_thumb.top()
						} else {
							down_thumb.size.height / 2.
						};
						down_drag.update(cx, |value, _| *value = Some(grab));
						set_scroll(&down_scroll, event.position.y - bounds.top() - grab, travel);
						cx.notify(owner);
						cx.stop_propagation();
					}
				});
				let scroll = scroll.clone();
				let up_drag = drag.clone();
				window.on_mouse_event(move |_: &gpui::MouseUpEvent, phase, _, cx| {
					if phase == gpui::DispatchPhase::Capture {
						up_drag.update(cx, |value, _| *value = None);
					}
				});
				window.on_mouse_event(move |event: &gpui::MouseMoveEvent, phase, _, cx| {
					if phase != gpui::DispatchPhase::Capture {
						return;
					}
					let Some(grab) = *drag.read(cx) else { return };
					if event.pressed_button == Some(MouseButton::Left) {
						set_scroll(&scroll, event.position.y - bounds.top() - grab, travel);
						cx.notify(owner);
						cx.stop_propagation();
					} else {
						drag.update(cx, |value, _| *value = None);
					}
				});
			},
		)
		.size_full();
		gpui::div()
			.id("health-scrollbar")
			.debug_selector(|| "health-scrollbar".into())
			.role(Role::ScrollBar)
			.aria_label("Scroll diagnostics")
			.tab_index(0)
			.on_key_down(move |event: &gpui::KeyDownEvent, _, cx| {
				let current = keys.offset().y;
				let page = keys.bounds().size.height;
				let maximum = keys.max_offset().y;
				let next = match event.keystroke.key.as_str() {
					"up" => current + gpui::px(40.),
					"down" => current - gpui::px(40.),
					"pageup" => current + page,
					"pagedown" => current - page,
					"home" => gpui::px(0.),
					"end" => -maximum,
					_ => return,
				};
				keys.set_offset(gpui::point(gpui::px(0.), next.clamp(-maximum, gpui::px(0.))));
				cx.notify(owner);
				cx.stop_propagation();
			})
			.absolute()
			.top_1()
			.bottom_1()
			.right_1()
			.w(gpui::px(11.))
			.child(canvas)
	}
}

fn set_scroll(scroll: &ScrollHandle, position: Pixels, travel: Pixels) {
	if travel > gpui::px(0.) {
		let fraction = (position / travel).clamp(0., 1.);
		scroll.set_offset(gpui::point(gpui::px(0.), -scroll.max_offset().y * fraction));
	}
}
