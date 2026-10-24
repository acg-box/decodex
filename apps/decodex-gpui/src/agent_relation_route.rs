//! Cache smooth connections and route obstructed edges through free card gutters.
use super::{BTreeMap, Edge, Kind, Node};
use std::{cmp::Ordering, collections::BinaryHeap, rc::Rc};

type Point = (f32, f32);
pub(super) const CLEARANCE: f32 = 12.;

#[derive(Clone, PartialEq)]
pub(super) struct Curve {
	pub start: Point,
	pub end: Point,
	pub first: Point,
	pub second: Point,
}

impl Curve {
	fn line(start: Point, end: Point) -> Self {
		let length = distance(start, end);
		Self {
			start,
			first: toward(start, end, length / 3.),
			second: toward(end, start, length / 3.),
			end,
		}
	}

	fn reversed(&self) -> Self {
		Self { start: self.end, end: self.start, first: self.second, second: self.first }
	}

	fn at(&self, t: f32) -> Point {
		let u = 1. - t;
		let axis =
			|a, b, c, d| u * u * u * a + 3. * u * u * t * b + 3. * u * t * t * c + t * t * t * d;
		(
			axis(self.start.0, self.first.0, self.second.0, self.end.0),
			axis(self.start.1, self.first.1, self.second.1, self.end.1),
		)
	}

	fn span(&self) -> f32 {
		distance(self.start, self.first)
			+ distance(self.first, self.second)
			+ distance(self.second, self.end)
	}

	fn halves(&self) -> [Self; 2] {
		let middle = |a: Point, b: Point| ((a.0 + b.0) / 2., (a.1 + b.1) / 2.);
		let a = middle(self.start, self.first);
		let b = middle(self.first, self.second);
		let c = middle(self.second, self.end);
		let d = middle(a, b);
		let e = middle(b, c);
		let center = middle(d, e);
		[
			Self { start: self.start, first: a, second: d, end: center },
			Self { start: center, first: e, second: c, end: self.end },
		]
	}
}

#[derive(Clone)]
pub(super) struct Route {
	pub curves: Vec<Curve>,
	pub ink: Vec<Curve>,
	pub ink_arrow: bool,
	pub label: Point,
	pub samples: Vec<(Point, f32)>,
	spans: Vec<f32>,
	length: f32,
}

impl Route {
	pub(super) fn is_visible(&self) -> bool {
		self.length > 0.001
	}

	pub(super) fn curve(start: Point, first: Point, second: Point, end: Point) -> Self {
		Self::from_curves(vec![Curve { start, first, second, end }])
	}

	fn branch(start: Point, end: Point, horizontal: bool) -> Self {
		// A single monotone curve leaves and enters the facing card edges.
		// No rectangular spine, short elbows, or overlapping fork decorations.
		let (first, second) = if horizontal {
			let x = (start.0 + end.0) / 2.;
			((x, start.1), (x, end.1))
		} else {
			let y = (start.1 + end.1) / 2.;
			((start.0, y), (end.0, y))
		};
		Self::curve(start, first, second, end)
	}

	fn from_curves(curves: Vec<Curve>) -> Self {
		let spans: Vec<_> = curves.iter().map(Curve::span).collect();
		let length = spans.iter().sum();
		let mut samples = vec![(curves[0].start, 0.)];
		for (curve, span) in curves.iter().zip(&spans) {
			let steps = (span / 16.).ceil().clamp(2., 64.) as usize;
			for i in 1..=steps {
				let point = curve.at(i as f32 / steps as f32);
				let &(previous, length) = samples.last().expect("curve start");
				samples.push((point, length + distance(previous, point)));
			}
		}
		let mut route = Self {
			ink: curves.clone(),
			ink_arrow: true,
			curves,
			spans,
			length,
			samples,
			label: (0., 0.),
		};
		route.label = route.at(0.5);
		route
	}

	pub(super) fn at(&self, t: f32) -> Point {
		let mut remaining = t * self.length;
		for (curve, &span) in self.curves.iter().zip(&self.spans) {
			if remaining <= span && span > 0. {
				return curve.at(remaining / span);
			}
			remaining -= span;
		}
		self.curves.last().expect("connection curve").end
	}

	pub(super) fn reversed(&self) -> Self {
		let mut route = Self::from_curves(self.curves.iter().rev().map(Curve::reversed).collect());
		route.label = self.label;
		route
	}

	fn polyline(points: &[Point], rounding: f32) -> Self {
		let mut curves = Vec::new();
		let mut start = points[0];
		let mut longest = (0_f32, start);
		for triple in points.windows(3) {
			let [before, corner, after] = [triple[0], triple[1], triple[2]];
			let radius =
				rounding.min(distance(before, corner) / 2.).min(distance(corner, after) / 2.);
			let enter = toward(corner, before, radius);
			let leave = toward(corner, after, radius);
			append_line(&mut curves, start, enter, &mut longest);
			curves.push(Curve {
				start: enter,
				first: toward(enter, corner, radius * 2. / 3.),
				second: toward(leave, corner, radius * 2. / 3.),
				end: leave,
			});
			start = leave;
		}
		append_line(&mut curves, start, *points.last().expect("route endpoint"), &mut longest);
		let mut route = Self::from_curves(curves);
		route.label = longest.1;
		route
	}
}

fn distance(a: Point, b: Point) -> f32 {
	(b.0 - a.0).hypot(b.1 - a.1)
}

fn toward(a: Point, b: Point, amount: f32) -> Point {
	let scale = amount / distance(a, b).max(f32::EPSILON);
	(a.0 + (b.0 - a.0) * scale, a.1 + (b.1 - a.1) * scale)
}

fn append_line(curves: &mut Vec<Curve>, start: Point, end: Point, longest: &mut (f32, Point)) {
	let length = distance(start, end);
	if length > 0.001 {
		curves.push(Curve::line(start, end));
		if length > longest.0 {
			*longest = (length, ((start.0 + end.0) / 2., (start.1 + end.1) / 2.));
		}
	}
}

/// Each color owns its shared rails once. Full routes remain intact for focus and flow.
#[derive(Default)]
pub(super) struct Ink {
	lines: Vec<(usize, f32, f32, f32)>,
	curves: Vec<Curve>,
	arrows: Vec<(Point, Point)>,
}

impl Ink {
	fn take(&mut self, curve: &Curve) -> Vec<Curve> {
		let points = [curve.start, curve.first, curve.second, curve.end];
		let axis = if points.iter().all(|p| (p.1 - curve.start.1).abs() < 0.001) {
			0
		} else if points.iter().all(|p| (p.0 - curve.start.0).abs() < 0.001) {
			1
		} else {
			if self.curves.iter().any(|old| old == curve || *old == curve.reversed()) {
				return Vec::new();
			}
			self.curves.push(curve.clone());
			return vec![curve.clone()];
		};
		let (start, end, at) = if axis == 0 {
			(curve.start.0.min(curve.end.0), curve.start.0.max(curve.end.0), curve.start.1)
		} else {
			(curve.start.1.min(curve.end.1), curve.start.1.max(curve.end.1), curve.start.0)
		};
		let mut remaining = vec![(start, end)];
		for &(_, _, left, right) in self.lines.iter().filter(|&&(direction, position, _, _)| {
			direction == axis && (position - at).abs() < 0.001
		}) {
			remaining = remaining
				.into_iter()
				.flat_map(|(a, b)| {
					[(a, b.min(left)), (a.max(right), b)].into_iter().filter(|(a, b)| b - a > 0.001)
				})
				.collect();
		}
		if !remaining.is_empty() {
			self.lines.push((axis, at, start, end));
		}
		remaining
			.into_iter()
			.map(|(a, b)| {
				if axis == 0 {
					Curve::line((a, at), (b, at))
				} else {
					Curve::line((at, a), (at, b))
				}
			})
			.collect()
	}

	pub(super) fn retain(&mut self, route: &mut Route) {
		route.ink = route.curves.iter().flat_map(|curve| self.take(curve)).collect();
		let tip = route.curves.last().expect("connection curve");
		let length = distance(tip.second, tip.end).max(0.001);
		let arrow =
			(tip.end, ((tip.end.0 - tip.second.0) / length, (tip.end.1 - tip.second.1) / length));
		route.ink_arrow = !self.arrows.contains(&arrow);
		self.arrows.push(arrow);
	}
}

pub(super) fn route(a: &Node, b: &Node) -> Route {
	let (a_bounds, b_bounds) = (a.connection_bounds(), b.connection_bounds());
	let dx = b.connection_center().0 - a.connection_center().0;
	let dy = b.port_y() - a.port_y();
	let gap_x = if dx >= 0. { b_bounds[0] - a_bounds[2] } else { a_bounds[0] - b_bounds[2] };
	let gap_y = dy.abs() - (a_bounds[3] - a_bounds[1] + b_bounds[3] - b_bounds[1]) / 2.;
	// Relation type never changes geometry or creates a hook in a shared edge.
	if gap_x >= 0. && gap_x >= gap_y {
		let forward = dx >= 0.;
		let start = (a_bounds[if forward { 2 } else { 0 }], a.port_y());
		let end = (b_bounds[if forward { 0 } else { 2 }], b.port_y());
		let bend = (end.0 - start.0) / 2.;
		Route::curve(start, (start.0 + bend, start.1), (end.0 - bend, end.1), end)
	} else {
		let forward = dy >= 0.;
		let start = (a.connection_center().0, a_bounds[if forward { 3 } else { 1 }]);
		let end = (b.connection_center().0, b_bounds[if forward { 1 } else { 3 }]);
		let bend = (end.1 - start.1) / 2.;
		Route::curve(start, (start.0, start.1 + bend), (end.0, end.1 - bend), end)
	}
}

#[derive(Clone, PartialEq)]
struct Card {
	key: String,
	bounds: [f32; 4],
	port_y: f32,
}

impl Card {
	fn blocks_curve(&self, curve: &Curve) -> bool {
		fn intersects(curve: &Curve, bounds: [f32; 4], depth: u8) -> bool {
			let [left, top, right, bottom] = bounds;
			let points = [curve.start, curve.first, curve.second, curve.end];
			let min_x = points.iter().map(|p| p.0).fold(f32::INFINITY, f32::min);
			let max_x = points.iter().map(|p| p.0).fold(f32::NEG_INFINITY, f32::max);
			let min_y = points.iter().map(|p| p.1).fold(f32::INFINITY, f32::min);
			let max_y = points.iter().map(|p| p.1).fold(f32::NEG_INFINITY, f32::max);
			if max_x <= left || min_x >= right || max_y <= top || min_y >= bottom {
				return false;
			}
			if depth == 0 || min_x == max_x || min_y == max_y {
				return true;
			}
			curve.halves().iter().any(|half| intersects(half, bounds, depth - 1))
		}
		// Subdivision keeps the conservative hull guarantee without routing around
		// cards that lie inside the full control hull but nowhere near the curve.
		intersects(curve, self.bounds, 8)
	}

	fn crosses(&self, a: Point, b: Point) -> bool {
		let [left, top, right, bottom] = self.bounds;
		if (a.0 - b.0).abs() < 0.001 {
			a.0 > left + 0.001
				&& a.0 < right - 0.001
				&& a.1.min(b.1) < bottom - 0.001
				&& a.1.max(b.1) > top + 0.001
		} else {
			a.1 > top + 0.001
				&& a.1 < bottom - 0.001
				&& a.0.min(b.0) < right - 0.001
				&& a.0.max(b.0) > left + 0.001
		}
	}
}

#[derive(Default)]
pub(super) struct Cache {
	cards: Vec<Card>,
	edges: Vec<(String, String, Kind, u32)>,
	paths: Vec<Rc<Route>>,
}

impl Cache {
	pub(super) fn paths(&mut self, nodes: &[Node], edges: &[Edge]) -> BTreeMap<String, Rc<Route>> {
		let cards: Vec<_> = nodes
			.iter()
			.flat_map(|n| {
				let [left, top, right, bottom] = n.connection_bounds();
				let mut cards = vec![Card {
					key: n.key.clone(),
					port_y: n.port_y(),
					bounds: [
						left - CLEARANCE,
						top - CLEARANCE,
						right + CLEARANCE,
						bottom + CLEARANCE,
					],
				}];
				if let Some([left, top, right, bottom]) = n.identity {
					cards.push(Card {
						key: format!("identity:{}", n.key),
						port_y: n.y + (top + bottom) / 2.,
						bounds: [
							n.x + left - CLEARANCE,
							n.y + top - CLEARANCE,
							n.x + right + CLEARANCE,
							n.y + bottom + CLEARANCE,
						],
					});
				}
				// Keep the source and target's own activity text out of the route too.
				// A right port inside this column is not exposed; use another side.
				if n.attachment.is_some() && right + 3. < n.x + n.width() {
					cards.push(Card {
						key: format!("activity:{}", n.key),
						port_y: n.port_y(),
						bounds: [right + 3., n.y, n.x + n.width(), n.y + n.height()],
					});
				}
				cards
			})
			.collect();
		let pairs: Vec<_> =
			edges.iter().map(|e| (e.from.clone(), e.to.clone(), e.kind, e.color)).collect();
		if self.cards != cards || self.edges != pairs {
			let mut paths: Vec<_> = edges
				.iter()
				.map(|edge| {
					let a = nodes.iter().find(|n| n.key == edge.from).expect("edge source");
					let b = nodes.iter().find(|n| n.key == edge.to).expect("edge target");
					edge.kind
						.hierarchy()
						.then(|| tree_route(a, b, nodes, edges, &cards))
						.flatten()
						.unwrap_or_else(|| avoid_cards(route(a, b), a, b, &cards))
				})
				.collect();
			let mut ink = BTreeMap::<u32, Ink>::new();
			for (edge, path) in edges.iter().zip(&mut paths) {
				if edge.kind.pattern().is_none() {
					ink.entry(edge.color).or_default().retain(path);
				}
			}
			self.paths = paths.into_iter().map(Rc::new).collect();
			self.cards = cards;
			self.edges = pairs;
		}
		edges.iter().zip(&self.paths).map(|(edge, path)| (edge.id.clone(), path.clone())).collect()
	}
}

// Ownership uses clean curves between facing ports in each tree rank.
// Obstructed or dragged branches use the same obstacle router as every other edge.
fn tree_route(a: &Node, b: &Node, nodes: &[Node], edges: &[Edge], cards: &[Card]) -> Option<Route> {
	let children: Vec<_> = edges
		.iter()
		.filter(|edge| edge.kind.hierarchy() && edge.from == a.key)
		.filter_map(|edge| nodes.iter().find(|node| node.key == edge.to))
		.collect();
	let mut branches = Vec::new();
	let (aa, bb) = (a.connection_bounds(), b.connection_bounds());
	if children.iter().all(|node| node.connection_bounds()[0] >= aa[2] + CLEARANCE) {
		let start = (aa[2], a.port_y());
		branches.push(Route::branch(start, (bb[0], b.port_y()), true));
	}
	if children.iter().all(|node| node.connection_bounds()[1] >= aa[3] + CLEARANCE) {
		let start = (a.connection_center().0, aa[3]);
		branches.push(Route::branch(start, (b.connection_center().0, bb[1]), false));
	}
	branches.into_iter().find_map(|direct| {
		if clears_cards(&direct, a, b, cards) {
			return Some(direct);
		}
		let start = direct.curves[0].start;
		if !ports(a, start, cards).iter().any(|port| port.anchor == start) {
			return None;
		}
		// Siblings share an exposed origin even when their number blocks a top port.
		let path = avoid_cards_from(direct, a, b, cards, Some(start));
		path.is_visible().then_some(path)
	})
}

fn avoid_cards(direct: Route, a: &Node, b: &Node, cards: &[Card]) -> Route {
	avoid_cards_from(direct, a, b, cards, None)
}

fn avoid_cards_from(
	direct: Route,
	a: &Node,
	b: &Node,
	cards: &[Card],
	origin: Option<Point>,
) -> Route {
	if clears_cards(&direct, a, b, cards) {
		return direct;
	}
	let curve = &direct.curves[0];
	let mut starts = ports(a, curve.start, cards);
	starts.retain(|port| origin.is_none_or(|origin| port.anchor == origin));
	let ends = ports(b, curve.end, cards);
	let mut pairs: Vec<_> = starts.iter().flat_map(|a| ends.iter().map(move |b| (a, b))).collect();
	pairs.sort_by(|(a, b), (c, d)| {
		let cost = |a: &Port, b: &Port| {
			distance(a.outer, b.outer)
				+ distance(a.anchor, curve.start)
				+ distance(b.anchor, curve.end)
		};
		cost(a, b).total_cmp(&cost(c, d))
	});
	// An activity column can cover a facing port without blocking the free space
	// between characters. Try exposed tangents before adding gutter corners.
	if a.attachment.is_some() || b.attachment.is_some() {
		for (start, end) in &pairs {
			let reach = (distance(start.anchor, end.anchor) * 0.35).clamp(24., 72.);
			let path = Route::curve(
				start.anchor,
				toward(start.anchor, start.outer, reach),
				toward(end.anchor, end.outer, reach),
				end.anchor,
			);
			if clears_cards(&path, a, b, cards) {
				return path;
			}
		}
	}
	for (start, end) in pairs {
		if let Some(mut points) =
			Grid::new(cards, start.outer, end.outer).path(start.outer, end.outer)
		{
			points.insert(0, start.anchor);
			points.push(end.anchor);
			return rounded_path(points, CLEARANCE);
		}
	}
	// A fully covered card has no exposed port. Do not draw a false shortcut
	// through other cards while it is hidden beneath them during a drag.
	Route::curve(curve.start, curve.start, curve.start, curve.start)
}

fn clears_cards(path: &Route, a: &Node, b: &Node, cards: &[Card]) -> bool {
	!cards.iter().any(|card| {
		card.key != a.key
			&& card.key != b.key
			&& path.curves.iter().any(|curve| card.blocks_curve(curve))
	})
}

struct Port {
	anchor: Point,
	outer: Point,
}

fn ports(node: &Node, preferred: Point, cards: &[Card]) -> Vec<Port> {
	let center = node.connection_center();
	let [left, top, right, bottom] = node.connection_bounds();
	let mut ports: Vec<_> = [
		((left, node.port_y()), (-CLEARANCE, 0.)),
		((right, node.port_y()), (CLEARANCE, 0.)),
		((center.0, top), (0., -CLEARANCE)),
		((center.0, bottom), (0., CLEARANCE)),
	]
	.into_iter()
	.map(|(anchor, offset)| Port { anchor, outer: (anchor.0 + offset.0, anchor.1 + offset.1) })
	.filter(|port| !cards.iter().any(|c| c.key != node.key && c.crosses(port.anchor, port.outer)))
	.collect();
	ports.sort_by(|a, b| distance(a.anchor, preferred).total_cmp(&distance(b.anchor, preferred)));
	ports
}

fn rounded_path(points: Vec<Point>, rounding: f32) -> Route {
	let mut corners: Vec<Point> = Vec::new();
	for point in points {
		while corners.len() >= 2 {
			let a = corners[corners.len() - 2];
			let b = corners[corners.len() - 1];
			if (a.0 == b.0 && b.0 == point.0) || (a.1 == b.1 && b.1 == point.1) {
				corners.pop();
			} else {
				break;
			}
		}
		corners.push(point);
	}
	Route::polyline(&corners, rounding)
}

struct Grid<'a> {
	x: Vec<f32>,
	y: Vec<f32>,
	cards: &'a [Card],
}

#[derive(PartialEq)]
struct Visit {
	priority: f32,
	cost: f32,
	state: usize,
}
impl Eq for Visit {}
impl PartialOrd for Visit {
	fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
		Some(self.cmp(other))
	}
}
impl Ord for Visit {
	fn cmp(&self, other: &Self) -> Ordering {
		other.priority.total_cmp(&self.priority).then_with(|| other.state.cmp(&self.state))
	}
}

impl<'a> Grid<'a> {
	fn new(cards: &'a [Card], start: Point, end: Point) -> Self {
		let mut x = vec![start.0, end.0];
		let mut y = vec![start.1, end.1];
		for card in cards {
			x.extend([card.bounds[0], card.bounds[2]]);
			y.extend([card.bounds[1], card.bounds[3]]);
		}
		x.sort_by(f32::total_cmp);
		x.dedup();
		y.sort_by(f32::total_cmp);
		y.dedup();
		Self { x, y, cards }
	}

	fn point(&self, state: usize) -> Point {
		let cell = state / 2;
		(self.x[cell % self.x.len()], self.y[cell / self.x.len()])
	}

	fn neighbors(&self, state: usize) -> impl Iterator<Item = (usize, usize)> {
		let cell = state / 2;
		let x = cell % self.x.len();
		let y = cell / self.x.len();
		[
			(x.checked_sub(1).map(|x| y * self.x.len() + x), 0),
			((x + 1 < self.x.len()).then_some(cell + 1), 0),
			(y.checked_sub(1).map(|y| y * self.x.len() + x), 1),
			((y + 1 < self.y.len()).then_some(cell + self.x.len()), 1),
		]
		.into_iter()
		.filter_map(|(cell, axis)| cell.map(|cell| (cell * 2 + axis, axis)))
	}

	fn path(&self, start: Point, end: Point) -> Option<Vec<Point>> {
		let index = |p: Point| {
			self.y.iter().position(|y| *y == p.1).expect("grid y") * self.x.len()
				+ self.x.iter().position(|x| *x == p.0).expect("grid x")
		};
		let target = index(end);
		let count = self.x.len() * self.y.len() * 2;
		let mut costs = vec![f32::INFINITY; count];
		let mut previous = vec![usize::MAX; count];
		let mut pending = BinaryHeap::new();
		for axis in 0..2 {
			let state = index(start) * 2 + axis;
			costs[state] = 0.;
			pending.push(Visit { priority: 0., cost: 0., state });
		}
		while let Some(Visit { cost, state, .. }) = pending.pop() {
			if cost > costs[state] {
				continue;
			}
			if state / 2 == target {
				let mut points = vec![self.point(state)];
				let mut cursor = state;
				while previous[cursor] != usize::MAX {
					cursor = previous[cursor];
					points.push(self.point(cursor));
				}
				points.reverse();
				return Some(points);
			}
			let a = self.point(state);
			for (next, axis) in self.neighbors(state) {
				let b = self.point(next);
				if self.cards.iter().any(|card| card.crosses(a, b)) {
					continue;
				}
				let next_cost = cost + distance(a, b) + if axis == state % 2 { 0. } else { 24. };
				if next_cost < costs[next] {
					costs[next] = next_cost;
					previous[next] = state;
					pending.push(Visit {
						priority: next_cost + (end.0 - b.0).abs() + (end.1 - b.1).abs(),
						cost: next_cost,
						state: next,
					});
				}
			}
		}
		None
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn character_connections_keep_the_source_and_targets_own_activity_columns_clear() {
		let mut a = node("parent", 0., 0.);
		a.attachment = Some([6., 6., 80., 42.]);
		for (x, y) in [(400., 0.), (400., 100.), (400., -100.), (0., 180.)] {
			let b = Node { key: "child".into(), x, y, ..a.clone() };
			let nodes = vec![a.clone(), b];
			let paths = Cache::default().paths(&nodes, &[edge("parent", "child")]);
			let path = &paths["parent-child"];
			assert!(path.is_visible(), "an exposed character must remain connected");
			assert_eq!(path.curves.len(), 1, "free character space needs no gutter elbows");
			for i in 0..=1000 {
				let (x, y) = path.at(i as f32 / 1000.);
				for node in &nodes {
					let [left, top, right, bottom] = node.connection_bounds();
					assert!(
						!(x > left + 0.01
							&& x < right - 0.01
							&& y > top + 0.01
							&& y < bottom - 0.01),
						"connection entered the character or HUD"
					);
					assert!(
						!(x > node.x + 83.
							&& x < node.x + node.width()
							&& y > node.y
							&& y < node.y + node.height()),
						"connection crossed {} activity at ({x}, {y})",
						node.key
					);
				}
			}
		}
	}

	#[test]
	fn resource_ports_follow_the_card_edges_at_every_camera_scale() {
		for scale in [1., 0.85, 0.7] {
			let a = node("parent", 0., 0.);
			for (x, y) in [(a.width() + 80. / scale, 0.), (0., a.height() + 80. / scale)] {
				let b = Node { key: "child".into(), x, y, ..a.clone() };
				let path = route(&a, &b);
				let start = path.curves.first().unwrap().start;
				let end = path.curves.last().unwrap().end;
				if x > 0. {
					assert_eq!(start, (a.width(), a.height() / 2.));
					assert_eq!(end, (b.x, b.height() / 2.));
				} else {
					assert_eq!(start, (a.width() / 2., a.height()));
					assert_eq!(end, (b.width() / 2., b.y));
				}
			}
		}
	}
	#[test]
	fn resource_branches_keep_facing_ports_and_smooth_tangents_at_every_scale() {
		for scale in [1., 0.85, 0.7] {
			let a = node("parent", 0., 0.);
			let b = Node {
				key: "child".into(),
				x: a.width() + 80. / scale,
				y: 100. / scale,
				..a.clone()
			};
			let paths = Cache::default().paths(&[a.clone(), b.clone()], &[edge("parent", "child")]);
			let route = &paths["parent-child"];
			assert_eq!(route.curves.len(), 1, "one continuous curve without small elbows");
			assert_eq!(route.curves[0].start, (a.width(), a.height() / 2.));
			assert_eq!(route.curves.last().unwrap().end, (b.x, b.port_y()));
			let curve = &route.curves[0];
			assert_eq!(curve.start.1, curve.first.1);
			assert_eq!(curve.second.1, curve.end.1);
			for pair in route.curves.windows(2) {
				assert_eq!(pair[0].end, pair[1].start);
				let before = (pair[0].end.0 - pair[0].second.0, pair[0].end.1 - pair[0].second.1);
				let after = (pair[1].first.0 - pair[1].start.0, pair[1].first.1 - pair[1].start.1);
				assert!((before.0 * after.1 - before.1 * after.0).abs() < 0.001);
				assert!(before.0 * after.0 + before.1 * after.1 > 0.);
			}
		}
	}

	fn node(key: &str, x: f32, y: f32) -> Node {
		Node {
			key: key.into(),
			navigation: (x, y),
			title: key.into(),
			assignment: String::new(),
			status: String::new(),
			color: 0,
			row: None,
			activity: Default::default(),
			size: (224., 48.),
			attachment: None,
			identity: None,
			base_height: 48.,
			layout_height: 48.,
			x,
			y,
		}
	}

	fn edge(from: &str, to: &str) -> Edge {
		Edge {
			in_progress: false,
			id: format!("{from}-{to}"),
			from: from.into(),
			to: to.into(),
			kind: Kind::Assigned,
			color: 0,
			label: "Owner".into(),
			detail: String::new(),
			excerpt: None,
			source: None,
			link: None,
		}
	}

	#[test]
	fn ownership_siblings_keep_smooth_facing_ports_and_distinct_landings() {
		let nodes = vec![
			node("main", 40., 160.),
			node("a", 400., 40.),
			node("b", 400., 160.),
			node("c", 400., 280.),
		];
		let edges = vec![edge("main", "a"), edge("main", "b"), edge("main", "c")];
		let paths = Cache::default().paths(&nodes, &edges);
		for (child, y) in [("a", 64.), ("b", 184.), ("c", 304.)] {
			let route = &paths[&format!("main-{child}")];
			assert_eq!(route.curves.len(), 1);
			let curve = &route.curves[0];
			assert_eq!(curve.start, (264., 184.));
			assert_eq!(curve.end, (400., y));
			assert_eq!(curve.start.1, curve.first.1);
			assert_eq!(curve.second.1, curve.end.1);
			for pair in route.samples.windows(2) {
				assert!(pair[0].0.0 < pair[1].0.0, "branches never double back");
			}
		}
		assert!(paths.values().all(|path| path.ink_arrow));
	}

	#[test]
	fn vertical_fan_uses_continuous_curves_without_card_crossings() {
		let nodes = vec![
			node("main", 400., 40.),
			node("a", 0., 240.),
			node("b", 300., 240.),
			node("c", 600., 240.),
			node("d", 900., 240.),
		];
		let edges: Vec<_> = ["a", "b", "c", "d"].iter().map(|child| edge("main", child)).collect();
		let paths = Cache::default().paths(&nodes, &edges);
		for route in paths.values() {
			assert_eq!(route.curves.len(), 1);
			let curve = &route.curves[0];
			assert_eq!(curve.start, (512., 88.));
			assert_eq!(curve.start.0, curve.first.0);
			assert_eq!(curve.second.0, curve.end.0, "land perpendicular to the card");
			for i in 1..1000 {
				let point = route.at(i as f32 / 1000.);
				for node in &nodes {
					assert!(
						!(point.0 > node.x
							&& point.0 < node.x + node.width()
							&& point.1 > node.y
							&& point.1 < node.y + node.height())
					);
				}
			}
		}
	}

	#[test]
	fn curve_avoidance_distinguishes_empty_control_hulls_from_real_obstacles() {
		let curve =
			Curve { start: (0., 0.), first: (150., 0.), second: (150., 100.), end: (300., 100.) };
		let beside = Card { key: "beside".into(), bounds: [250., 0., 280., 25.], port_y: 12.5 };
		let across = Card { key: "across".into(), bounds: [140., 40., 160., 60.], port_y: 50. };
		assert!(!beside.blocks_curve(&curve));
		assert!(across.blocks_curve(&curve));
	}

	#[test]
	fn connections_go_around_intervening_cards_and_keep_their_direction() {
		let nodes = vec![
			node("main", 0., 0.),
			node("middle", 320., 0.),
			node("end", 640., 0.),
			node("lower", 320., 120.),
		];
		let edges = vec![edge("main", "end"), edge("main", "lower")];
		let paths = Cache::default().paths(&nodes, &edges);
		assert!(paths["main-end"].curves.len() > 1, "the intervening card requires a detour");
		for path in paths.values() {
			assert!(path.is_visible());
			let reverse = path.reversed();
			for i in 0..=1000 {
				let t = i as f32 / 1000.;
				let point = path.at(t);
				let reflected = reverse.at(1. - t);
				assert!(distance(point, reflected) < 0.01);
				for node in &nodes {
					assert!(
						!(point.0 > node.x + 0.01
							&& point.0 < node.x + node.width() - 0.01
							&& point.1 > node.y + 0.01
							&& point.1 < node.y + node.height() - 0.01),
						"route enters {} at {point:?}",
						node.key
					);
				}
			}
		}
	}

	#[test]
	fn partially_covered_ports_use_an_exposed_side_instead_of_crossing_cards() {
		let nodes =
			vec![node("main", 0., 0.), node("middle", 320., 160.), node("child", 440., 180.)];
		let paths = Cache::default().paths(&nodes, &[edge("main", "child")]);
		let path = &paths["main-child"];
		assert!(path.is_visible());
		assert!(path.curves.len() > 1);
		for i in 0..=1000 {
			let point = path.at(i as f32 / 1000.);
			let blocker = &nodes[1];
			assert!(
				!(point.0 > blocker.x
					&& point.0 < blocker.x + 224.
					&& point.1 > blocker.y
					&& point.1 < blocker.y + blocker.height()),
				"route enters the overlapping card at {point:?}"
			);
		}
	}

	#[test]
	fn a_fully_covered_card_never_draws_a_shortcut_through_other_cards() {
		let mut nodes =
			vec![node("main", 0., 0.), node("cover", 640., 0.), node("child", 640., 0.)];
		let edges = [edge("main", "child")];
		let mut cache = Cache::default();
		assert!(!cache.paths(&nodes, &edges)["main-child"].is_visible());
		nodes[2].y += 160.;
		assert!(cache.paths(&nodes, &edges)["main-child"].is_visible());
	}

	#[test]
	fn animation_and_status_updates_reuse_geometry_but_dragging_reroutes() {
		let mut nodes = vec![node("main", 0., 0.), node("child", 640., 0.)];
		let edges = vec![edge("main", "child")];
		let mut cache = Cache::default();
		let first = cache.paths(&nodes, &edges);
		nodes[0].status = "Running".into();
		let unchanged = cache.paths(&nodes, &edges);
		assert!(Rc::ptr_eq(&first["main-child"], &unchanged["main-child"]));
		nodes[1].y += 160.;
		let moved = cache.paths(&nodes, &edges);
		assert!(!Rc::ptr_eq(&first["main-child"], &moved["main-child"]));
		assert_eq!(moved["main-child"].curves.last().expect("endpoint").end.1, 184.);
	}

	#[test]
	fn flow_samples_include_short_corners_between_long_segments() {
		let route =
			Route::polyline(&[(0., 0.), (1000., 0.), (1000., 48.), (2000., 48.)], CLEARANCE);
		for curve in &route.curves {
			assert!(route.samples.iter().any(|(point, _)| distance(*point, curve.end) < 0.001));
		}
		assert!(route.samples.windows(2).all(|pair| pair[0].1 <= pair[1].1));
	}

	#[test]
	fn shared_rails_keep_one_stroke_and_complete_individual_routes() {
		let mut a = Route::polyline(&[(0., 0.), (100., 0.), (100., 200.)], CLEARANCE);
		let mut b = Route::polyline(&[(0., 0.), (100., 0.), (100., 300.), (200., 300.)], CLEARANCE);
		let original = b.samples.clone();
		let annotation = b.label;
		let mut ink = Ink::default();
		ink.retain(&mut a);
		ink.retain(&mut b);
		let crosses = |curve: &Curve| {
			curve.start.0 == 100.
				&& curve.end.0 == 100.
				&& curve.start.1.min(curve.end.1) < 150.
				&& curve.start.1.max(curve.end.1) > 150.
		};
		assert_eq!(a.curves.iter().chain(&b.curves).filter(|c| crosses(c)).count(), 2);
		assert_eq!(a.ink.iter().chain(&b.ink).filter(|c| crosses(c)).count(), 1);
		assert_eq!(b.samples, original);
		assert_eq!(b.reversed().curves.first().expect("start").start, (200., 300.));
		assert_eq!(b.label, annotation, "ink batching must not move annotations");
		let mut duplicate = a.clone();
		ink.retain(&mut duplicate);
		assert!(duplicate.ink.is_empty());
		assert!(!duplicate.ink_arrow);
	}

	#[test]
	fn shared_strokes_preserve_gaps_and_handle_reverse_direction() {
		let mut ink = Ink::default();
		assert_eq!(ink.take(&Curve::line((60., 0.), (40., 0.))).len(), 1);
		let fragments = ink.take(&Curve::line((0., 0.), (100., 0.)));
		assert_eq!(fragments.len(), 2);
		assert_eq!((fragments[0].start, fragments[0].end), ((0., 0.), (40., 0.)));
		assert_eq!((fragments[1].start, fragments[1].end), ((60., 0.), (100., 0.)));
		assert!(ink.take(&Curve::line((100., 0.), (0., 0.))).is_empty());
		assert_eq!(ink.take(&Curve::line((0., 8.), (100., 8.))).len(), 1);
	}

	#[test]
	fn stroke_style_changes_invalidate_cached_ink() {
		let nodes = vec![node("main", 0., 0.), node("child", 400., 0.)];
		let mut edges = vec![edge("main", "child")];
		let mut cache = Cache::default();
		let first = cache.paths(&nodes, &edges);
		edges[0].color = 0x123456;
		let colored = cache.paths(&nodes, &edges);
		assert!(!Rc::ptr_eq(&first["main-child"], &colored["main-child"]));
		edges[0].kind = Kind::Context;
		let dashed = cache.paths(&nodes, &edges);
		assert!(!Rc::ptr_eq(&colored["main-child"], &dashed["main-child"]));
		assert_eq!(dashed["main-child"].ink.len(), dashed["main-child"].curves.len());
	}
}
