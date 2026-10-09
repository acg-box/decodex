//! A selected connection explains one fact and links to its source.
use super::{
	AgentSurface, AnyElement, Context, Edge, GREEN, InteractiveElement, IntoElement, Kind,
	ParentElement, Role, StatefulInteractiveElement, Styled, TEXT_MUTED,
};

impl AgentSurface {
	pub(in super::super) fn relation_details(
		&self,
		edge: &Edge,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let graph = self.relation_graph();
		let title = |key: &str| {
			graph
				.nodes
				.iter()
				.find(|n| n.key == key)
				.map(|n| n.title.clone())
				.unwrap_or_else(|| key.into())
		};
		let from = title(&edge.from);
		let to = title(&edge.to);
		let summary = format!("{from} → {to}");
		let related: Vec<_> = graph.edges.iter().filter(|other| edge.same_pair(other)).collect();
		let contents: Vec<_> = related
			.iter()
			.filter(|fact| fact.excerpt.as_ref().is_some_and(|text| !text.trim().is_empty()))
			.collect();
		let mut body = gpui::div()
			.flex()
			.flex_col()
			.gap_3()
			.p_3()
			.w_full()
			.min_w_0()
			.text_size(gpui::px(12.))
			.child(gpui::div().text_size(gpui::px(14.)).child(summary));
		for fact in &contents {
			let heading = match fact.kind {
				Kind::Spawned | Kind::Assigned => "Task",
				Kind::Returned => "Result",
				Kind::Context => "Context",
				_ => "Message",
			};
			body = body.child(
				gpui::div()
					.flex()
					.flex_col()
					.gap_1()
					.child(
						gpui::div()
							.text_size(gpui::px(11.))
							.text_color(gpui::rgb(fact.color))
							.child(heading),
					)
					.child(fact.excerpt.clone().unwrap_or_default()),
			);
		}
		if matches!(edge.kind, Kind::Assigned | Kind::Spawned)
			&& contents.is_empty()
			&& let Some(row) =
				graph.nodes.iter().find(|n| n.key == edge.to).and_then(|n| n.row.as_ref())
			&& row.title != to
		{
			body = body.child(gpui::div().text_size(gpui::px(12.)).child(row.title.clone()));
		}
		if related.iter().any(|fact| fact.kind == Kind::Returned && fact.label == "Finished") {
			body = body.child(
				gpui::div()
					.text_size(gpui::px(11.))
					.text_color(gpui::rgb(GREEN))
					.child("Completed"),
			);
		}
		if contents.is_empty() {
			let message = match edge.kind {
				Kind::Assigned | Kind::Spawned => "Task text unavailable",
				Kind::Returned => "Finished · result text unavailable",
				Kind::Context => "Shared conversation reference",
				Kind::Resource => "Attached resource",
				Kind::Dependency => "Task prerequisite",
				Kind::Wait => "Waiting for this agent",
				_ => "Interaction · message text unavailable",
			};
			body = body.child(gpui::div().text_color(gpui::rgb(TEXT_MUTED)).child(message));
		}
		body = self.relation_detail_actions(body, edge, &graph, cx);

		gpui::div()
			.w(gpui::px(300.))
			.h_full()
			.min_h_0()
			.flex_none()
			.flex()
			.flex_col()
			.border_l_1()
			.border_color(gpui::rgba(0xffffff0c))
			.child(
				gpui::div()
					.flex()
					.flex_none()
					.items_center()
					.p_2()
					.child(gpui::div().flex_1().child("Connection"))
					.child(self.workspace_action(
						"relation-close".into(),
						"×".into(),
						|s, cx| {
							s.work_board.view.edge = None;
							cx.notify();
						},
						cx,
					)),
			)
			.child(
				gpui::div()
					.id("relation-inspector-scroll")
					.h_0()
					.flex_1()
					.min_h_0()
					.overflow_y_scroll()
					.child(body),
			)
			.into_any_element()
	}

	fn relation_detail_actions(
		&self,
		mut body: gpui::Div,
		edge: &Edge,
		graph: &super::Graph,
		cx: &mut Context<Self>,
	) -> gpui::Div {
		let related = graph.edges.iter().filter(|other| edge.same_pair(other));
		let title = |key: &str| {
			graph
				.nodes
				.iter()
				.find(|n| n.key == key)
				.map(|n| n.title.clone())
				.unwrap_or_else(|| key.into())
		};
		if let Some((work, thread)) = edge.source.clone() {
			body = body.child(self.workspace_action(
				"relation-source".into(),
				format!(
						"View {} ↗",
						graph
							.nodes
							.iter()
							.find(|n| n.row.as_ref().is_some_and(
								|r| r.work == work && r.thread.as_ref() == Some(&thread)
							))
							.map(|n| n.title.as_str())
							.unwrap_or("conversation")
					),
				move |s, cx| {
					if s.snapshot.as_ref().is_some_and(|snap| {
						snap.work_items
							.iter()
							.any(|w| w.id == work && w.codex_thread_id.as_ref() == Some(&thread))
					}) {
						s.workspace.graph_expanded = false;
						s.open_page(&work, cx);
					} else {
						cx.open_url(&format!("codex://threads/{thread}"));
					}
				},
				cx,
			));
		}
		if let Some(url) = edge.link.clone() {
			body = body.child(self.workspace_action(
				"relation-resource-link".into(),
				"Open resource".into(),
				move |_, cx| cx.open_url(&url),
				cx,
			));
		}
		let toggle =
			if self.work_board.view.show_record { "Hide event records" } else { "Event records" };
		body = body.child(
			gpui::div()
				.id("relation-record-toggle")
				.role(Role::Button)
				.aria_label(toggle)
				.tab_index(0)
				.w_full()
				.flex_none()
				.cursor_pointer()
				.py_2()
				.on_click(cx.listener(|s, _, _, cx| {
					s.work_board.view.show_record = !s.work_board.view.show_record;
					cx.notify();
				}))
				.on_key_down(cx.listener(|s, e: &gpui::KeyDownEvent, _, cx| {
					if ["enter", "space"].contains(&e.keystroke.key.as_str()) {
						s.work_board.view.show_record = !s.work_board.view.show_record;
						cx.stop_propagation();
						cx.notify();
					}
				}))
				.child(toggle),
		);
		if self.work_board.view.show_record {
			for fact in related {
				body = body.child(gpui::div().w_full().min_w_0().text_size(gpui::px(11.)).child(
					format!(
						"{} → {} · {}\n{}",
						title(&fact.from),
						title(&fact.to),
						fact.label,
						fact.detail
					),
				));
			}
		}
		body
	}
}
