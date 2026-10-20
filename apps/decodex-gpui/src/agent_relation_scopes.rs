//! Workspace membership is visible on the canvas, without hiding cross-workspace edges.
use super::{GRID, Graph};

pub(super) struct Scope {
	pub id: Option<String>,
	pub title: String,
	pub count: usize,
	pub bounds: [f32; 4],
}

impl Graph {
	pub(super) fn scope_bounds(&mut self, workspaces: &[decodex_protocol::WorkspaceDto]) {
		if !self.nodes.iter().any(|n| n.row.as_ref().is_some_and(|r| r.workspace.is_some())) {
			return;
		}
		// A root that coordinates across workspaces belongs above those groups.
		// Including it in "No workspace" would stretch that frame across all branches.
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
					.and_then(|id| workspaces.iter().find(|w| &w.id == id))
					.map(|w| w.name.clone())
					.unwrap_or_else(|| "No workspace".into());
				self.scopes.push(Scope { id: row.workspace.clone(), title, count: 1, bounds });
			}
		}
	}
}
