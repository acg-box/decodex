//! People, their contributions, and the files they cited in the selected round.
use super::*;
use crate::ui_cloudlet::Portrait;
use std::path::PathBuf;

impl AgentSurface {
	pub(super) fn team_grid(
		&self,
		work: &str,
		thread: &str,
		turn: &str,
		members: &BTreeMap<String, Participant>,
		cx: &mut Context<Self>,
	) -> Div {
		let prefix = shared_assignment_prefix(members.values().map(|m| m.assignment.as_str()));
		let mut team = gpui::div().w_full().flex().flex_wrap().gap(gpui::px(6.));
		for (agent, member) in members {
			let key = format!("contributor-{turn}-{agent}");
			let page = ReferencePage::Participant {
				thread: thread.into(),
				turn: turn.into(),
				agent: agent.clone(),
			};
			let (status, icon, ink) = match member.state {
				ParticipantState::Replied => ("Idle", Symbol::Document, ui_theme::SUCCESS),
				ParticipantState::Working => ("Running", Symbol::WorkActivity, BLUE),
				ParticipantState::NeedsApproval => ("Approval", Symbol::Shield, ui_theme::WARNING),
				ParticipantState::NeedsInput =>
					("Input needed", Symbol::BellAttention, ui_theme::WARNING),
				ParticipantState::Failed | ParticipantState::Stopped =>
					("Stopped", Symbol::BellError, ui_theme::ERROR),
				ParticipantState::Assigned | ParticipantState::Finished =>
					("Idle", Symbol::Agents, SECONDARY),
			};
			let assignment = member.assignment.strip_prefix(prefix).unwrap_or(&member.assignment);
			let action = if member.state == ParticipantState::Replied {
				"Read report"
			} else {
				member.state.short_label()
			};
			let label = if assignment.is_empty() {
				member.name.clone()
			} else {
				activity::wording::purpose(assignment)
			};
			let subtitle =
				if label == member.name { action.to_owned() } else { member.name.clone() };
			let accessible = if label == member.name {
				format!("{label}. {action}")
			} else {
				format!("{}. {label}. {action}", member.name)
			};
			let tip = if member.state == ParticipantState::Replied {
				activity::wording::message(&member.reply)
			} else {
				member.state.short_label().to_owned()
			};
			team = team.child(
				self.reference_control(
					key,
					accessible,
					move |s, cx| s.show_reference(page.clone(), cx),
					cx,
				)
				.relative()
				.flex_col()
				.items_start()
				.justify_start()
				.flex_1()
				.flex_basis(gpui::px(82.))
				.min_w(gpui::px(76.))
				.max_w(gpui::px(138.))
				.rounded(gpui::px(ui_theme::NAVIGATION_ROW_RADIUS))
				.px(gpui::px(9.))
				.py(gpui::px(7.))
				.gap(gpui::px(1.))
				.bg(ui_theme::rgb(SECONDARY).opacity(0.065))
				.child(
					gpui::div()
						.w_full()
						.flex()
						.items_center()
						.justify_between()
						.child(Portrait {
							seed: format!("native:{work}:{agent}"),
							status,
							edge: 28.,
						})
						.child(
							gpui::div()
								.text_color(ui_theme::rgb(ink))
								.child(workspace_symbols::icon_sized(icon, 12.)),
						),
				)
				.child(
					gpui::div()
						.w_full()
						.line_clamp(2)
						.text_ellipsis()
						.line_height(gpui::px(16.))
						.child(label),
				)
				.child(
					gpui::div()
						.w_full()
						.truncate()
						.font_weight(FontWeight::NORMAL)
						.text_size(gpui::px(ui_theme::NAVIGATION_DETAIL_SIZE))
						.text_color(ui_theme::rgb(SECONDARY))
						.child(subtitle),
				)
				.aria_description(tip.clone())
				.hover_hint(tip)
				.smooth(),
			);
		}
		team
	}

	pub(super) fn participant_reader(
		&self,
		turn: &str,
		agent: &str,
		member: &Participant,
		source: Option<&Row>,
	) -> (&'static str, Div) {
		let report = member.state == ParticipantState::Replied;
		let mut body = reading_body().child(
			gpui::div()
				.w_full()
				.flex()
				.items_center()
				.justify_between()
				.child(reader_title(&member.name))
				.when(report, |head| {
					head.child(markdown::copy_button(
						&format!("copy-report-{turn}-{agent}"),
						"Copy report",
						member.reply.clone(),
					))
				}),
		);
		if report {
			body = body.child(markdown::render(&member.reply, &format!("report-{turn}-{agent}")));
			("Agent report", body)
		} else {
			body = body.child(note(member.state.short_label()));
			if let Some(row) = source.filter(|_| {
				matches!(
					member.state,
					ParticipantState::Working
						| ParticipantState::NeedsInput
						| ParticipantState::NeedsApproval
				)
			}) {
				let (state, summary) = self.work_activity_summary(row);
				if !summary.is_empty() && summary != state && summary != member.task {
					body = body.child(section(&state)).child(note(&summary));
				}
			}
			let assignment = member
				.task
				.strip_prefix("[Work: ")
				.and_then(|text| text.split_once("]\n"))
				.map_or(member.task.as_str(), |(_, text)| text);
			if !assignment.trim().is_empty() && assignment != member.name {
				body = body
					.child(section("Assignment"))
					.child(markdown::render(assignment, &format!("assignment-{turn}-{agent}")));
			}
			if !member.reply.trim().is_empty() {
				body = body
					.child(markdown::render(&member.reply, &format!("activity-{turn}-{agent}")));
			}
			("Agent activity", body)
		}
	}

	pub(super) fn referenced_file(
		&self,
		path: PathBuf,
		authors: &BTreeSet<String>,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let name = path.file_name().unwrap_or(path.as_os_str()).to_string_lossy().into_owned();
		let key = format!("cited-file-{}", path.display());
		let readable = Self::is_readable_report(&path);
		let action = if readable {
			format!("Open {name}")
		} else if cfg!(target_os = "macos") {
			format!("Show {name} in Finder")
		} else {
			format!("Show {name} in folder")
		};
		let provenance =
			format!("Cited by {}", authors.iter().cloned().collect::<Vec<_>>().join(", "));
		let tip = format!("{provenance}\n{}", path.display());
		self.reference_control(
			key,
			action,
			move |s, cx| {
				if readable {
					s.open_reference_report(path.clone(), cx);
				} else {
					cx.reveal_path(&path);
				}
			},
			cx,
		)
		.w_full()
		.min_h(gpui::px(30.))
		.justify_start()
		.px(gpui::px(4.))
		.bg(gpui::transparent_black())
		.gap(gpui::px(8.))
		.child(
			gpui::div()
				.text_color(ui_theme::rgb(BLUE))
				.child(workspace_symbols::icon_sized(Symbol::Document, 15.)),
		)
		.child(gpui::div().flex_1().min_w_0().truncate().child(name))
		.child(gpui::div().text_color(ui_theme::rgb(SECONDARY)).child(
			workspace_symbols::icon_sized(
				if readable { Symbol::Forward } else { Symbol::NavigationFolder },
				12.,
			),
		))
		.aria_description(provenance)
		.hover_hint(tip)
		.smooth()
		.into_any_element()
	}
}

/// Native assignments often share the conversation name. Keep the distinguishing words.
fn shared_assignment_prefix<'a>(mut labels: impl Iterator<Item = &'a str>) -> &'a str {
	let Some(first) = labels.next() else { return "" };
	let Some(second) = labels.next() else { return "" };
	let mut prefix = first;
	for label in std::iter::once(second).chain(labels) {
		let words = prefix
			.split_inclusive(' ')
			.zip(label.split_inclusive(' '))
			.take_while(|(a, b)| a == b && a.ends_with(' '))
			.map(|(word, _)| word.len())
			.sum::<usize>();
		prefix = &first[..words];
	}
	prefix
}
