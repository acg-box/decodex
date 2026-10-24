//! A bounded conversation timeline. Browsing freezes the visible page.
use crate::{ui_motion, ui_scroll, ui_theme};
use decodex_protocol::{
	AgentClient, AgentDeliveriesResult, AgentDeliveryEntry, AgentDeliveryKind as Kind,
	AgentDeliveryQuery, ClientProfile, EntityId,
};
use gpui::{
	Context, EventEmitter, IntoElement, MouseButton, Render, ScrollHandle, Task, Window, prelude::*,
};
use std::{
	collections::{BTreeMap, BTreeSet},
	time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const LIMIT: u16 = 20;
const DATE_HEIGHT: f32 = 26.;
const ENTRANCE: Duration = Duration::from_millis(220);

pub(super) struct OpenDelivery(pub String);

#[path = "agent_delivery_filters.rs"] mod filters;
#[path = "agent_delivery_rows.rs"] mod rows;

#[derive(Default)]
struct Feed {
	entries: Vec<AgentDeliveryEntry>,
	pending: Option<Vec<AgentDeliveryEntry>>,
	arrivals: BTreeMap<i64, Instant>,
	loaded: bool,
}
impl Feed {
	fn receive(&mut self, mut entries: Vec<AgentDeliveryEntry>, browsing: bool) -> bool {
		for entry in &mut entries {
			entry.summary = plain_body(&entry.summary);
		}
		if self.loaded && entries == self.entries {
			return self.pending.take().is_some();
		}
		if self.loaded && browsing {
			if self.pending.as_ref() == Some(&entries) {
				return false;
			}
			self.pending = Some(entries);
			return true;
		}
		let now = Instant::now();
		self.arrivals.clear();
		if self.loaded {
			for entry in &entries {
				if !self.entries.iter().any(|old| old.id == entry.id) {
					self.arrivals.insert(entry.id, now);
				}
			}
		}
		self.entries = entries;
		self.pending = None;
		self.loaded = true;
		true
	}

	fn accept_pending(&mut self) {
		if let Some(entries) = self.pending.take() {
			self.receive(entries, false);
		}
	}
}

pub(super) struct Panel {
	width: f32,
	feed: Feed,
	scroll: ScrollHandle,
	motion: Option<ui_scroll::Motion>,
	profile: Option<ClientProfile>,
	request: Option<Task<()>>,
	last_read: Option<Instant>,
	generation: u64,
	filter: filters::State,
	failed: bool,
	expanded: Option<i64>,
	active_work: BTreeSet<String>,
	layouts: BTreeMap<i64, rows::CachedLayout>,
}
impl Default for Panel {
	fn default() -> Self {
		Self {
			width: 240.,
			feed: Feed::default(),
			scroll: ScrollHandle::new(),
			motion: None,
			profile: None,
			request: None,
			last_read: None,
			generation: 0,
			filter: filters::State::default(),
			failed: false,
			expanded: None,
			active_work: BTreeSet::new(),
			layouts: BTreeMap::new(),
		}
	}
}
impl EventEmitter<OpenDelivery> for Panel {}

impl Panel {
	pub(super) fn set_width(&mut self, width: f32, cx: &mut Context<Self>) {
		if (self.width - width).abs() > 0.1 {
			self.width = width;
			cx.notify();
		}
	}

	fn refresh(
		&mut self,
		profile: ClientProfile,
		agents: Vec<filters::AgentChoice>,
		active_work: BTreeSet<String>,
		cx: &mut Context<Self>,
	) {
		self.active_work = active_work;
		if self.profile.as_ref() != Some(&profile) {
			self.profile = Some(profile);
			self.generation += 1;
			self.request = None;
			self.last_read = None;
			self.feed = Feed::default();
			self.layouts.clear();
			self.filter = filters::State::default();
			self.motion = None;
			self.expanded = None;
			self.failed = false;
			self.scroll.set_offset(gpui::point(gpui::px(0.), gpui::px(0.)));
		}
		if self.filter.agents != agents {
			self.filter.agents = agents;
			cx.notify();
		}
		if self.request.is_some()
			|| self.last_read.is_some_and(|at| at.elapsed() < Duration::from_secs(2))
		{
			return;
		}
		let profile = self.profile.clone().expect("profile set before read");
		let filters::TimeRange { after, before } = self.filter.range;
		let query = AgentDeliveryQuery {
			agent_id: self.filter.agent.clone().and_then(|s| EntityId::new(s).ok()),
			after,
			before,
			limit: LIMIT,
		};
		let generation = self.generation;
		self.last_read = Some(Instant::now());
		let read = cx.background_executor().spawn(async move {
			let runtime = tokio::runtime::Builder::new_current_thread()
				.enable_all()
				.build()
				.map_err(|_| ())?;
			runtime.block_on(AgentClient::new(profile).deliveries(query)).map_err(|_| ())
		});
		self.request = Some(cx.spawn(async move |panel, cx| {
			let result = read.await;
			let _ = panel.update(cx, |s, cx| {
				if s.generation != generation {
					return;
				}
				s.request = None;
				match result {
					Ok(AgentDeliveriesResult::Available { entries, extent }) => {
						let changed = s.filter.receive_extent(extent);
						s.failed = false;
						if s.feed.receive(
							entries,
							s.expanded.is_some()
								|| s.scroll.offset().y < gpui::px(-2.)
								|| s.motion.as_ref().is_some_and(|m| m.to < -2.),
						) || changed
						{
							cx.notify();
						}
					},
					_ => {
						s.failed = true;
						cx.notify();
					},
				}
			});
		}));
	}

	fn reload(&mut self, cx: &mut Context<Self>) {
		self.generation += 1;
		self.request = None;
		self.last_read = None;
		self.filter.menu = false;
		self.filter.time_menu = false;
		self.failed = false;
		self.feed.pending = None;
		self.expanded = None;
		self.motion = None;
		self.scroll.set_offset(gpui::point(gpui::px(0.), gpui::px(0.)));
		if let Some(profile) = self.profile.clone() {
			self.refresh(profile, self.filter.agents.clone(), self.active_work.clone(), cx);
		}
		cx.notify();
	}

	fn wheel(&mut self, event: &gpui::ScrollEvent, cx: &mut Context<Self>) {
		let delta = f32::from(event.delta.pixel_delta(gpui::px(ui_theme::BODY_LINE_HEIGHT)).y);
		if delta == 0. {
			return;
		}
		let current = f32::from(self.scroll.offset().y);
		let maximum = f32::from(self.scroll.max_offset().y).max(0.);
		if ui_scroll::smooth(event.delta) {
			self.motion
				.get_or_insert_with(|| ui_scroll::Motion::new(current, Instant::now()))
				.retarget(current, delta, maximum, Instant::now());
		} else {
			self.motion = None;
			self.scroll.set_offset(gpui::point(
				gpui::px(0.),
				gpui::px((current + delta).clamp(-maximum, 0.)),
			));
		}
		cx.stop_propagation();
		cx.notify();
	}

	fn toggle_entry(&mut self, id: i64, cx: &mut Context<Self>) {
		self.expanded = if self.expanded == Some(id) { None } else { Some(id) };
		cx.notify();
	}

	fn latest(&mut self, cx: &mut Context<Self>) {
		self.expanded = None;
		let current = f32::from(self.scroll.offset().y);
		if ui_scroll::enabled() && current < -2. {
			let mut motion = ui_scroll::Motion::new(current, Instant::now());
			motion.retarget(
				current,
				-current,
				f32::from(self.scroll.max_offset().y),
				Instant::now(),
			);
			self.motion = Some(motion);
		} else {
			self.scroll.set_offset(gpui::point(gpui::px(0.), gpui::px(0.)));
			self.feed.accept_pending();
		}
		cx.notify();
	}
}

impl Render for Panel {
	fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		if let Some(motion) = &self.motion {
			let (position, moving) = if ui_scroll::enabled() {
				motion.sample(Instant::now())
			} else {
				(motion.to, false)
			};
			self.scroll.set_offset(gpui::point(gpui::px(0.), gpui::px(position)));
			if moving {
				ui_motion::request_frame(window, cx);
			} else {
				self.motion = None;
			}
		}
		if self.expanded.is_none()
			&& self.scroll.offset().y >= gpui::px(-1.)
			&& self.motion.is_none()
		{
			self.feed.accept_pending();
		}
		let list = self.conversation_list(window, cx);
		let mut panel = gpui::div()
			.relative()
			.capture_any_mouse_down(cx.listener(|s, _, _, _| s.motion = None))
			.on_key_down(cx.listener(|s, e: &gpui::KeyDownEvent, _, cx| {
				if e.keystroke.key == "escape" && (s.filter.menu || s.filter.time_menu) {
					s.filter.menu = false;
					s.filter.time_menu = false;
					cx.notify();
					cx.stop_propagation();
				}
			}))
			.size_full()
			.flex()
			.flex_col()
			.text_color(crate::ui_theme::rgb(ui_theme::TEXT))
			.child(self.header(cx))
			.child(gpui::div().relative().flex_1().min_h_0().flex().flex_col().child(list).child(
				ui_scroll::Scrollbar::new(
					"delivery-scrollbar",
					"Scroll conversation timeline",
					self.scroll.clone(),
				),
			));
		panel = panel.child(self.latest_notice(cx));
		panel.child(self.filter_menu(cx))
	}
}

impl Panel {
	fn conversation_list(
		&mut self,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> gpui::AnyElement {
		let mut list = gpui::div()
			.id("delivery-scroll")
			.debug_selector(|| "delivery-scroll".into())
			.flex_1()
			.min_h_0()
			.px(gpui::px(12.))
			.pb(gpui::px(8.))
			.overflow_hidden()
			.track_scroll(&self.scroll)
			.on_scroll(cx.listener(|s, e, _, cx| s.wheel(e, cx)));
		let mut previous = None;
		let mut offset = 0.;
		let top = -f32::from(self.scroll.offset().y);
		let viewport = f32::from(self.scroll.bounds().size.height);
		let bottom =
			top + if viewport > 0. { viewport } else { f32::from(window.viewport_size().height) };
		self.layouts.retain(|id, _| self.feed.entries.iter().any(|entry| entry.id == *id));
		let mut rows = gpui::div().relative().w_full().flex_none();
		let mut rail_start = None;
		let mut rail_end = 0.;
		let mut visible_height: f32 = 0.;
		for entry in &self.feed.entries {
			let day = day_key(entry.updated_at_micros);
			let show = previous != Some(day);
			previous = Some(day);
			let layout = rows::CachedLayout::get(
				&mut self.layouts,
				entry,
				self.width - 24. - rows::RAIL_WIDTH - rows::CONTENT_GAP,
				self.expanded == Some(entry.id),
				window,
			);
			let height = layout.height + if show { DATE_HEIGHT } else { 0. };
			let card_height = ui_motion::value(
				gpui::SharedString::from(format!("delivery-height-{}", entry.id)),
				layout.height,
				window,
				cx,
			);
			let position = ui_motion::value(
				gpui::SharedString::from(format!("delivery-position-{}", entry.id)),
				offset,
				window,
				cx,
			);
			let marker = position + if show { DATE_HEIGHT } else { 0. } + rows::MARKER_Y;
			rail_start.get_or_insert(marker);
			rail_end = marker;
			visible_height =
				visible_height.max(position + card_height + if show { DATE_HEIGHT } else { 0. });
			// Preserve full scroll geometry while laying out only the visible rows and
			// one row of overscan. Offscreen cards never take part in per-frame paint.
			if position + height >= top - 120. && position <= bottom + 120. {
				let row = self.row(
					entry,
					&layout,
					rows::Placement { date: show, height: card_height },
					window,
					cx,
				);
				rows = rows.child(
					gpui::div().absolute().left_0().right_0().top(gpui::px(position)).child(row),
				);
			}
			offset += height;
		}
		if let Some(start) = rail_start {
			rows = gpui::div()
				.relative()
				.w_full()
				.flex_none()
				.child(
					gpui::div()
						.absolute()
						.left(gpui::px((rows::RAIL_WIDTH - 2.) / 2.))
						.top(gpui::px(start))
						.h(gpui::px((rail_end - start).max(0.)))
						.w(gpui::px(2.))
						.rounded_full()
						.bg(crate::ui_theme::rgba(0xaaa6b84a)),
				)
				.child(rows.h(gpui::px(visible_height)));
		}
		list = list.child(rows.h(gpui::px(visible_height)));

		if self.feed.entries.is_empty() {
			list = list.child(
				gpui::div()
					.p_3()
					.text_size(gpui::px(12.))
					.text_color(crate::ui_theme::rgb(ui_theme::TEXT_MUTED))
					.child(if self.failed {
						"Could not load conversations. Retry"
					} else if self.feed.loaded {
						if self.filter.range.after.is_some() || self.filter.range.before.is_some() {
							"No conversations in this period"
						} else {
							"Your conversations appear here"
						}
					} else {
						"Loading conversations…"
					})
					.on_mouse_down(
						MouseButton::Left,
						cx.listener(|s, _, _, cx| {
							if s.failed {
								s.reload(cx);
							}
						}),
					),
			);
		}
		list.into_any_element()
	}

	fn latest_notice(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
		let button = gpui::div()
			.id("delivery-latest")
			.debug_selector(|| "delivery-latest".into())
			.role(gpui::Role::Button)
			.tab_index(0)
			.aria_label("New updates. Back to latest")
			.cursor_pointer()
			.rounded(gpui::px(8.))
			.py(gpui::px(7.))
			.px(gpui::px(10.))
			.bg(crate::ui_theme::rgba(0x30303af0))
			.text_size(gpui::px(11.5))
			.child("New updates · Back to latest")
			.on_click(cx.listener(|s, _, _, cx| s.latest(cx)))
			.on_key_down(cx.listener(|s, e: &gpui::KeyDownEvent, _, cx| {
				if !e.is_held && ["enter", "space"].contains(&e.keystroke.key.as_str()) {
					s.latest(cx);
					cx.stop_propagation();
				}
			}))
			.smooth();
		gpui::div()
			.absolute()
			.bottom(gpui::px(12.))
			.left(gpui::px(42.))
			.right(gpui::px(12.))
			.child(ui_motion::disclosure(
				"delivery-latest-notice",
				self.feed.pending.is_some(),
				button,
			))
			.into_any_element()
	}
}

use crate::ui_motion::SmoothControl;

fn plain_body(markdown: &str) -> String {
	use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
	let mut text = String::new();
	for event in Parser::new_ext(markdown, Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH) {
		match event {
			Event::Text(value)
			| Event::Code(value)
			| Event::Html(value)
			| Event::InlineHtml(value) => text.push_str(&value),
			Event::SoftBreak => text.push(' '),
			Event::HardBreak | Event::End(TagEnd::Item | TagEnd::TableRow | TagEnd::TableHead) =>
				text.push('\n'),
			Event::Start(Tag::Item) => text.push_str("• "),
			Event::End(TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::CodeBlock) =>
				text.push_str("\n\n"),
			Event::End(TagEnd::TableCell) => text.push_str("  "),
			_ => {},
		}
	}
	text.trim().to_owned()
}

fn text_height(
	text: &str,
	size: f32,
	line_height: f32,
	weight: gpui::FontWeight,
	width: f32,
	limit: Option<usize>,
	window: &Window,
) -> f32 {
	let mut font = window.text_style().font();
	font.weight = weight;
	let run = gpui::TextRun {
		len: text.len(),
		font,
		color: crate::ui_theme::rgb(ui_theme::TEXT).into(),
		background_color: None,
		underline: None,
		strikethrough: None,
	};
	window
		.text_system()
		.shape_text(
			text.to_owned().into(),
			gpui::px(size),
			&[run],
			Some(gpui::px(width.max(1.))),
			// GPUI omits line_clamp from its wrapping cache key. Keep one
			// full layout for both states and clip the measured presentation below.
			None,
		)
		.map_or(line_height, |lines| {
			let height = lines
				.iter()
				.map(|line| f32::from(line.size(gpui::px(line_height)).height))
				.sum::<f32>();
			limit.map_or(height, |limit| height.min(line_height * limit as f32))
		})
}

fn now_micros() -> i64 {
	SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.unwrap_or_default()
		.as_micros()
		.min(i64::MAX as u128) as i64
}
fn local(micros: i64) -> libc::tm {
	let seconds = (micros / 1_000_000) as libc::time_t;
	let mut result = std::mem::MaybeUninit::<libc::tm>::zeroed();
	// SAFETY: both pointers reference initialized storage of the expected C types.
	unsafe {
		libc::localtime_r(&seconds, result.as_mut_ptr());
		result.assume_init()
	}
}
fn day_key(micros: i64) -> (i32, i32, i32) {
	let d = local(micros);
	(d.tm_year, d.tm_mon, d.tm_mday)
}
fn clock(micros: i64) -> String {
	let d = local(micros);
	format!("{:02}:{:02}", d.tm_hour, d.tm_min)
}
fn day_start(micros: i64, offset: i32) -> i64 {
	let mut d = local(micros);
	d.tm_hour = 0;
	d.tm_min = 0;
	d.tm_sec = 0;
	d.tm_mday += offset;
	d.tm_isdst = -1;
	// SAFETY: mktime accepts an initialized local calendar and normalizes day overflow.
	unsafe { libc::mktime(&mut d) as i64 * 1_000_000 }
}
fn day_label(micros: i64) -> String {
	let now = now_micros();
	let d = local(micros);
	let date = format!(
		"{} {}",
		["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"]
			[d.tm_mon as usize],
		d.tm_mday
	);
	if day_key(micros) == day_key(now) {
		"Today".into()
	} else if day_key(micros) == day_key(day_start(now, -1)) {
		"Yesterday".into()
	} else {
		format!("{date}, {}", d.tm_year + 1900)
	}
}

impl super::AgentSurface {
	pub(super) fn refresh_deliveries(&mut self, cx: &mut Context<Self>) {
		let Some(profile) = self.profile.clone() else {
			return;
		};
		let mut agents = self.snapshot.as_ref().map_or_else(Vec::new, |snapshot| {
			snapshot
				.work_items
				.iter()
				.filter(|work| !snapshot.archived_work_ids.contains(&work.id))
				.filter_map(|work| {
					let slot = snapshot.agent_slots.get(&work.id).copied()?;
					Some(filters::AgentChoice {
						id: work.id.clone(),
						label: work.title.clone(),
						slot,
					})
				})
				.collect::<Vec<_>>()
		});
		agents.sort_by_key(|agent| agent.slot);
		self.deliveries.update(cx, |panel, cx| {
			panel.refresh(
				profile,
				agents,
				self.snapshot.as_ref().map_or_else(BTreeSet::new, |s| {
					s.work_items.iter().map(|w| w.id.clone()).collect()
				}),
				cx,
			)
		});
	}
}

#[cfg(any(test, feature = "visual-capture"))]
impl super::AgentSurface {
	pub(crate) fn visual_delivery_fixture(&mut self, cx: &mut Context<Self>) {
		let now = now_micros();
		let samples = [
			(
				"Sign-in recovery",
				"The retry fix and regression checks are ready.",
				Kind::Result,
				"release",
			),
			(
				"Microphone startup",
				"Choose the default audio-input behavior.",
				Kind::NeedsInput,
				"verify",
			),
			(
				"Wide-screen graph layout",
				"Routing update reviewed and accepted.",
				Kind::Accepted,
				"website",
			),
			("Reading plan", "A flexible fifteen-minute reading plan.", Kind::Result, "flow"),
			(
				"Session recovery",
				"Checking draft restoration across restarts.",
				Kind::Working,
				"model",
			),
			(
				"Markdown rendering",
				"Checking list and code-block spacing.",
				Kind::Working,
				"markdown",
			),
		];
		self.workspace.agent_tree_visible = true;
		self.workspace.agent_panel_width = std::env::var("DECODEX_VISUAL_DELIVERY_WIDTH")
			.ok()
			.and_then(|s| s.parse().ok())
			.unwrap_or(272.);
		self.deliveries.update(cx, |panel, cx| {
			panel.filter.agents = vec![filters::AgentChoice {
				id: "release".into(),
				label: "Sign-in recovery".into(),
				slot: 1,
			}];
			panel.filter.extent = Some(decodex_protocol::AgentDeliveryExtent {
				first: now - 90 * 86_400_000_000,
				last: now,
			});
			panel.feed.receive(
				samples
					.into_iter()
					.enumerate()
					.map(|(i, (title, summary, kind, work))| AgentDeliveryEntry {
						id: i as i64 + 1,
						work_id: work.into(),
						title: title.into(),
						slot: Some(i as u32 + 1),
						kind,
						summary: summary.into(),
						artifacts: Vec::new(),
						updates: Vec::new(),
						updated_at_micros: now - i as i64 * 900_000_000,
					})
					.collect(),
				false,
			);
			let extent = panel.filter.extent;
			panel.filter.receive_extent(extent);
			if std::env::var("DECODEX_VISUAL_DELIVERY_MENU").is_ok_and(|v| v == "agents") {
				panel.toggle_filter(cx);
			}
			cx.notify();
		});
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	fn entry(id: i64) -> AgentDeliveryEntry {
		AgentDeliveryEntry {
			id,
			work_id: format!("work-{id}"),
			title: "Result".into(),
			slot: Some(1),
			kind: Kind::Result,
			summary: "Ready".into(),
			artifacts: Vec::new(),
			updates: Vec::new(),
			updated_at_micros: id,
		}
	}
	#[test]
	fn markdown_excerpt_stays_plain_and_unchanged_refresh_is_quiet() {
		let mut row = entry(1);
		row.summary = "**Ready** with `code` and [notes](https://example.com).".into();
		let mut feed = Feed::default();
		feed.receive(vec![row.clone()], false);
		assert_eq!(feed.entries[0].summary, "Ready with code and notes.");
		assert!(!feed.receive(vec![row], false));
		assert_eq!(
			plain_body("| Book | Author |\n| --- | --- |\n| Dune | Herbert |"),
			"Book  Author  \nDune  Herbert"
		);
	}
	#[test]
	fn browsing_preserves_exact_rows_until_returning_to_latest() {
		let mut feed = Feed::default();
		feed.receive(vec![entry(2), entry(1)], false);
		feed.receive(vec![entry(3), entry(2)], true);
		assert_eq!(feed.entries, vec![entry(2), entry(1)]);
		feed.receive(vec![entry(4), entry(3)], true);
		assert_eq!(feed.entries, vec![entry(2), entry(1)]);
		feed.accept_pending();
		assert_eq!(feed.entries, vec![entry(4), entry(3)]);
		assert!(feed.pending.is_none());
	}
	#[test]
	fn initial_load_and_unchanged_refresh_do_not_replay_entrance() {
		let mut f = Feed::default();
		f.receive(vec![entry(1)], false);
		assert!(f.arrivals.is_empty());
		assert!(!f.receive(vec![entry(1)], false));
		f.receive(vec![entry(2), entry(1)], false);
		assert_eq!(f.arrivals.keys().copied().collect::<Vec<_>>(), vec![2]);
	}
	#[test]
	fn empty_result_finishes_loading_and_reverted_updates_clear_the_notice() {
		let mut feed = Feed::default();
		assert!(feed.receive(vec![], false));
		assert!(feed.loaded);
		feed.receive(vec![entry(1)], false);
		assert!(feed.receive(vec![entry(2)], true));
		assert!(!feed.receive(vec![entry(2)], true));
		assert!(feed.receive(vec![entry(1)], true));
		assert!(feed.pending.is_none());
	}
	#[gpui::test]
	fn expansion_and_new_arrivals_do_not_leave_a_tall_collapsed_card(
		cx: &mut gpui::TestAppContext,
	) {
		let (panel, visual) = cx.add_window_view(|_, _| Panel::default());
		let mut row = entry(2);
		row.title = "Help me plan a quiet fifteen-minute reading break with one short question, then give me the complete plan after I choose. Keep every detail in the final response.".into();
		row.summary = "Walk at an easy pace for seven minutes along a quiet, familiar route, then pause for one minute to enjoy your surroundings. Spend the final seven minutes strolling back, with no need to rush. Take a light jacket and enjoy the rest of the afternoon.".into();
		panel.update(visual, |p, _| {
			p.feed.receive(vec![row.clone(), entry(1)], false);
		});
		visual.simulate_resize(gpui::size(gpui::px(240.), gpui::px(740.)));
		visual.update(crate::ui_frame::draw_frame);
		let compact = visual.debug_bounds("delivery-2").unwrap().size.height;
		panel.update(visual, |p, cx| p.toggle_entry(2, cx));
		visual.update(crate::ui_frame::draw_frame);
		std::thread::sleep(Duration::from_millis(220));
		visual.update(crate::ui_frame::draw_frame);
		assert!(visual.debug_bounds("delivery-2").unwrap().size.height > compact + gpui::px(50.));
		assert!(visual.debug_bounds("delivery-request-2").unwrap().size.height > gpui::px(34.));
		assert!(visual.debug_bounds("delivery-response-2").unwrap().size.height > gpui::px(64.));
		panel.update(visual, |p, cx| {
			p.feed.receive(vec![entry(3), row, entry(1)], true);
			p.latest(cx);
		});
		visual.update(crate::ui_frame::draw_frame);
		std::thread::sleep(Duration::from_millis(220));
		visual.update(crate::ui_frame::draw_frame);
		assert_eq!(visual.debug_bounds("delivery-2").unwrap().size.height, compact);
	}

	#[gpui::test]
	fn expanding_work_preserves_the_chat_and_holds_updates_until_closed(
		cx: &mut gpui::TestAppContext,
	) {
		use std::{cell::RefCell, rc::Rc};
		let opened = Rc::new(RefCell::new(Vec::new()));
		let (panel, visual) = cx.add_window_view(|_, _| Panel::default());
		let events = opened.clone();
		let _subscription = visual.update(|_, app| {
			app.subscribe(&panel, move |_, event: &OpenDelivery, _| {
				events.borrow_mut().push(event.0.clone())
			})
		});
		panel.update(visual, |p, _| {
			let mut result = entry(2);
			result.summary = "The guide is ready to review.".into();
			result.updates = vec![decodex_protocol::AgentDeliveryUpdate {
				id: 1,
				kind: decodex_protocol::AgentDeliveryUpdateKind::Decision,
				summary: "Essays — What would you like to read?".into(),
				created_at_micros: 1,
			}];
			p.feed.receive(vec![result], false);
			p.active_work.insert("work-2".into());
		});
		visual.simulate_resize(gpui::size(gpui::px(240.), gpui::px(740.)));
		visual.update(crate::ui_frame::draw_frame);
		let point = visual.debug_bounds("delivery-open-2").unwrap().center();
		visual.simulate_click(point, Default::default());
		assert_eq!(*opened.borrow(), vec!["work-2"]);
		panel.read_with(visual, |p, _| assert!(p.expanded.is_none()));
		opened.borrow_mut().clear();
		let point = visual.debug_bounds("delivery-expand-2").unwrap().center();
		visual.simulate_click(point, Default::default());
		visual.update(crate::ui_frame::draw_frame);
		panel.read_with(visual, |p, _| assert_eq!(p.expanded, Some(2)));
		assert!(opened.borrow().is_empty(), "expansion must not navigate the current conversation");
		std::thread::sleep(Duration::from_millis(220));
		visual.update(crate::ui_frame::draw_frame);
		assert!(visual.debug_bounds("delivery-open-2").is_some());
		panel.update(visual, |p, cx| {
			p.feed.receive(vec![entry(3)], true);
			cx.notify();
		});
		visual.update(crate::ui_frame::draw_frame);
		panel.read_with(visual, |p, _| assert_eq!(p.feed.entries[0].id, 2));
		let point = visual.debug_bounds("delivery-open-2").unwrap().center();
		visual.simulate_click(point, Default::default());
		assert_eq!(*opened.borrow(), vec!["work-2"]);
		panel.read_with(visual, |p, _| assert_eq!(p.expanded, Some(2)));
		panel.update(visual, |p, cx| p.toggle_entry(2, cx));
		visual.update(crate::ui_frame::draw_frame);
		panel.read_with(visual, |p, _| assert_eq!(p.feed.entries[0].id, 3));
	}

	#[gpui::test]
	fn narrow_timeline_filters_and_arrivals_preserve_reading_position(
		cx: &mut gpui::TestAppContext,
	) {
		let (panel, visual) = cx.add_window_view(|_, _| Panel::default());
		panel.update(visual, |p, _| {
			p.feed.receive((1..=20).rev().map(entry).collect(), false);
			p.filter.agents = vec![filters::AgentChoice {
				id: "work-20".into(),
				label: "A long agent name".into(),
				slot: 1,
			}];
		});
		for width in [160., 240., 320.] {
			panel.update(visual, |p, cx| p.set_width(width, cx));
			visual.simulate_resize(gpui::size(gpui::px(width), gpui::px(740.)));
			visual.update(|w, cx| {
				w.draw(cx).clear(cx);
			});
			let bounds = visual.debug_bounds("delivery-20").unwrap();
			assert!(bounds.right() <= gpui::px(width));
			let marker = visual.debug_bounds("delivery-marker-20").unwrap();
			assert!(marker.left() >= bounds.left());
			assert!(marker.right() <= bounds.left() + gpui::px(rows::RAIL_WIDTH));
			assert_eq!(marker.size, gpui::size(gpui::px(7.), gpui::px(7.)));
		}
		let viewport = visual.debug_bounds("delivery-scroll").unwrap();
		visual.simulate_event(gpui::ScrollEvent {
			position: viewport.center(),
			delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(-60.))),
			..Default::default()
		});
		visual.update(crate::ui_frame::draw_frame);
		panel.read_with(visual, |p, _| {
			assert_eq!(p.scroll.offset().y, gpui::px(-60.));
			assert!(p.scroll.max_offset().y > gpui::px(60.));
		});
		panel.update(visual, |p, cx| {
			p.scroll.set_offset(gpui::point(gpui::px(0.), gpui::px(-350.)));
			cx.notify();
		});
		visual.update(|w, cx| {
			w.draw(cx).clear(cx);
		});
		let before = panel.read_with(visual, |p, _| p.scroll.offset());
		assert!(visual.debug_bounds("delivery-16").is_some());
		panel.update(visual, |p, cx| {
			p.feed.receive((2..=21).rev().map(entry).collect(), true);
			cx.notify();
		});
		visual.update(|w, cx| {
			w.draw(cx).clear(cx);
		});
		panel.read_with(visual, |p, _| {
			assert_eq!(p.scroll.offset(), before);
			assert_eq!(p.feed.entries[0].id, 20);
			assert!(p.feed.pending.is_some());
		});
		assert!(visual.debug_bounds("delivery-latest").is_some());
		let point = visual.debug_bounds("delivery-agent-filter").unwrap().center();
		visual.simulate_click(point, Default::default());
		panel.read_with(visual, |p, _| assert!(p.filter.menu));
		visual.update(|w, cx| w.draw(cx).clear(cx));
		let point = visual.debug_bounds("delivery-agent-option-1").unwrap().center();
		visual.simulate_click(point, Default::default());
		panel.read_with(visual, |p, _| {
			assert_eq!(p.filter.agent.as_deref(), Some("work-20"));
			assert!(!p.filter.menu);
			assert_eq!(p.scroll.offset().y, gpui::px(0.));
		});
	}
}
