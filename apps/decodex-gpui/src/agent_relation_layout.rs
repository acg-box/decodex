//! Compact ownership trees: complete branches share space and keep their source relationships.
use super::{BTreeMap, FRAME_INSET, Graph};

const WIDTH: f32 = super::card::WIDTH;
const SIBLING_GAP: f32 = 24.;
const COLUMN_GAP: f32 = 48.;
const ROW_GAP: f32 = 24.;
// Keep project boundaries clear without adding a full empty hierarchy rank.
const WORKSPACE_GAP: f32 = 88.;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Presentation {
	#[default]
	Links,
	Together,
}
impl Presentation {
	fn gap(self, usual: f32, together: f32) -> f32 {
		if self == Self::Together { together } else { usual }
	}
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Mode {
	#[default]
	Tree,
	Radial,
	Grouped,
}
impl Mode {
	pub(super) const ALL: [Self; 3] = [Self::Tree, Self::Radial, Self::Grouped];

	pub(super) fn label(self) -> &'static str {
		match self {
			Self::Tree => "Tree",
			Self::Radial => "Radial",
			Self::Grouped => "Grouped",
		}
	}
}

#[derive(Default)]
pub(super) struct Cache {
	input: Input,
	positions: Vec<(usize, f32, f32)>,
}

#[derive(PartialEq)]
struct Input {
	mode: Mode,
	presentation: Presentation,
	base_heights: Vec<u32>,
	nodes: Vec<(String, Option<String>, u32)>,
	edges: Vec<(usize, usize)>,
	viewport: (f32, f32),
	width: f32,
	column_gap: f32,
}
impl Default for Input {
	fn default() -> Self {
		Self {
			mode: Mode::default(),
			presentation: Presentation::default(),
			base_heights: vec![],
			nodes: vec![],
			edges: vec![],
			viewport: (0., 0.),
			width: WIDTH,
			column_gap: COLUMN_GAP,
		}
	}
}

struct Tree {
	index: usize,
	children: Vec<Self>,
}

#[derive(Clone, Default)]
struct Block {
	width: f32,
	height: f32,
	positions: Vec<(usize, f32, f32)>,
}

impl Cache {
	pub(super) fn place(
		&mut self,
		graph: &mut Graph,
		viewport: (f32, f32),
		mode: Mode,
		presentation: Presentation,
	) {
		let indices: BTreeMap<_, _> =
			graph.nodes.iter().enumerate().map(|(i, n)| (n.key.as_str(), i)).collect();
		let label_gap = graph
			.connections()
			.iter()
			.filter(|edge| edge.kind.hierarchy())
			.map(|edge| graph.connection_label(edge).width + 16.)
			.fold(0., f32::max);
		let input = Input {
			mode,
			presentation,
			base_heights: graph.nodes.iter().map(|node| node.base_height.to_bits()).collect(),
			nodes: graph
				.nodes
				.iter()
				.map(|n| {
					(
						n.key.clone(),
						n.row.as_ref().and_then(|r| r.workspace.clone()),
						n.layout_height.to_bits(),
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
			column_gap: presentation.gap(COLUMN_GAP.max(label_gap), 24.),
		};
		if self.input != input {
			self.positions = input.arrange();
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

	fn arrange(&self) -> Vec<(usize, f32, f32)> {
		let mut forest = self.forest();
		let trees = if let Some(group) = self.coordinator(&forest) {
			let mut root = forest[group].remove(0);
			root.children.extend(forest.into_iter().flatten());
			vec![root]
		} else {
			forest.into_iter().flatten().collect()
		};
		let available = (
			(self.viewport.0 - 2. * FRAME_INSET).max(self.width),
			(self.viewport.1 - 2. * FRAME_INSET).max(320.),
		);
		let block = match self.mode {
			Mode::Tree | Mode::Grouped => Block::pack(
				trees.iter().map(|tree| tree.layout(self, self.mode == Mode::Grouped)).collect(),
				self.presentation.gap(WORKSPACE_GAP, 24.),
				available,
			),
			Mode::Radial => self.radial(&trees),
		};
		block.positions
	}

	fn radial(&self, trees: &[Tree]) -> Block {
		let gap = self.presentation.gap(WORKSPACE_GAP, 24.);
		let (root, branches) = if trees.len() == 1 && !trees[0].children.is_empty() {
			(Some(trees[0].index), trees[0].children.as_slice())
		} else {
			(None, trees)
		};
		if branches.is_empty() {
			return Block::default();
		}
		let mut blocks = Vec::new();
		let mut directions = Vec::new();
		for (index, tree) in branches.iter().enumerate() {
			let angle = -std::f32::consts::FRAC_PI_2
				+ index as f32 * std::f32::consts::TAU / branches.len() as f32;
			let (x, y) = (angle.cos(), angle.sin());
			let horizontal = x.abs() > y.abs();
			let mut block = tree.layout(self, horizontal);
			if horizontal && x < 0. {
				for &mut (_, ref mut px, _) in &mut block.positions {
					*px = block.width - *px - self.width;
				}
			} else if !horizontal && y < 0. {
				for &mut (node, _, ref mut py) in &mut block.positions {
					*py = block.height - *py - f32::from_bits(self.nodes[node].2);
				}
			}
			blocks.push(block);
			directions.push((x, y));
		}
		let root_height = root.map_or(0., |index| f32::from_bits(self.nodes[index].2));
		let rx =
			blocks.iter().map(|b| b.width).fold(self.width, f32::max) / 2. + self.width / 2. + gap;
		let ry = blocks.iter().map(|b| b.height).fold(root_height, f32::max) / 2.
			+ root_height / 2.
			+ gap;
		let mut factor = 1_f32;
		let separation = |dx: f32, dy: f32, width: f32, height: f32| {
			let along = |extent: f32, delta: f32| {
				if delta.abs() < 0.001 { f32::INFINITY } else { extent / delta.abs() }
			};
			along(width, dx).min(along(height, dy))
		};
		for (i, block) in blocks.iter().enumerate() {
			let (x, y) = directions[i];
			if root.is_some() {
				factor = factor.max(separation(
					x * rx,
					y * ry,
					(block.width + self.width) / 2. + gap,
					(block.height + root_height) / 2. + gap,
				));
			}
			for (j, other) in blocks.iter().enumerate().take(i) {
				factor = factor.max(separation(
					(x - directions[j].0) * rx,
					(y - directions[j].1) * ry,
					(block.width + other.width) / 2. + gap,
					(block.height + other.height) / 2. + gap,
				));
			}
		}
		let mut result = Block::default();
		if let Some(index) = root {
			result.positions.push((index, -self.width / 2., -root_height / 2.));
		}
		for (mut block, (x, y)) in blocks.into_iter().zip(directions) {
			block.offset(x * rx * factor - block.width / 2., y * ry * factor - block.height / 2.);
			result.positions.extend(block.positions);
		}
		let left = result.positions.iter().map(|p| p.1).fold(0., f32::min);
		let top = result.positions.iter().map(|p| p.2).fold(0., f32::min);
		result.offset(-left, -top);
		result.width = result.positions.iter().map(|p| p.1 + self.width).fold(0., f32::max);
		result.height = result
			.positions
			.iter()
			.map(|p| p.2 + f32::from_bits(self.nodes[p.0].2))
			.fold(0., f32::max);
		result
	}
}

impl Tree {
	fn visit(index: usize, children: &[Vec<usize>], visited: &mut [bool]) -> Self {
		visited[index] = true;
		let mut tree = Self { index, children: Vec::new() };
		for &child in &children[index] {
			if !visited[child] {
				tree.children.push(Self::visit(child, children, visited));
			}
		}
		tree
	}

	fn layout(&self, input: &Input, horizontal: bool) -> Block {
		let gap = input.presentation.gap(
			SIBLING_GAP,
			if self.children.iter().any(|child| !child.children.is_empty()) { 24. } else { 12. },
		);
		let height = f32::from_bits(input.nodes[self.index].2);
		let base =
			input.base_heights.get(self.index).copied().map(f32::from_bits).unwrap_or(height);
		let step = if horizontal {
			input.width + input.column_gap
		} else {
			(base + input.presentation.gap(28., 8.)).max(height + input.presentation.gap(24., 8.))
		};
		let children: Vec<_> =
			self.children.iter().map(|child| child.layout(input, horizontal)).collect();
		let leaves = self.children.iter().all(|child| child.children.is_empty());
		let mut block = if leaves && children.len() >= 4 {
			// Balance the actual footprints and their parent as one compact group.
			// Use a stable aspect so resizing or zooming does not reshuffle a tree.
			(2..=children.len().min(4))
				.map(|columns| Block::grid(children.clone(), columns, gap))
				.max_by(|a, b| {
					let score = |block: &Block| {
						let width =
							(block.width + if horizontal { step } else { 0. }).max(input.width);
						let height =
							(block.height + if horizontal { 0. } else { step }).max(height);
						-((width / height) / (4. / 3.)).ln().abs()
					};
					score(a).total_cmp(&score(b))
				})
				.unwrap_or_default()
		} else if children.len() >= 4 {
			Block::pack(children, input.presentation.gap(WORKSPACE_GAP, 24.), input.viewport)
		} else {
			Block::siblings(children, horizontal, gap)
		};
		if !block.positions.is_empty() {
			// New activity first uses the existing connector clearance. It must
			// not move every sibling when the first operation appears.
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
	fn grid(blocks: Vec<Self>, columns: usize, gap: f32) -> Self {
		let mut result = Self::default();
		let mut y = 0.;
		for row in blocks.chunks(columns) {
			let mut x = 0.;
			let height = row.iter().map(|b| b.height).fold(0., f32::max);
			for block in row {
				let mut block = block.clone();
				let width = block.width;
				block.offset(x, y);
				result.positions.extend(block.positions);
				result.width = result.width.max(x + width);
				x += width + gap;
			}
			result.height = y + height;
			y += height + ROW_GAP;
		}
		result
	}

	// Compare shelf counts in both axes. This extends the existing shelf packer;
	// no graph or bin-packing runtime is needed for these small cached forests.
	fn pack(blocks: Vec<Self>, gap: f32, available: (f32, f32)) -> Self {
		let mut best: Option<(f32, f32, Self)> = None;
		for horizontal in [false, true] {
			let ordered = blocks.clone();
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
	fn explicit_tree_layout_retains_its_direction_in_wide_and_tall_canvases() {
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
		assert!(extents[0].0 > extents[0].1, "the compact tree retains its horizontal extent");
		assert_eq!(
			extents[0], extents[1],
			"an explicit layout must not change direction on resize"
		);
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
	fn together_keeps_all_hierarchy_members_in_compact_non_overlapping_formations() {
		for mode in Mode::ALL {
			let mut source = input((1400., 900.));
			source.mode = mode;
			let original = source.arrange();
			let original_area = {
				let (w, h) = bounds(&original, &source);
				w * h
			};
			source.presentation = Presentation::Together;
			source.column_gap = 24.;
			let positions = source.arrange();
			assert_eq!(positions.len(), source.nodes.len());
			for &(i, x, y) in &positions {
				for &(j, bx, by) in &positions {
					if i != j {
						assert!(
							x + source.width <= bx
								|| bx + source.width <= x
								|| y + f32::from_bits(source.nodes[i].2) <= by
								|| by + f32::from_bits(source.nodes[j].2) <= y
						);
					}
				}
			}
			let (w, h) = positions.iter().fold((0_f32, 0_f32), |(w, h), &(i, x, y)| {
				(w.max(x + source.width), h.max(y + f32::from_bits(source.nodes[i].2)))
			});
			assert!(w * h < original_area, "{mode:?} should gather its teams");
			assert_eq!(positions, source.arrange(), "ongoing work does not make teams wander");
		}
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
				.map(|i| {
					(
						i.to_string(),
						None,
						super::super::card::Layout::new(1.).world_height(0.).to_bits(),
					)
				})
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
							|| y + super::super::card::Layout::new(1.).world_height(0.) <= by
							|| by + super::super::card::Layout::new(1.).world_height(0.) <= y
					);
				}
			}
		}
	}
	#[test]
	fn all_layouts_retain_a_hundred_agents_with_variable_footprints_without_overlap() {
		for zoom in [0.05, 0.1, 0.19, 0.2, 0.25, 0.45, 0.7, 1.] {
			let nodes = (0..103)
				.map(|i| {
					(
						format!("agent-{i}"),
						(i > 0).then(|| {
							format!("project-{}", if i <= 6 { i } else { 1 + (i - 7) / 16 })
						}),
						super::super::card::Layout::new(zoom)
							.world_height(if i % 3 == 0 { 3. } else { 0. })
							.to_bits(),
					)
				})
				.collect();
			let edges = (1..103).map(|i| (if i <= 6 { 0 } else { 1 + (i - 7) / 16 }, i)).collect();
			let mut input = Input { nodes, edges, viewport: (1030., 1000.), ..Default::default() };
			let mut shapes = Vec::new();
			for mode in Mode::ALL {
				input.mode = mode;
				let positions = input.arrange();
				let ids: std::collections::BTreeSet<_> = positions.iter().map(|p| p.0).collect();
				assert_eq!(ids.len(), 103, "{mode:?} at {zoom}");
				assert_eq!(positions.len(), ids.len());
				assert_eq!(positions, input.arrange());
				for &(a, x, y) in &positions {
					assert!(x.is_finite() && y.is_finite());
					for &(b, bx, by) in &positions {
						if a != b {
							assert!(
								x + WIDTH <= bx + 0.01
									|| bx + WIDTH <= x + 0.01
									|| y + f32::from_bits(input.nodes[a].2) <= by + 0.01
									|| by + f32::from_bits(input.nodes[b].2) <= y + 0.01,
								"{mode:?} at {zoom}: {a} overlaps {b}"
							);
						}
					}
				}
				shapes.push(positions);
			}
			assert!(shapes.windows(2).all(|pair| pair[0] != pair[1]));
		}
	}
}
