//! Workspace membership is visible on the canvas, without hiding cross-workspace edges.
use gpui::{AnyElement, IntoElement as _, PathBuilder, Styled as _};

use crate::{
	shell::agent_surface::work_board::relations::{GRID, Graph},
	ui_motion, ui_theme,
};
use decodex_protocol::ProjectDto;

pub(super) struct Scope {
	pub id: Option<String>,
	pub title: String,
	pub count: usize,
	pub bounds: [f32; 4],
}

impl Scope {
	pub(super) fn contains(&self, graph: &Graph, key: &str) -> bool {
		graph.nodes.iter().any(|node| {
			node.key == key && node.row.as_ref().is_some_and(|row| row.workspace == self.id)
		})
	}

	pub(super) fn label(&self) -> String {
		let title = if self.id.is_some() {
			format!("Project: {}", self.title)
		} else {
			"No project".into()
		};

		format!("{title} · {} {}", self.count, if self.count == 1 { "agent" } else { "agents" })
	}

	/// A quiet dashed boundary distinguishes project membership from solid agent links.
	pub(super) fn outline(&self, scale: f32, selected: bool) -> AnyElement {
		let id = format!("project-scope-{}", self.id.as_deref().unwrap_or("unassigned"));

		gpui::canvas(
			|_, _, _| (),
			move |bounds, _, window, cx| {
				let active = ui_motion::value(id.clone(), u8::from(selected) as f32, window, cx);
				let radius = (18. * scale).clamp(4., 18.);
				let left = 0.5;
				let top = 0.5;
				let right = f32::from(bounds.size.width) - 0.5;
				let bottom = f32::from(bounds.size.height) - 0.5;
				let point = |x, y| bounds.origin + gpui::point(gpui::px(x), gpui::px(y));
				let mut path =
					PathBuilder::stroke(gpui::px(1.)).dash_array(&[gpui::px(4.), gpui::px(5.)]);

				path.move_to(point(left + radius, top));
				path.line_to(point(right - radius, top));
				path.curve_to(point(right, top + radius), point(right, top));
				path.line_to(point(right, bottom - radius));
				path.curve_to(point(right - radius, bottom), point(right, bottom));
				path.line_to(point(left + radius, bottom));
				path.curve_to(point(left, bottom - radius), point(left, bottom));
				path.line_to(point(left, top + radius));
				path.curve_to(point(left + radius, top), point(left, top));

				if let Ok(path) = path.build() {
					let color = ui_theme::rgb(ui_theme::TEXT_MUTED)
						.blend(ui_theme::rgb(ui_theme::BLUE).opacity(active));

					window.paint_path(path, color.opacity(0.24 + active * 0.42));
				}
			},
		)
		.absolute()
		.inset_0()
		.size_full()
		.into_any_element()
	}
}

impl Graph {
	pub(super) fn scope_bounds(&mut self, projects: &[ProjectDto]) {
		if !self.nodes.iter().any(|n| n.row.as_ref().is_some_and(|r| r.workspace.is_some())) {
			return;
		}

		// A root that coordinates across projects belongs above those groups.
		// Including it in "No Project" would stretch that frame across all branches.
		let coordinator = self
			.nodes
			.iter()
			.find(|node| {
				node.row.as_ref().is_some_and(|row| row.workspace.is_none())
					&& !self.edges.iter().any(|edge| edge.kind.hierarchy() && edge.to == node.key)
					&& self.edges.iter().any(|edge| {
						edge.kind.hierarchy()
							&& edge.from == node.key
							&& self.nodes.iter().any(|child| {
								child.key == edge.to
									&& child.row.as_ref().is_some_and(|row| row.workspace.is_some())
							})
					})
			})
			.map(|node| node.key.as_str());

		for node in &self.nodes {
			if coordinator == Some(node.key.as_str()) {
				continue;
			}

			let Some(row) = &node.row else { continue };
			let bounds = [
				((node.x - GRID) / GRID).floor() * GRID,
				((node.y - 2. * GRID) / GRID).floor() * GRID,
				((node.x + node.width() + GRID) / GRID).ceil() * GRID,
				((node.y + node.height() + GRID) / GRID).ceil() * GRID,
			];

			if let Some(scope) = self.scopes.iter_mut().find(|g| g.id == row.workspace) {
				scope.bounds[0] = scope.bounds[0].min(bounds[0]);
				scope.bounds[1] = scope.bounds[1].min(bounds[1]);
				scope.bounds[2] = scope.bounds[2].max(bounds[2]);
				scope.bounds[3] = scope.bounds[3].max(bounds[3]);
				scope.count += 1;
			} else {
				let title = row
					.workspace
					.as_ref()
					.and_then(|id| projects.iter().find(|w| &w.id == id))
					.map(|w| w.name.clone())
					.unwrap_or_else(|| "No Project".into());

				self.scopes.push(Scope { id: row.workspace.clone(), title, count: 1, bounds });
			}
		}
	}
}
