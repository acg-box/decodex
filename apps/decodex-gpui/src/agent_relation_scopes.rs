//! Workspace membership is visible on the canvas, without hiding cross-workspace edges.
use super::{BTreeMap, GRID, Graph, Kind, snap};

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
		let mut groups: BTreeMap<Option<String>, Vec<usize>> = BTreeMap::new();
		for (i, node) in self.nodes.iter().enumerate() {
			if let Some(row) = &node.row {
				groups.entry(row.workspace.clone()).or_default().push(i);
			}
		}
		let children = {
			let indices: BTreeMap<_, _> =
				self.nodes.iter().enumerate().map(|(i, n)| (n.key.as_str(), i)).collect();
			let mut children = vec![Vec::new(); self.nodes.len()];
			for edge in &self.edges {
				if !matches!(edge.kind, Kind::Assigned | Kind::Spawned) {
					continue;
				}
				let (Some(&from), Some(&to)) =
					(indices.get(edge.from.as_str()), indices.get(edge.to.as_str()))
				else {
					continue;
				};
				if let (Some(a), Some(b)) = (&self.nodes[from].row, &self.nodes[to].row)
					&& a.workspace == b.workspace
				{
					children[from].push(to);
				}
			}
			children
		};
		// Reuse the ownership order, but remove rows occupied by other workspaces.
		// Compact local leaves first, then center their parents from the bottom up.
		for members in groups.values_mut() {
			members.sort_by(|a, b| self.nodes[*a].y.total_cmp(&self.nodes[*b].y));
			let mut cursor = 0.;
			for &i in members.iter().filter(|i| children[**i].is_empty()) {
				self.nodes[i].y = cursor;
				cursor += 96.;
			}
		}
		let mut parents: Vec<_> =
			(0..self.nodes.len()).filter(|i| !children[*i].is_empty()).collect();
		parents.sort_by(|a, b| self.nodes[*b].x.total_cmp(&self.nodes[*a].x));
		for i in parents {
			let ys = || children[i].iter().map(|child| self.nodes[*child].y);
			self.nodes[i].y =
				(ys().fold(f32::INFINITY, f32::min) + ys().fold(f32::NEG_INFINITY, f32::max)) / 2.;
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
