//! Bounded, deterministic projection of Chief dependency facts into canvas space.
use std::collections::{BTreeMap, BTreeSet};

use decodex_protocol::{
	ChiefDispatchStateDto, ChiefSnapshotDto, ChiefWorkItemDto, ChiefWorkStatusDto,
};

#[derive(Clone, Debug)]
pub(super) struct Node {
	pub id: String,
	pub x: f32,
	pub y: f32,
}

#[derive(Clone, Debug, Default)]
pub(super) struct Layout {
	pub nodes: Vec<Node>,
	pub edges: Vec<(usize, usize)>,
	pub reports: Vec<(usize, usize)>,
	pub cyclic: bool,
}

impl Layout {
	pub fn new(snapshot: &ChiefSnapshotDto, scope: Option<&str>) -> Self {
		let items: Vec<_> = snapshot
			.work_items
			.iter()
			.filter(|work| work.parent_goal_id.as_deref() == scope && work.parent_goal_id.is_some())
			.collect();
		let ids: BTreeSet<_> = items.iter().map(|w| w.id.as_str()).collect();
		let mut levels = BTreeMap::new();
		let mut remaining = ids.clone();
		while !remaining.is_empty() {
			let ready: Vec<_> = remaining
				.iter()
				.copied()
				.filter(|id| {
					snapshot
						.dependencies
						.iter()
						.filter(|e| e.work_item_id == *id && ids.contains(e.depends_on_id.as_str()))
						.all(|e| levels.contains_key(e.depends_on_id.as_str()))
				})
				.collect();
			if ready.is_empty() {
				break;
			}
			for id in ready {
				let level = snapshot
					.dependencies
					.iter()
					.filter(|e| e.work_item_id == id)
					.filter_map(|e| levels.get(e.depends_on_id.as_str()))
					.copied()
					.max()
					.map_or(0, |n: usize| n + 1);
				levels.insert(id, level);
				remaining.remove(id);
			}
		}
		let cyclic = !remaining.is_empty();
		let fallback = levels.values().copied().max().map_or(0, |n| n + 1);
		for id in remaining {
			levels.insert(id, fallback);
		}
		let mut nodes: Vec<Node> = Vec::new();
		for level in 0..=fallback {
			let mut occupied = Vec::<f32>::new();
			for work in items.iter().filter(|w| levels[w.id.as_str()] == level) {
				let parents: Vec<_> = snapshot
					.dependencies
					.iter()
					.filter(|e| e.work_item_id == work.id)
					.filter_map(|e| nodes.iter().find(|n| n.id == e.depends_on_id))
					.collect();
				let mut x = if parents.is_empty() {
					20.0
				} else {
					parents.iter().map(|n| n.x).sum::<f32>() / parents.len() as f32
				};
				while occupied.iter().any(|used| (x - used).abs() < 180.0) {
					x += 188.0;
				}
				occupied.push(x);
				nodes.push(Node { id: work.id.clone(), x, y: 32.0 + level as f32 * 112.0 });
			}
		}

		let edges = snapshot
			.dependencies
			.iter()
			.filter_map(|edge| {
				Some((
					nodes.iter().position(|n| n.id == edge.depends_on_id)?,
					nodes.iter().position(|n| n.id == edge.work_item_id)?,
				))
			})
			.collect();
		let mut reports = Vec::new();
		if !nodes.is_empty()
			&& let Some(owner) =
				scope.and_then(|id| snapshot.work_items.iter().find(|w| w.id == id))
		{
			let center = nodes.iter().map(|n| n.x).sum::<f32>() / nodes.len() as f32;
			for node in &mut nodes {
				node.y += 112.0;
			}
			let index = nodes.len();
			reports = (0..index).map(|child| (index, child)).collect();
			nodes.push(Node { id: owner.id.clone(), x: center, y: 32.0 });
		}
		Self { nodes, edges, reports, cyclic }
	}
}

pub(super) fn state(work: &ChiefWorkItemDto) -> (&'static str, u32) {
	use super::ui_theme;
	match work.dispatch_state {
		ChiefDispatchStateDto::Unknown => ("Needs attention", ui_theme::AMBER),
		ChiefDispatchStateDto::Running => ("Running", ui_theme::GREEN),
		ChiefDispatchStateDto::Dispatching => ("Starting", ui_theme::BLUE),
		ChiefDispatchStateDto::Idle => match work.status {
			ChiefWorkStatusDto::Resolved => ("Resolved", ui_theme::TEXT_MUTED),
			ChiefWorkStatusDto::UserDecision => ("Needs you", ui_theme::AMBER),
			ChiefWorkStatusDto::Wait => ("Waiting", ui_theme::TEXT_MUTED),
			ChiefWorkStatusDto::FollowUp => ("Follow-up", ui_theme::BLUE),
			ChiefWorkStatusDto::Open => ("Open", ui_theme::TEXT_MUTED),
		},
	}
}

/// A dependency is ready only after acceptance and an idle provider state.
pub(super) fn blockers<'a>(
	snapshot: &'a ChiefSnapshotDto,
	work: &ChiefWorkItemDto,
) -> Vec<&'a ChiefWorkItemDto> {
	snapshot
		.dependencies
		.iter()
		.filter(|edge| edge.work_item_id == work.id)
		.filter_map(|edge| snapshot.work_items.iter().find(|item| item.id == edge.depends_on_id))
		.filter(|item| {
			item.status != ChiefWorkStatusDto::Resolved
				|| item.dispatch_state != ChiefDispatchStateDto::Idle
		})
		.collect()
}

pub(super) fn state_in(
	snapshot: &ChiefSnapshotDto,
	work: &ChiefWorkItemDto,
) -> (&'static str, u32) {
	if work.dispatch_state == ChiefDispatchStateDto::Idle
		&& work.status != ChiefWorkStatusDto::Resolved
		&& !blockers(snapshot, work).is_empty()
	{
		("Blocked", super::ui_theme::AMBER)
	} else {
		state(work)
	}
}
