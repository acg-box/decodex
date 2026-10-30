//! Diagnostic results and actions with optional technical details.

use std::rc::Rc;

use gpui::{
	AnyElement, ClipboardItem, Context, Div, FocusHandle, IntoElement as _, KeyDownEvent, Role,
	ScrollHandle, Stateful, Window,
	prelude::{
		FluentBuilder as _, InteractiveElement as _, ParentElement as _,
		StatefulInteractiveElement as _, Styled as _,
	},
};

use crate::{
	shell::{
		self, HEALTH_APP_SERVER_COMPONENTS, HEALTH_CORE_COMPONENTS, HEALTH_OPTIONAL_COMPONENTS,
		HealthLoadState, HealthSnapshot, Shell, workspace_symbols, workspace_symbols::Symbol,
	},
	ui_feedback::{self, Feedback, Tone},
	ui_motion::{self, SmoothControl as _},
	ui_scroll::Scrollbar,
	ui_theme::{
		self, HOVER_FILL, SETTINGS_GROUP_GAP, SETTINGS_INSET, SETTINGS_LABEL_SIZE, SETTINGS_WIDTH,
		TEXT,
	},
};
use decodex_protocol::{DoctorComponent, DoctorIssue, DoctorReport, DoctorStatus};

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

pub(super) fn controls(
	shell: &Shell,
	refresh_focus: FocusHandle,
	window: &Window,
	cx: &mut Context<Shell>,
) -> AnyElement {
	gpui::div()
		.flex()
		.items_center()
		.gap_2()
		.when_some(report_text(&shell.health), |row, report| {
			row.child(
				control(
					"health-copy-report",
					if shell.diagnostics.copied { "Copied" } else { "Copy report" },
					cx,
					move |shell, cx| {
						cx.write_to_clipboard(ClipboardItem::new_string(report.clone()));

						shell.diagnostics.copied = true;
					},
				)
				.min_w(gpui::px(84.))
				.smooth(),
			)
		})
		.child(shell::refresh_control(refresh_focus, shell.health.can_refresh, window, cx))
		.into_any_element()
}

pub(super) fn content(shell: &Shell, cx: &mut Context<Shell>) -> AnyElement {
	let content = summary_content(shell, cx);

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
		.child(Scrollbar::new(
			"health-scrollbar",
			"Scroll diagnostics",
			shell.diagnostics.scroll.clone(),
		))
		.into_any_element()
}

fn issue_description(issue: DoctorIssue) -> (&'static str, &'static str) {
	match issue {
		DoctorIssue::Authentication =>
			("Account access needs attention", "Check sign-in and availability in Accounts."),
		DoctorIssue::ConfigurationMissing | DoctorIssue::ConfigurationMalformed => (
			"App settings could not be read",
			"Restart and check again. Copy the report if the problem continues.",
		),
		DoctorIssue::ConfigurationVersion | DoctorIssue::ProtocolVersionMismatch =>
			("App components do not match", "Open Updates, then restart Decodex."),
		DoctorIssue::DatabaseNotConfigured
		| DoctorIssue::DatabaseMalformedConfig
		| DoctorIssue::DatabaseUnreachable
		| DoctorIssue::DatabaseIncompatible
		| DoctorIssue::UnsafeDatabaseAuthority
		| DoctorIssue::Integrity =>
			("Saved data could not be checked", "Copy the report for storage troubleshooting."),
		DoctorIssue::ProtocolDisconnected
		| DoctorIssue::ServerIdentityMismatch
		| DoctorIssue::ServerIdentityUnavailable =>
			("Local connection needs attention", "Restart to reconnect, then check again."),
		DoctorIssue::UnsafeHostPath =>
			("A required folder is unavailable", "Copy the report for file access troubleshooting."),
		DoctorIssue::Plugin =>
			("A required plugin is unavailable", "Copy the report for plugin troubleshooting."),
		DoctorIssue::NotProbed | DoctorIssue::Disabled => ("Check incomplete", "Check again."),
	}
}

fn report_text(snapshot: &HealthSnapshot) -> Option<String> {
	let report = serde_json::to_string_pretty(snapshot.report.as_ref()?).ok()?;

	Some(format!("Decodex diagnostics\nReport state: {:?}\n{report}", snapshot.load))
}

fn summary_content(shell: &Shell, cx: &mut Context<Shell>) -> AnyElement {
	let snapshot = &shell.health;
	let presentation = shell::health_presentation(snapshot);
	let mut content = gpui::div()
		.w_full()
		.max_w(gpui::px(SETTINGS_WIDTH))
		.mx_auto()
		.flex()
		.flex_col()
		.gap(gpui::px(SETTINGS_GROUP_GAP))
		.text_size(gpui::px(SETTINGS_LABEL_SIZE))
		.child(
			ui_feedback::status_row(
				"health-query-status",
				presentation.tone,
				presentation.label,
				presentation.detail,
				presentation.tone.icon(),
			)
			.debug_selector(|| "health-query-status".into())
			.min_h(gpui::px(32.)),
		);

	for (index, issue) in issues(snapshot).into_iter().enumerate() {
		let (title, detail) = issue_description(issue);

		content = content.child(
			gpui::div()
				.id(("health-issue", index))
				.debug_selector(move || format!("health-issue-{index}"))
				.children(ui_feedback::inline_feedback(
					format!("health-issue-message-{index}"),
					title,
					&Feedback::warning(detail),
				)),
		);
	}

	if snapshot.load == HealthLoadState::Ready {
		content = content.child(component_section(
			"health-core-components",
			"Local services",
			0,
			&HEALTH_CORE_COMPONENTS,
			snapshot.report.as_ref(),
		));
	}

	let snapshot = snapshot.clone();

	content
		.child(gpui::div().flex().flex_col().child(details_toggle(shell, cx)).child(
			ui_motion::disclosure_lazy(
				"health-details-motion",
				shell.diagnostics.expanded,
				move |_| technical_details(&snapshot),
			),
		))
		.into_any_element()
}

fn details_toggle(shell: &Shell, cx: &Context<Shell>) -> AnyElement {
	let expanded = shell.diagnostics.expanded;

	gpui::div()
		.id("health-details-toggle")
		.debug_selector(|| "health-details-toggle".into())
		.role(Role::Button)
		.tab_index(0)
		.aria_label("Technical details")
		.aria_expanded(expanded)
		.w_full()
		.min_h(gpui::px(32.0))
		.py(gpui::px(6.0))
		.rounded(gpui::px(6.))
		.flex()
		.items_center()
		.gap_2()
		.cursor_pointer()
		.text_size(gpui::px(SETTINGS_LABEL_SIZE))
		.text_color(ui_theme::rgb(TEXT))
		.hover(|el| el.bg(ui_theme::rgba(HOVER_FILL)))
		.focus_visible(|el| el.bg(ui_theme::rgba(HOVER_FILL)))
		.on_click(cx.listener(|shell, _, _, cx| {
			shell.diagnostics.expanded = !shell.diagnostics.expanded;

			cx.notify();
		}))
		.on_key_down(cx.listener(|shell, event: &KeyDownEvent, _, cx| {
			if !event.is_held && ["enter", "space"].contains(&event.keystroke.key.as_str()) {
				shell.diagnostics.expanded = !shell.diagnostics.expanded;

				cx.notify();
				cx.stop_propagation();
			}
		}))
		.child(workspace_symbols::icon_sized(
			if expanded { Symbol::ChevronDown } else { Symbol::Forward },
			12.,
		))
		.child("Technical details")
		.smooth()
		.into_any_element()
}

fn technical_details(snapshot: &HealthSnapshot) -> AnyElement {
	gpui::div()
		.id("health-components")
		.debug_selector(|| "health-components".into())
		.flex()
		.flex_col()
		.pt_3()
		.gap(gpui::px(SETTINGS_GROUP_GAP))
		.when(snapshot.load != HealthLoadState::Ready && snapshot.report.is_some(), |details| {
			details
				.child(ui_theme::settings_description("Previous report; results may have changed."))
				.child(component_section(
					"health-previous-core-components",
					"Local services",
					0,
					&HEALTH_CORE_COMPONENTS,
					snapshot.report.as_ref(),
				))
		})
		.child(
			gpui::div()
				.flex()
				.flex_col()
				.gap_2()
				.child(ui_theme::settings_section_title("Agent process"))
				.child(ui_theme::settings_description(shell::native_process_summary(
					snapshot.report.as_ref().and_then(|report| report.native_process()),
				))),
		)
		.child(component_section(
			"health-app-server-components",
			"Codex capabilities",
			HEALTH_CORE_COMPONENTS.len(),
			&HEALTH_APP_SERVER_COMPONENTS,
			snapshot.report.as_ref(),
		))
		.child(component_section(
			"health-optional-components",
			"Optional features",
			HEALTH_CORE_COMPONENTS.len() + HEALTH_APP_SERVER_COMPONENTS.len(),
			&HEALTH_OPTIONAL_COMPONENTS,
			snapshot.report.as_ref(),
		))
		.into_any_element()
}

fn component_row(
	index: usize,
	component: DoctorComponent,
	status: Option<DoctorStatus>,
) -> AnyElement {
	let label = shell::component_label(component);
	let presentation = shell::component_presentation(status);

	gpui::div()
		.id(("health-component", index))
		.debug_selector(move || format!("health-component-{index}"))
		.role(Role::ListItem)
		.aria_label(format!("{label}: {}", presentation.label))
		.min_w_0()
		.min_h(gpui::px(28.))
		.py_1()
		.flex()
		.items_center()
		.justify_between()
		.gap_3()
		.text_size(gpui::px(SETTINGS_LABEL_SIZE))
		.text_color(ui_theme::rgb(TEXT))
		.child(
			gpui::div()
				.min_w_0()
				.flex()
				.flex_col()
				.gap(gpui::px(2.))
				.child(label)
				.when(matches!(presentation.tone, Tone::Warning | Tone::Error), |name| {
					name.child(ui_theme::settings_description(presentation.detail))
				}),
		)
		.child(
			gpui::div()
				.flex_none()
				.flex()
				.items_center()
				.gap(gpui::px(6.))
				.text_color(ui_theme::rgb(if presentation.tone == Tone::Neutral {
					ui_theme::TEXT_MUTED
				} else {
					TEXT
				}))
				.child(
					gpui::div()
						.size(gpui::px(5.))
						.flex_none()
						.rounded_full()
						.bg(ui_theme::rgb(presentation.tone.color())),
				)
				.child(presentation.label),
		)
		.into_any_element()
}

fn component_section(
	id: &'static str,
	title: &'static str,
	index_offset: usize,
	components: &[DoctorComponent],
	report: Option<&DoctorReport>,
) -> AnyElement {
	let rows = components.iter().copied().enumerate().map(|(index, component)| {
		let status = report.and_then(|report| report.check(component)).map(|check| check.status);

		component_row(index_offset + index, component, status)
	});

	gpui::div()
		.id(id)
		.debug_selector(move || id.into())
		.flex()
		.flex_col()
		.gap_2()
		.child(ui_theme::settings_section_title(title))
		.child(
			gpui::div()
				.id(("health-section-list", index_offset))
				.role(Role::List)
				.aria_label(title)
				.grid()
				.grid_cols(2)
				.gap_x(gpui::px(32.))
				.children(rows),
		)
		.into_any_element()
}

fn control(
	id: &'static str,
	label: &'static str,
	cx: &Context<Shell>,
	action: impl Fn(&mut Shell, &mut Context<Shell>) + 'static,
) -> Stateful<Div> {
	let action = Rc::new(action);
	let click = action.clone();

	ui_theme::settings_button(id, label)
		.on_click(cx.listener(move |shell, _, _, cx| {
			click(shell, cx);

			cx.notify();
		}))
		.on_key_down(cx.listener(move |shell, event: &KeyDownEvent, _, cx| {
			if !event.is_held && ["enter", "space"].contains(&event.keystroke.key.as_str()) {
				action(shell, cx);

				cx.notify();
				cx.stop_propagation();
			}
		}))
}
