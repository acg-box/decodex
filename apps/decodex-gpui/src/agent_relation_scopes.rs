//! Workspace membership is visible on the canvas, without hiding cross-workspace edges.
use super::*;

pub(super) struct Scope {
	pub id: Option<String>,
	pub title: String,
	pub count: usize,
	pub bounds: [f32; 4],
}

impl Graph {
	pub(super) fn pack_workspaces(&mut self) {
		if !self.nodes.iter().any(|n| n.row.as_ref().is_some_and(|r| r.workspace.is_some())) {
			return;
		}
		// Cross-workspace children must not stretch their parent's local scope.
		let mut parents: Vec<_> = (0..self.nodes.len()).collect();
		parents.sort_by(|a, b| self.nodes[*b].x.total_cmp(&self.nodes[*a].x));
		for i in parents {
			let Some(row) = &self.nodes[i].row else { continue };
			let local: Vec<_> = self
				.edges
				.iter()
				.filter(|e| {
					e.from == self.nodes[i].key && matches!(e.kind, Kind::Assigned | Kind::Spawned)
				})
				.filter_map(|e| self.nodes.iter().find(|n| n.key == e.to))
				.filter(|n| n.row.as_ref().is_some_and(|r| r.workspace == row.workspace))
				.map(|n| n.y)
				.collect();
			if !local.is_empty() {
				self.nodes[i].y = (local.iter().copied().fold(f32::INFINITY, f32::min)
					+ local.iter().copied().fold(f32::NEG_INFINITY, f32::max))
					/ 2.;
			}
		}
		let mut groups: BTreeMap<Option<String>, Vec<usize>> = BTreeMap::new();
		for (i, node) in self.nodes.iter().enumerate() {
			if let Some(row) = &node.row {
				groups.entry(row.workspace.clone()).or_default().push(i);
			}
		}
		let mut top = 48.;
		for members in groups.values() {
			let min_y = members.iter().map(|i| self.nodes[*i].y).fold(f32::INFINITY, f32::min);
			let mut bottom = top;
			for i in members {
				let node = &mut self.nodes[*i];
				node.y += top - min_y;
				bottom = bottom.max(node.y + node.height());
			}
			top = snap(bottom + 96.);
		}
	}

	pub(super) fn scope_bounds(&mut self, workspaces: &[decodex_protocol::WorkspaceDto]) {
		if !self.nodes.iter().any(|n| n.row.as_ref().is_some_and(|r| r.workspace.is_some())) {
			return;
		}
		for node in &self.nodes {
			let Some(row) = &node.row else { continue };
			let bounds = [
				((node.x - GRID) / GRID).floor() * GRID,
				((node.y - 2. * GRID) / GRID).floor() * GRID,
				((node.x + 224. + GRID) / GRID).ceil() * GRID,
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
