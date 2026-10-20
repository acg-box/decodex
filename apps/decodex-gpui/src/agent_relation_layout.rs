//! Compact ownership trees: complete branches share space and keep their source relationships.
use super::{BTreeMap, FRAME_INSET, Graph};

const WIDTH: f32 = super::card::WIDTH;
const SIBLING_GAP: f32 = 24.;
const COLUMN_GAP: f32 = 40.;
const ROW_GAP: f32 = 36.;
const WORKSPACE_GAP: f32 = 40.;

#[derive(Default)]
pub(super) struct Cache {
	input: Input,
	positions: Vec<(usize, f32, f32)>,
	horizontal: Option<bool>,
}

#[derive(PartialEq)]
struct Input {
	nodes: Vec<(String, Option<String>, u32)>,
	edges: Vec<(usize, usize)>,
	viewport: (f32, f32),
	width: f32,
	spacing: f32,
	column_gap: f32,
}
impl Default for Input {
	fn default() -> Self {
		Self {
			nodes: vec![],
			edges: vec![],
			viewport: (0., 0.),
			width: WIDTH,
			spacing: 1.,
			column_gap: COLUMN_GAP,
		}
	}
}

struct Tree {
	index: usize,
	children: Vec<Self>,
	leaves: usize,
}

#[derive(Clone, Default)]
struct Block {
	width: f32,
	height: f32,
	positions: Vec<(usize, f32, f32)>,
}

impl Cache {
	pub(super) fn place(&mut self, graph: &mut Graph, viewport: (f32, f32)) {
		let indices: BTreeMap<_, _> =
			graph.nodes.iter().enumerate().map(|(i, n)| (n.key.as_str(), i)).collect();
		let label_gap = graph
			.connections()
			.iter()
			.filter(|edge| edge.kind.hierarchy())
			.map(|edge| graph.connection_label(edge).width + 16.)
			.fold(0., f32::max);
		let input = Input {
			nodes: graph
				.nodes
				.iter()
				.map(|n| {
					(
						n.key.clone(),
						n.row.as_ref().and_then(|r| r.workspace.clone()),
						n.height().to_bits(),
					)
				})
				.collect(),
			edges: graph
				.edges
				.iter()
				.filter(|e| e.kind.hierarchy())
				.filter_map(|e| {
					Some((*indices.get(e.from.as_str())?, *indices.get(e.to.as_str())?))
				})
				.collect(),
			viewport,
			width: graph.nodes.iter().map(|node| node.width()).fold(WIDTH, f32::max),
			// Labels and cards share world space, so zoom never changes the layout.
			spacing: 1.,
			column_gap: COLUMN_GAP.max(label_gap),
		};
		if self.input != input {
			// Keep the reading direction when only activity or measurements change.
			let stable = self.input.viewport == input.viewport
				&& self.input.edges == input.edges
				&& self.input.nodes.len() == input.nodes.len()
				&& self.input.nodes.iter().zip(&input.nodes).all(|(a, b)| a.0 == b.0 && a.1 == b.1);
			let (positions, horizontal) =
				input.arrange_oriented(self.horizontal.filter(|_| stable));
			self.positions = positions;
			self.horizontal = Some(horizontal);
			self.input = input;
		}
		for &(index, x, y) in &self.positions {
			graph.nodes[index].x = x + 32.;
			graph.nodes[index].y = y + 48.;
		}
	}
}

impl Input {
	fn forest(&self) -> Vec<Vec<Tree>> {
		let mut groups: BTreeMap<_, Vec<usize>> = BTreeMap::new();
		for (i, (key, scope, _)) in self.nodes.iter().enumerate() {
			if !key.starts_with("resource:") {
				groups.entry(scope).or_default().push(i);
			}
		}
		let mut children = vec![Vec::new(); self.nodes.len()];
		let mut incoming = vec![false; self.nodes.len()];
		for &(from, to) in &self.edges {
			if from != to && self.nodes[from].1 == self.nodes[to].1 {
				children[from].push(to);
				incoming[to] = true;
			}
		}
		for children in &mut children {
			children.sort_unstable();
			children.dedup();
		}
		let mut visited = vec![false; self.nodes.len()];
		groups
			.values()
			.map(|members| {
				let mut trees = Vec::new();
				// Visit roots first, then retain each isolated cycle once.
				for roots in [true, false] {
					for &index in members {
						if !visited[index] && (!roots || !incoming[index]) {
							trees.push(Tree::visit(index, &children, &mut visited));
						}
					}
				}
				trees
			})
			.collect()
	}

	fn coordinator(&self, forest: &[Vec<Tree>]) -> Option<usize> {
		if forest.len() < 2 {
			return None;
		}
		forest.iter().enumerate().find_map(|(index, trees)| {
			let root = trees.first()?;
			(trees.len() == 1
				&& self.nodes[root.index].1.is_none()
				&& forest
					.iter()
					.enumerate()
					.filter(|(other, _)| *other != index)
					.flat_map(|(_, trees)| trees)
					.all(|child| self.edges.contains(&(root.index, child.index))))
			.then_some(index)
		})
	}

	#[cfg(test)]
	fn arrange(&self) -> Vec<(usize, f32, f32)> {
		self.arrange_oriented(None).0
	}

	fn arrange_oriented(&self, direction: Option<bool>) -> (Vec<(usize, f32, f32)>, bool) {
		let mut forest = self.forest();
		let coordinator = self.coordinator(&forest).map(|group| {
			let tree = forest[group].remove(0);
			forest[group] = tree.children;
			tree.index
		});
		let mut heights =
			coordinator.map(|root| vec![f32::from_bits(self.nodes[root].2)]).unwrap_or_default();
		for trees in &forest {
			let depth = usize::from(coordinator.is_some());
			for tree in trees {
				tree.rank_heights(self, depth, &mut heights);
			}
		}
		let (width, height) = self.viewport;
		let tallest = heights.iter().copied().fold(1_f32, f32::max);
		let available = if width > 2. * FRAME_INSET && height > 2. * FRAME_INSET {
			(width - 2. * FRAME_INSET, height - 2. * FRAME_INSET)
		} else {
			(1152., 448.)
		};
		// Choose one reading direction for the entire graph. Never wrap siblings
		// into a different rank just to improve the fitted scale.
		[false, true]
			.into_iter()
			.filter(|horizontal| direction.is_none_or(|direction| direction == *horizontal))
			.map(|horizontal| {
				let mut rank = 0.;
				let ranks: Vec<_> = heights
					.iter()
					.map(|height| {
						let current = rank;
						rank += if horizontal {
							self.width + self.column_gap
						} else {
							height + ROW_GAP * self.spacing
						};
						current
					})
					.collect();
				let scopes = forest
					.iter()
					.filter(|trees| !trees.is_empty())
					.map(|trees| {
						let depth = usize::from(coordinator.is_some());
						let blocks: Vec<_> = trees
							.iter()
							.map(|tree| tree.layout(self, depth, &ranks, horizontal))
							.collect();
						Block::pack(blocks, SIBLING_GAP * self.spacing, available)
					})
					.collect();
				let mut block = Block::siblings(scopes, horizontal, WORKSPACE_GAP * self.spacing);
				if let Some(root) = coordinator {
					let height = f32::from_bits(self.nodes[root].2);
					let step =
						if horizontal { self.width + self.column_gap } else { height + ROW_GAP };
					block.offset(
						if horizontal { step } else { 0. },
						if horizontal { 0. } else { step },
					);
					if horizontal {
						block.width += step;
					} else {
						block.height += step;
					}
					block.positions.push((
						root,
						if horizontal { 0. } else { (block.width - self.width).max(0.) / 2. },
						if horizontal { (block.height - height).max(0.) / 2. } else { 0. },
					));
					block.width = block.width.max(self.width);
					block.height = block.height.max(height);
				}
				// Choose the direction that gives the largest fitted overview.
				let summary_width = block.width * super::card::WIDTH / self.width;
				let summary_height = block.height * super::card::HEIGHT / tallest;
				let scale = (available.0 / summary_width.max(1.))
					.min(available.1 / summary_height.max(1.))
					.min(1.);
				(block, scale, horizontal)
			})
			.max_by(|(a, sa, _), (b, sb, _)| {
				sa.total_cmp(sb).then_with(|| (b.width * b.height).total_cmp(&(a.width * a.height)))
			})
			.map_or_else(
				|| (Vec::new(), false),
				|(block, _, horizontal)| (block.positions, horizontal),
			)
	}
}

impl Tree {
	fn visit(index: usize, children: &[Vec<usize>], visited: &mut [bool]) -> Self {
		visited[index] = true;
		let mut tree = Self { index, children: Vec::new(), leaves: 1 };
		for &child in &children[index] {
			if !visited[child] {
				tree.children.push(Self::visit(child, children, visited));
			}
		}
		tree.leaves = tree.children.iter().map(|child| child.leaves).sum::<usize>().max(1);
		tree
	}

	fn rank_heights(&self, input: &Input, depth: usize, heights: &mut Vec<f32>) {
		heights.resize(heights.len().max(depth + 1), 0.);
		heights[depth] = heights[depth].max(f32::from_bits(input.nodes[self.index].2));
		for child in &self.children {
			child.rank_heights(input, depth + 1, heights);
		}
	}

	fn layout(&self, input: &Input, depth: usize, ranks: &[f32], horizontal: bool) -> Block {
		let height = f32::from_bits(input.nodes[self.index].2);
		let step = if horizontal {
			input.width + input.column_gap
		} else {
			ranks.get(depth + 1).map_or(height + ROW_GAP, |next| next - ranks[depth])
		};
		let children: Vec<_> = self
			.children
			.iter()
			.map(|child| child.layout(input, depth + 1, ranks, horizontal))
			.collect();
		let mut block = if children.len() >= 4 {
			// A wide fan-out is a group of complete branches, not one enormous
			// rank. Packing those branches preserves identity and exact edges.
			Block::pack(
				children,
				SIBLING_GAP * input.spacing,
				(
					(input.viewport.0 - if horizontal { step } else { 0. } - 2. * FRAME_INSET)
						.max(input.width),
					(input.viewport.1 - if horizontal { 0. } else { step } - 2. * FRAME_INSET)
						.max(height),
				),
			)
		} else {
			Block::siblings(children, horizontal, SIBLING_GAP * input.spacing)
		};
		if !block.positions.is_empty() {
			block.offset(if horizontal { step } else { 0. }, if horizontal { 0. } else { step });
			if horizontal {
				block.width += step;
			} else {
				block.height += step;
			}
		}
		if horizontal {
			block.height = block.height.max(height);
			block.width = block.width.max(input.width);
			block.positions.push((self.index, 0., (block.height - height) / 2.));
		} else {
			block.width = block.width.max(input.width);
			block.height = block.height.max(height);
			block.positions.push((self.index, (block.width - input.width) / 2., 0.));
		}
		block
	}
}

impl Block {
	// Compare shelf counts in both axes. This extends the existing shelf packer;
	// no graph or bin-packing runtime is needed for these small cached forests.
	fn pack(blocks: Vec<Self>, gap: f32, available: (f32, f32)) -> Self {
		let mut best: Option<(f32, f32, Self)> = None;
		for horizontal in [false, true] {
			let mut ordered = blocks.clone();
			ordered.sort_by(|a, b| {
				let extent = |block: &Self| if horizontal { block.width } else { block.height };
				extent(b).total_cmp(&extent(a))
			});
			let lengths: Vec<_> = ordered
				.iter()
				.map(|block| if horizontal { block.height } else { block.width })
				.collect();
			let minimum = lengths.iter().copied().fold(0_f32, f32::max);
			let total = lengths.iter().sum::<f32>() + gap * lengths.len().saturating_sub(1) as f32;
			for lanes in 1..=ordered.len() {
				let packed = Self::shelf(
					ordered.clone(),
					lengths.clone(),
					horizontal,
					gap,
					gap,
					(total / lanes as f32).max(minimum),
				);
				let score = (available.0 / packed.width.max(1.))
					.min(available.1 / packed.height.max(1.))
					.min(1.);
				let area = packed.width * packed.height;
				if best.as_ref().is_none_or(|(old_score, old_area, _)| {
					score.total_cmp(old_score).then_with(|| old_area.total_cmp(&area)).is_gt()
				}) {
					best = Some((score, area, packed));
				}
			}
		}
		best.map(|(_, _, packed)| packed).unwrap_or_default()
	}

	fn shelf(
		blocks: Vec<Self>,
		reference: Vec<f32>,
		horizontal: bool,
		gap: f32,
		reference_gap: f32,
		limit: f32,
	) -> Self {
		let mut result = Self::default();
		let (mut primary, mut secondary, mut extent) = (0., 0., 0_f32);
		let mut reference_primary = 0.;
		for (mut block, reference_size) in blocks.into_iter().zip(reference) {
			let size = if horizontal { block.height } else { block.width };
			if reference_primary > 0. && reference_primary + reference_size > limit {
				secondary += extent + gap;
				primary = 0.;
				reference_primary = 0.;
				extent = 0.;
			}
			let (x, y) = if horizontal { (secondary, primary) } else { (primary, secondary) };
			block.offset(x, y);
			result.width = result.width.max(x + block.width);
			result.height = result.height.max(y + block.height);
			extent = extent.max(if horizontal { block.width } else { block.height });
			primary += size + gap;
			reference_primary += reference_size + reference_gap;
			result.positions.extend(block.positions);
		}
		result
	}

	fn siblings(blocks: Vec<Self>, horizontal: bool, gap: f32) -> Self {
		let mut result = Self::default();
		let mut offset = 0.;
		for mut block in blocks {
			if !result.positions.is_empty() {
				offset += gap;
			}
			block
				.offset(if horizontal { 0. } else { offset }, if horizontal { offset } else { 0. });
			result.width = result.width.max(block.width + if horizontal { 0. } else { offset });
			result.height = result.height.max(block.height + if horizontal { offset } else { 0. });
			offset += if horizontal { block.height } else { block.width };
			result.positions.extend(block.positions);
		}
		result
	}

	fn offset(&mut self, x: f32, y: f32) {
		for (_, left, top) in &mut self.positions {
			*left += x;
			*top += y;
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn input(viewport: (f32, f32)) -> Input {
		let mut nodes = vec![("main".into(), None, 64_f32.to_bits())];
		nodes.extend((1..=18).map(|i| (format!("agent-{i}"), None, 80_f32.to_bits())));
		Input { nodes, edges: (1..=18).map(|i| (0, i)).collect(), viewport, ..Default::default() }
	}

	fn bounds(positions: &[(usize, f32, f32)], input: &Input) -> (f32, f32) {
		positions.iter().fold((0_f32, 0_f32), |(w, h), &(i, x, y)| {
			(w.max(x + WIDTH), h.max(y + f32::from_bits(input.nodes[i].2)))
		})
	}

	#[test]
	fn wide_and_tall_canvases_use_their_available_space_without_overlapping_cards() {
		let mut extents = Vec::new();
		for viewport in [(1400., 420.), (520., 1100.)] {
			let input = input(viewport);
			let positions = input.arrange();
			assert_eq!(positions.len(), input.nodes.len());
			for &(i, x, y) in &positions {
				for &(j, other_x, other_y) in &positions {
					if i == j {
						continue;
					}
					assert!(
						x + WIDTH <= other_x
							|| other_x + WIDTH <= x
							|| y + f32::from_bits(input.nodes[i].2) <= other_y
							|| other_y + f32::from_bits(input.nodes[j].2) <= y
					);
				}
			}
			extents.push(bounds(&positions, &input));
			assert_eq!(positions, input.arrange());
		}
		assert!(extents[0].0 > extents[0].1 * 2.);
		assert!(extents[1].1 > extents[1].0);
		assert!(extents[0].1 < extents[1].1);
	}

	#[test]
	fn ownership_cycles_and_cross_workspace_links_do_not_drop_or_duplicate_nodes() {
		let mut input = input((1200., 600.));
		input.nodes[1].1 = Some("other".into());
		input.nodes[2].1 = Some("other".into());
		input.edges.extend([(1, 2), (2, 1), (3, 0), (0, 1)]);
		let positions = input.arrange();
		let mut indices: Vec<_> = positions.iter().map(|p| p.0).collect();
		indices.sort_unstable();
		assert_eq!(indices, (0..input.nodes.len()).collect::<Vec<_>>());
	}

	#[test]
	fn a_coordinator_with_unassigned_children_still_precedes_all_workspace_branches() {
		let nodes = (0..13)
			.map(|i| {
				(
					i.to_string(),
					match i {
						0 | 1 => None,
						2..=3 => Some("project".into()),
						_ => Some("reading".into()),
					},
					64_f32.to_bits(),
				)
			})
			.collect();
		let edges = (1..13).map(|i| (if i == 3 { 2 } else { 0 }, i)).collect();
		let input = Input { nodes, edges, viewport: (2300., 1000.), ..Default::default() };
		let positions = input.arrange();
		let &(_, x, y) = positions.iter().find(|p| p.0 == 0).unwrap();
		assert!(
			positions.iter().filter(|p| p.0 != 0).all(|p| p.1 >= x + WIDTH)
				|| positions.iter().filter(|p| p.0 != 0).all(|p| p.2 >= y + 64.)
		);
		assert_eq!(positions.len(), 13);
	}

	#[test]
	fn shallow_wide_canvas_retains_complete_branches_without_overlapping_cards() {
		let nodes = (0..21)
			.map(|i| {
				(
					i.to_string(),
					match i {
						0 => None,
						1..=3 => Some("project".into()),
						_ => Some("reading".into()),
					},
					64_f32.to_bits(),
				)
			})
			.collect();
		let edges = (1..21)
			.map(|i| {
				(
					match i {
						2 | 3 => 1,
						5 | 6 => 4,
						15 | 16 => 14,
						_ => 0,
					},
					i,
				)
			})
			.collect();
		let input = Input { nodes, edges, viewport: (2080., 264.), ..Default::default() };
		let positions = input.arrange();
		let at = |i| positions.iter().find(|p| p.0 == i).unwrap();
		for &(parent, child) in &input.edges {
			assert!(
				at(child).1 >= at(parent).1 + WIDTH + COLUMN_GAP
					|| at(child).2 >= at(parent).2 + 64. + ROW_GAP
			);
		}
		assert_eq!(positions.len(), input.nodes.len());
		for &(a, x, y) in &positions {
			for &(b, bx, by) in &positions {
				if a != b {
					assert!(x + WIDTH <= bx || bx + WIDTH <= x || y + 64. <= by || by + 64. <= y);
				}
			}
		}
	}
	#[test]
	fn seven_parallel_branches_fit_at_readable_size_in_the_workspace() {
		let input = Input {
			nodes: (0..8)
				.map(|i| (i.to_string(), None, super::super::card::HEIGHT.to_bits()))
				.collect(),
			edges: (1..8).map(|child| (0, child)).collect(),
			viewport: (920., 680.),
			..Default::default()
		};
		let positions = input.arrange();
		assert_eq!(positions.len(), 8);
		let (width, height) = bounds(&positions, &input);
		let readable_scale = (888. / width).min(648. / height);
		assert!(readable_scale >= 0.85, "fan-out uses the available pane: {width} x {height}");
		for &(a, x, y) in &positions {
			for &(b, bx, by) in &positions {
				if a != b {
					assert!(
						x + WIDTH <= bx
							|| bx + WIDTH <= x
							|| y + super::super::card::HEIGHT <= by
							|| by + super::super::card::HEIGHT <= y
					);
				}
			}
		}
	}
}
