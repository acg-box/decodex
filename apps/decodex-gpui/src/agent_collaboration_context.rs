//! Shared sources and discussions for the inspected conversation or graph agent.
use super::*;
use crate::ui_cloudlet::Portrait;
use std::path::PathBuf;

impl AgentSurface {
	/// Keep scope in native identities. Names only label the source links.
	pub(super) fn round_evidence(
		&self,
		work: &str,
		round: (&str, &str),
		topics: &[Vec<Post>],
		members: &BTreeMap<String, Participant>,
		selected_agent: Option<&str>,
		cx: &mut Context<Self>,
	) -> Option<Div> {
		let (thread, turn) = round;
		let mut files: BTreeMap<PathBuf, Vec<&str>> = BTreeMap::new();
		let mut other = Vec::new();
		for (id, member) in members.iter().filter(|(_, m)| m.state == ParticipantState::Replied) {
			let paths: BTreeSet<_> = markdown::referenced_files(&member.reply).collect();
			if paths.is_empty() && selected_agent.is_none_or(|agent| agent == id) {
				other.push((id, member));
			}
			for path in paths {
				files.entry(path).or_default().push(id);
			}
		}
		files.retain(|_, agents| selected_agent.is_none_or(|agent| agents.contains(&agent)));
		let mut body = reading_body();
		let evidence = !files.is_empty();
		if evidence {
			body = body.child(section("Cited files"));
		}
		for (path, ids) in files {
			let authors = ids.iter().map(|id| members[*id].name.clone()).collect();
			let file = self.referenced_file(path.clone(), &authors, cx);
			let mut byline = gpui::div().flex().flex_wrap().items_center().gap(gpui::px(4.));
			for id in ids {
				let member = &members[id];
				let page = ReferencePage::Participant {
					thread: thread.into(),
					turn: turn.into(),
					agent: id.into(),
				};
				let label = format!(
					"Read {}’s report citing {}",
					member.name,
					path.file_name().unwrap_or_default().to_string_lossy()
				);
				byline = byline.child(
					self.reference_control(
						format!("citation-{}-{id}", path.display()),
						label,
						move |s, cx| s.show_reference(page.clone(), cx),
						cx,
					)
					.gap(gpui::px(3.))
					.px(gpui::px(4.))
					.min_h(gpui::px(25.))
					.rounded(gpui::px(5.))
					.bg(gpui::transparent_black())
					.text_size(gpui::px(ui_theme::NAVIGATION_DETAIL_SIZE))
					.child(Portrait {
						seed: format!("native:{work}:{id}"),
						status: "Idle",
						edge: 15.,
					})
					.child(gpui::div().text_color(ui_theme::rgb(BLUE)).child(member.name.clone()))
					.child(
						gpui::div()
							.text_color(ui_theme::rgb(BLUE))
							.child(workspace_symbols::icon_sized(Symbol::Forward, 11.)),
					)
					.hover_hint(format!("Read {}’s report", member.name))
					.smooth(),
				);
			}
			body = body.child(
				gpui::div()
					.w_full()
					.min_w_0()
					.flex()
					.flex_col()
					.rounded(gpui::px(ui_theme::NAVIGATION_ROW_RADIUS))
					.px(gpui::px(6.))
					.py(gpui::px(4.))
					.bg(ui_theme::rgb(SECONDARY).opacity(0.065))
					.child(file)
					.child(
						gpui::div()
							.h(gpui::px(1.))
							.mx(gpui::px(4.))
							.bg(ui_theme::rgb(SECONDARY).opacity(0.13)),
					)
					.child(
						gpui::div()
							.px(gpui::px(4.))
							.pt(gpui::px(4.))
							.text_size(gpui::px(ui_theme::NAVIGATION_DETAIL_SIZE))
							.text_color(ui_theme::rgb(SECONDARY))
							.child("Cited by"),
					)
					.child(byline),
			);
		}
		if other.is_empty() {
			return evidence.then_some(body);
		}
		let outcome = self.collaboration_outcome(thread, turn).map(|(_, text, _)| text);
		other.retain(|(id, member)| {
			outcome.is_none_or(|text| !same_excerpt(text, &member.reply))
				&& !topics.iter().flatten().any(|post| {
					post.posted_by_thread.as_ref() == Some(*id)
						&& !post.truncated
						&& same_excerpt(&post.text, &member.reply)
				})
		});
		if other.is_empty() {
			return evidence.then_some(body);
		}
		body = body.child(index_section("Agent reports", evidence));
		for (id, member) in other {
			let page = ReferencePage::Participant {
				thread: thread.into(),
				turn: turn.into(),
				agent: id.clone(),
			};
			body = body.child(self.reference_row(
				format!("contributor-{turn}-{id}"),
				activity::wording::message(&member.reply),
				member.name.clone(),
				Some(Symbol::ActivityHandoff),
				move |s, cx| s.show_reference(page.clone(), cx),
				cx,
			));
		}
		Some(body)
	}

	pub(super) fn agent_context(&self, row: &Row, rows: &[Row], cx: &mut Context<Self>) -> Div {
		let mut body = reading_body().child(
			gpui::div()
				.flex()
				.items_center()
				.gap(gpui::px(7.))
				.child(Portrait { seed: row.key.clone(), status: "Idle", edge: 24. })
				.child(reader_title(participant_name(row))),
		);
		let Some(agent) = row.thread.as_deref() else { return body };
		let root =
			self.conversation_record(&row.work).and_then(|work| work.codex_thread_id.as_deref());
		let parent = self
			.native_agents
			.lists
			.get(&row.work)
			.and_then(|agents| agents.iter().find(|a| a.thread_id == agent))
			.map(|a| a.parent_thread_id.as_str());
		let owner = parent.unwrap_or(agent);
		let turn = self.timeline.native.thread_items(owner).rev().find_map(|item| match item {
			AgentTimelineContent::Item { turn_id, collaboration: Some(call), .. }
				if call.sender_thread_id == owner
					&& (agent == owner
						|| call.receiver_thread_ids.iter().any(|id| id == agent)) =>
				Some(turn_id.as_str()),
			_ => None,
		});
		let origin = root.unwrap_or(owner);
		let mut topics = self.board_topics(&row.work, origin, None, rows);
		if origin != agent {
			topics
				.retain(|posts| posts.iter().any(|p| p.posted_by_thread.as_deref() == Some(agent)));
		}
		let evidence = turn.and_then(|turn| {
			self.round_evidence(
				&row.work,
				(owner, turn),
				&topics,
				&self.turn_participants(owner, turn, rows),
				(agent != owner).then_some(agent),
				cx,
			)
		});
		let mut populated = evidence.is_some();
		body = body.children(evidence);
		if !populated && let Some(output) = self.work_output(row) {
			let files: BTreeSet<_> = markdown::referenced_files(output.text).collect();
			if !files.is_empty() {
				populated = true;
				body = body.child(section("Cited files"));
				let authors = BTreeSet::from([participant_name(row).to_string()]);
				for path in files {
					body = body.child(self.referenced_file(path, &authors, cx));
				}
			}
		}
		if !topics.is_empty() {
			body = body
				.child(index_section("Board topics", populated))
				.child(self.topic_rows(&row.work, origin, None, &topics, cx));
			populated = true;
		}
		let peer = |r: &Row| r.thread.as_deref() == Some(agent) && r.work == row.work;
		let exchanges = self.collaboration_cases(&[], rows);
		let mut related = exchanges
			.iter()
			.filter(|case| {
				case.source_event.is_some()
					&& case.to.as_ref().is_some_and(|to| peer(&case.from) || peer(to))
			})
			.peekable();
		if related.peek().is_some() {
			body = body.child(index_section("Exchanges", populated));
			for case in related {
				body = body.child(self.exchange_row(case, peer(&case.from), cx));
			}
			populated = true;
		}
		if !populated {
			body = body.child(note("No cited files or shared discussions yet."));
		}
		body
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
