//! Reveal relation detail without changing the ownership layout or losing history.
use std::{rc::Rc, time::Instant};

use crate::{
	shell::agent_surface::work_board::relations::{BTreeMap, Edge, Graph, Kind, Node, Route},
	ui_motion::Tween,
};

#[derive(Clone)]
pub(super) struct Link {
	pub base: Edge,
	pub fact: Edge,
	pub source: Node,
	pub target: Node,
	pub path: Rc<Route>,
	pub detail: f32,
	pub dim: f32,
	pub active: bool,
}
impl Link {
	/// A running child is active, but only an ongoing native call denotes a transfer.
	pub(super) fn flowing(&self) -> bool {
		self.active && self.fact.in_progress
	}
}

#[derive(Default)]
pub(super) struct Cache {
	detail: BTreeMap<(String, String), Tween>,
	dim: Option<Tween>,
}
impl Cache {
	pub(super) fn node_opacity(&self, graph: &Graph, node: &Node) -> f32 {
		if node.row.is_some() {
			return 1.;
		}

		let now = Instant::now();

		graph
			.edges
			.iter()
			.filter(|edge| edge.from == node.key || edge.to == node.key)
			.filter_map(|edge| self.detail.get(&pair_key(edge)))
			.map(|detail| detail.sample(now))
			.fold(0., f32::max)
	}

	pub(super) fn sample(
		&mut self,
		graph: &Graph,
		context: Option<&str>,
		picked: Option<&Edge>,
		connected: bool,
		now: Instant,
	) -> (Vec<Link>, bool) {
		let dim_tween = self.dim.get_or_insert_with(|| Tween::new(0.));

		dim_tween.target(if context.is_some() || picked.is_some() { 1. } else { 0. }, now);

		let dim = dim_tween.sample(now);
		let connections = graph.connections();
		let mut moving = dim_tween.moving(now);

		self.detail.retain(|pair, _| connections.iter().any(|edge| matches_pair(edge, pair)));

		let mut links = Vec::new();

		for base in connections {
			let target = graph.nodes.iter().find(|node| node.key == base.to).expect("edge target");
			let active_fact = if connected {
				graph
					.edges
					.iter()
					.rev()
					.find(|edge| edge.in_progress && edge.same_pair(&base))
					.or_else(|| base.executing(target, true).then_some(&base))
			} else {
				None
			};
			let active = active_fact.is_some();
			let focused = context.is_some_and(|key| base.from == key || base.to == key)
				|| picked.is_some_and(|edge| {
					edge.same_pair(&base)
						|| (edge.kind == Kind::Resource
							&& base.kind == Kind::Resource
							&& edge.to == base.to)
				});
			let detail = self.detail.entry(pair_key(&base)).or_insert_with(|| Tween::new(0.));

			detail.target(if focused || active { 1. } else { 0. }, now);

			moving |= detail.moving(now);

			let detail = detail.sample(now);
			let path = graph.paths[&base.id].clone();

			if !path.is_visible() || (!base.kind.hierarchy() && detail <= 0.001) {
				continue;
			}

			let fact = active_fact
				.or(picked.filter(|edge| edge.same_pair(&base)))
				.or_else(|| {
					graph
						.edges
						.iter()
						.rev()
						.find(|edge| !edge.kind.hierarchy() && edge.same_pair(&base))
				})
				.unwrap_or(&base)
				.clone();

			links.push(Link {
				source: graph
					.nodes
					.iter()
					.find(|node| node.key == fact.from)
					.expect("edge source")
					.clone(),
				target: graph
					.nodes
					.iter()
					.find(|node| node.key == fact.to)
					.expect("edge target")
					.clone(),
				base,
				fact,
				path,
				detail,
				dim,
				active,
			});
		}

		(links, moving)
	}
}

fn pair_key(edge: &Edge) -> (String, String) {
	if edge.from <= edge.to {
		(edge.from.clone(), edge.to.clone())
	} else {
		(edge.to.clone(), edge.from.clone())
	}
}

fn matches_pair(edge: &Edge, pair: &(String, String)) -> bool {
	(edge.from == pair.0 && edge.to == pair.1) || (edge.from == pair.1 && edge.to == pair.0)
}

#[cfg(test)]
mod tests {
	use std::time::Duration;

	use gpui::AppContext;

	use crate::shell::agent_surface::work_board::relations::visibility::{Cache, Instant};

	#[gpui::test]
	fn overview_and_focus_preserve_all_facts_and_fade_in_both_directions(
		cx: &mut gpui::TestAppContext,
	) {
		let surface = cx.new(super::super::super::tests::fixture);

		surface.update(cx, |s, _| {
			let graph = s.relation_graph();
			let coordinates: Vec<_> = graph.nodes.iter().map(|node| (node.x, node.y)).collect();
			let count = graph.edges.len();
			let now = Instant::now();
			let mut cache = Cache::default();
			let (overview, moving) = cache.sample(&graph, None, None, false, now);

			assert!(!moving);
			assert!(overview.iter().all(|link| link.base.kind.hierarchy() && link.detail == 0.));

			cache.sample(&graph, Some("verify"), None, false, now);

			let (focused, _) =
				cache.sample(&graph, Some("verify"), None, false, now + Duration::from_millis(220));

			assert!(focused.iter().any(|link| !link.base.kind.hierarchy() && link.detail == 1.));

			let leave = now + Duration::from_millis(221);

			cache.sample(&graph, None, None, false, leave);

			let (fading, moving) =
				cache.sample(&graph, None, None, false, leave + Duration::from_millis(80));

			assert!(moving);
			assert!(
				fading.iter().any(|link| !link.base.kind.hierarchy()
					&& link.detail > 0.
					&& link.detail < 1.)
			);

			let (overview, moving) =
				cache.sample(&graph, None, None, false, leave + Duration::from_millis(220));

			assert!(!moving);
			assert!(overview.iter().all(|link| link.base.kind.hierarchy()));

			let after = s.relation_graph();

			assert_eq!(after.edges.len(), count);
			assert_eq!(
				after.nodes.iter().map(|node| (node.x, node.y)).collect::<Vec<_>>(),
				coordinates
			);
		});
	}

	#[gpui::test]
	fn active_work_keeps_its_forward_flow_without_a_selected_node(cx: &mut gpui::TestAppContext) {
		let surface = cx.new(super::super::super::tests::fixture);

		surface.update(cx, |s, _| {
			let mut graph = s.relation_graph();
			let edge = graph.edges.iter().find(|edge| edge.kind.hierarchy()).unwrap().clone();

			graph
				.nodes
				.iter_mut()
				.find(|node| node.key == edge.to)
				.unwrap()
				.row
				.as_mut()
				.unwrap()
				.status = "Running".into();

			let now = Instant::now();
			let mut cache = Cache::default();

			cache.sample(&graph, None, None, true, now);

			let (links, _) =
				cache.sample(&graph, None, None, true, now + Duration::from_millis(220));
			let running = links.iter().find(|link| link.base.same_pair(&edge)).unwrap();

			assert!(running.active);
			assert!(!running.flowing(), "working alone must not imply a handoff");
			assert_eq!(running.detail, 1.);
			assert_eq!(running.fact.from, edge.from);
			assert_eq!(running.fact.to, edge.to);

			let renewed = "renewed-call".to_owned();

			graph.paths.insert(renewed.clone(), graph.paths[&edge.id].clone());

			graph.edges.iter_mut().find(|current| current.id == edge.id).unwrap().id = renewed;

			let (renewed, moving) =
				cache.sample(&graph, None, None, true, now + Duration::from_millis(221));

			assert!(!moving, "new evidence for the same pair must not restart its animation");
			assert_eq!(renewed.iter().find(|link| link.base.same_pair(&edge)).unwrap().detail, 1.);

			let (offline, _) =
				cache.sample(&graph, None, None, false, now + Duration::from_millis(222));

			assert!(offline.iter().all(|link| !link.active));
		});
	}

	#[gpui::test]
	fn an_in_progress_native_message_stays_visible_until_it_finishes(
		cx: &mut gpui::TestAppContext,
	) {
		let surface = cx.new(super::super::super::tests::fixture);

		surface.update(cx, |s, _| {
			let mut graph = s.relation_graph();

			for node in &mut graph.nodes {
				if let Some(row) = &mut node.row {
					row.status = "Idle".into();
				}
			}

			let mut message =
				graph.edges.iter().find(|edge| edge.kind.hierarchy()).unwrap().clone();

			message.id = "live-message".into();
			message.kind = super::super::Kind::Message;
			message.in_progress = true;

			graph.edges.insert(0, message.clone());

			let now = Instant::now();
			let mut cache = Cache::default();

			cache.sample(&graph, None, None, true, now);

			let (links, _) =
				cache.sample(&graph, None, None, true, now + Duration::from_millis(220));
			let link = links.iter().find(|link| link.base.same_pair(&message)).unwrap();

			assert!(link.base.kind.hierarchy(), "call arrival order must not replace the tree");
			assert!(link.active);
			assert!(link.flowing());
			assert_eq!(link.fact.id, "live-message");

			graph.edges[0].in_progress = false;

			let (links, _) =
				cache.sample(&graph, None, None, true, now + Duration::from_millis(221));

			assert!(links.iter().all(|link| !link.active));
			assert!(links.iter().all(|link| !link.flowing()));
		});
	}
}
