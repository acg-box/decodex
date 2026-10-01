//! Desktop lifecycle for optional recaps; the service retains inference ownership.
use std::{
	collections::BTreeSet,
	mem,
	time::{Duration, Instant},
};

use gpui::Subscription;
use tokio::{runtime::Builder, time};

use crate::shell::agent_surface::recap::*;
use decodex_protocol::{AgentTimelineContent, AgentTimelineResult};

const DELAY: Duration = Duration::from_secs(30 * 60);
const RETRY: Duration = Duration::from_secs(30);

pub(crate) struct Automatic {
	subscription: Option<Subscription>,
	focused: bool,
	enabled: bool,
	away: Option<Instant>,
	quiet: Option<Instant>,
	source: Option<Source>,
	epoch: u64,
	attempted: bool,
	retried: bool,
	retry_at: Option<Instant>,
	task: Option<Task<()>>,
	candidate: Vec<String>,
	last_recapped: Vec<String>,
	result: Option<String>,
	baseline: Option<(String, String, String)>,
}
impl Automatic {
	fn changed(&mut self, now: Instant) {
		self.epoch = self.epoch.wrapping_add(1);
		self.task = None;
		self.quiet = Some(now);
		self.attempted = false;
		self.retried = false;
		self.retry_at = None;

		self.candidate.clear();
	}

	fn due(&self, now: Instant) -> bool {
		if !self.enabled || self.focused {
			return false;
		}
		if self.attempted {
			return self.retry_at.is_some_and(|deadline| now >= deadline);
		}

		self.away.zip(self.quiet).is_some_and(|(away, quiet)| now >= away.max(quiet) + DELAY)
	}

	fn failed(&mut self, now: Instant) {
		if !self.retried {
			self.retry_at = Some(now + RETRY);
		}
	}
}

impl Default for Automatic {
	fn default() -> Self {
		Self {
			subscription: None,
			focused: true,
			enabled: false,
			away: None,
			quiet: None,
			source: None,
			epoch: 0,
			attempted: false,
			retried: false,
			retry_at: None,
			task: None,
			candidate: vec![],
			last_recapped: vec![],
			result: None,
			baseline: None,
		}
	}
}

#[derive(Clone, Eq, PartialEq)]
struct Source {
	work: String,
	thread: String,
	runtime: String,
	updated: i64,
	pending: i64,
	idle: bool,
}
impl Source {
	fn from_snapshot(snapshot: Option<&AgentSnapshotDto>, selected: Option<&str>) -> Option<Self> {
		let snapshot = snapshot?;
		let work = snapshot.work_items.iter().find(|work| Some(work.id.as_str()) == selected)?;

		Some(Self {
			work: work.id.clone(),
			thread: work.codex_thread_id.clone()?,
			runtime: snapshot.runtime_source.as_ref()?.as_str().into(),
			updated: work.updated_at_micros,
			pending: snapshot
				.pending_events
				.iter()
				.filter(|event| event.work_item_id == work.id)
				.map(|event| event.id)
				.max()
				.unwrap_or(0),
			idle: work.dispatch_state == AgentDispatchStateDto::Idle,
		})
	}

	fn same_owner(&self, other: &Self) -> bool {
		self.work == other.work && self.thread == other.thread && self.runtime == other.runtime
	}
}

impl AgentSurface {
	pub(in super::super) fn interrupt_automatic_recap(&mut self) {
		self.automatic_recap.changed(Instant::now());
		self.cancel_automatic_recap();
	}

	pub(in super::super) fn reset_automatic_recap(&mut self) {
		self.interrupt_automatic_recap();

		self.automatic_recap.source = None;

		self.automatic_recap.last_recapped.clear();

		self.automatic_recap.result = None;
		self.automatic_recap.baseline = None;
	}

	pub(super) fn invalidate_automatic_recap(&mut self, next: &AgentSnapshotDto) {
		let before = Source::from_snapshot(self.snapshot.as_ref(), self.selected.as_deref());
		let after = Source::from_snapshot(Some(next), self.selected.as_deref());

		if before != after {
			if matches!((&before, &after), (Some(a), Some(b)) if a.same_owner(b)) {
				self.interrupt_automatic_recap();
			} else {
				self.reset_automatic_recap();
			}
		}
	}

	pub(crate) fn observe_recap_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		if self.automatic_recap.subscription.is_none() {
			self.recap_focus(window.is_window_active());

			self.automatic_recap.subscription =
				Some(cx.observe_window_activation(window, |s, window, _| {
					s.recap_focus(window.is_window_active());
				}));
		}
	}

	fn recap_focus(&mut self, focused: bool) {
		if self.automatic_recap.focused == focused {
			return;
		}

		self.automatic_recap.focused = focused;
		self.automatic_recap.away = (!focused).then(Instant::now);

		self.automatic_recap.changed(Instant::now());

		if focused {
			self.cancel_automatic_recap();
		}
	}

	pub(super) fn record_recap_result(&mut self, state: &TaskRecapStatus) {
		if state.phase != Phase::Ready {
			return;
		}

		let (Some(id), Some(thread)) = (&state.request_id, &state.thread_id) else {
			return;
		};

		if self.automatic_recap.result.as_deref() == Some(id.as_str()) {
			return;
		}
		if self.recap.automatic && !self.automatic_recap.candidate.is_empty() {
			self.automatic_recap.last_recapped = mem::take(&mut self.automatic_recap.candidate);
			self.automatic_recap.result = Some(id.as_str().into());
			self.automatic_recap.baseline = None;
		} else {
			self.automatic_recap.baseline =
				Some((state.work_id.as_str().into(), thread.as_str().into(), id.as_str().into()));
		}
	}

	fn cancel_automatic_recap(&mut self) {
		if self.recap.automatic && self.recap.busy() {
			self.reset_recap();
		}
	}

	fn automatic_source(&mut self, now: Instant) -> Option<Source> {
		let source = Source::from_snapshot(self.snapshot.as_ref(), self.selected.as_deref());

		if source != self.automatic_recap.source {
			let same_owner = matches!((&source,&self.automatic_recap.source), (Some(a),Some(b)) if a.same_owner(b));

			if !same_owner {
				// A newly recorded manual baseline can precede the first automatic poll.
				if self.automatic_recap.source.is_some() {
					self.automatic_recap.baseline = None;
				}

				self.automatic_recap.last_recapped.clear();

				self.automatic_recap.result = None;
			}

			self.automatic_recap.source = source.clone();

			self.automatic_recap.changed(now);
			self.cancel_automatic_recap();
		}

		let source = source?;

		if !source.idle
			|| source.pending != 0
			|| self.native_agents.selected.is_some()
			|| self.voice.is_some()
			|| self.dictation.is_some()
		{
			self.automatic_recap.changed(now);
			self.cancel_automatic_recap();

			return None;
		}

		Some(source)
	}

	/// Called by the existing desktop lifecycle poll, including while the window is inactive.
	pub(crate) fn poll_automatic_recap(&mut self, enabled: bool, cx: &mut Context<Self>) {
		let now = Instant::now();

		if self.automatic_recap.enabled != enabled {
			self.automatic_recap.enabled = enabled;

			self.automatic_recap.changed(now);

			if !enabled {
				self.cancel_automatic_recap();
			}
		}
		if !enabled || !self.command_connection_ready() {
			return;
		}

		let Some(source) = self.automatic_source(now) else {
			return;
		};

		if self.automatic_recap.task.is_some() || self.recap.busy() {
			return;
		}

		let baseline = self
			.automatic_recap
			.baseline
			.as_ref()
			.filter(|(work, thread, _)| work == &source.work && thread == &source.thread)
			.map(|(_, _, id)| id.clone());

		if self.recap.automatic
			&& let Some(state) = &self.recap.state
			&& state.phase == Phase::Failed
			&& let Some(id) = &state.request_id
			&& self.automatic_recap.result.as_deref() != Some(id.as_str())
		{
			self.automatic_recap.result = Some(id.as_str().into());

			self.automatic_recap.failed(now);
		}
		if baseline.is_some()
			&& self.automatic_recap.attempted
			&& !self.automatic_recap.retry_at.is_some_and(|deadline| now >= deadline)
		{
			return;
		}
		if baseline.is_none() && !self.automatic_recap.due(now) {
			return;
		}

		self.read_automatic_progress(source, baseline, cx);
	}

	fn read_automatic_progress(
		&mut self,
		source: Source,
		baseline: Option<String>,
		cx: &mut Context<Self>,
	) {
		let Some(profile) = self.profile.clone() else {
			return;
		};
		let epoch = self.automatic_recap.epoch;

		{
			self.automatic_recap.retried |= self.automatic_recap.retry_at.take().is_some();
			self.automatic_recap.attempted = true;
		}

		let copy = source.clone();
		let read = cx.background_executor().spawn(async move {
			let runtime = Builder::new_current_thread().enable_all().build().ok()?;

			runtime.block_on(read_progress(profile, &copy.work, &copy.thread))
		});

		self.automatic_recap.task = Some(cx.spawn(async move |surface, cx| {
			let progress = read.await;
			let _ = surface.update(cx, |s, cx| {
				if s.automatic_recap.epoch != epoch {
					return;
				}

				s.automatic_recap.task = None;

				if !s.command_connection_ready()
					|| s.automatic_source(Instant::now()).as_ref() != Some(&source)
					|| s.automatic_recap.epoch != epoch
				{
					return;
				}

				let Some(progress) = progress else {
					s.automatic_recap.failed(Instant::now());

					return;
				};

				if let Some(id) = baseline {
					s.automatic_recap.last_recapped = progress;
					s.automatic_recap.result = Some(id);
					s.automatic_recap.baseline = None;
					s.automatic_recap.attempted = false;

					return;
				}

				if !has_new_progress(&progress, &s.automatic_recap.last_recapped) {
					return;
				}

				s.automatic_recap.candidate = progress;

				s.request_recap(&source.work, true, true, cx);
			});
		}));
	}
}

fn has_new_progress(current: &[String], previous: &[String]) -> bool {
	current.len() >= 3
		&& (previous.is_empty() || current.iter().filter(|id| !previous.contains(id)).count() >= 2)
}

async fn read_progress(profile: ClientProfile, work: &str, thread: &str) -> Option<Vec<String>> {
	time::timeout(Duration::from_secs(25), read_progress_inner(profile, work, thread))
		.await
		.ok()
		.flatten()
}

async fn read_progress_inner(
	profile: ClientProfile,
	work: &str,
	thread: &str,
) -> Option<Vec<String>> {
	let client = AgentClient::new(profile);
	let mut cursor = None;
	let mut seen = BTreeSet::new();
	let mut account = None;
	let mut completed = Vec::new();

	for _ in 0..8 {
		let result = client
			.timeline(EntityId::new(work).ok()?, EntityId::new(thread).ok()?, cursor)
			.await
			.ok()?;
		let AgentTimelineResult::Available { work_id, account_id, page } = result else {
			return None;
		};

		if work_id.as_str() != work
			|| page.thread_id != thread
			|| account.as_ref().is_some_and(|id| id != &account_id)
		{
			return None;
		}

		account = Some(account_id);

		for entry in page.entries.iter().rev() {
			if let AgentTimelineContent::TurnBoundary {
				turn_id,
				completed: true,
				status: Some(status),
				..
			} = &entry.content
				&& status == "completed"
				&& !completed.contains(turn_id)
			{
				completed.push(turn_id.clone());

				if completed.len() == 3 {
					return Some(completed);
				}
			}
		}

		let Some(next) = page.next_cursor else {
			return Some(completed);
		};

		if !seen.insert(next.clone()) {
			return None;
		}

		cursor = Some(WireText::new(next).ok()?);
	}

	None
}

#[cfg(test)]
mod tests {
	use crate::shell::agent_surface::recap::automatic::*;

	use std::future;

	use crate::shell::agent_surface::drafts::tests;

	#[test]
	fn deadline_requires_opt_in_and_thirty_minutes_after_latest_activity() {
		let now = Instant::now();
		let mut automatic =
			Automatic { away: Some(now), quiet: Some(now), focused: false, ..Default::default() };

		assert!(!automatic.due(now + DELAY));

		automatic.enabled = true;

		assert!(!automatic.due(now + DELAY - Duration::from_secs(1)));
		assert!(automatic.due(now + DELAY));

		automatic.changed(now + Duration::from_secs(60));

		assert!(!automatic.due(now + DELAY));
		assert!(automatic.due(now + DELAY + Duration::from_secs(60)));

		automatic.focused = true;

		assert!(!automatic.due(now + DELAY + Duration::from_secs(60)));
	}
	#[test]
	fn failed_attempt_has_one_bounded_retry_until_activity_changes() {
		let now = Instant::now();
		let mut automatic = Automatic {
			enabled: true,
			focused: false,
			away: Some(now - DELAY),
			quiet: Some(now - DELAY),
			attempted: true,
			..Default::default()
		};

		automatic.failed(now);

		assert!(!automatic.due(now));
		assert!(automatic.due(now + RETRY));

		automatic.retry_at = None;
		automatic.retried = true;

		automatic.failed(now + RETRY);

		assert!(!automatic.due(now + DELAY));

		automatic.changed(now + RETRY);

		assert!(automatic.due(now + DELAY + RETRY));
	}
	#[gpui::test]
	fn focus_gain_cancels_automatic_but_preserves_manual_requests(cx: &mut gpui::TestAppContext) {
		let surface = cx.new(AgentSurface::new);

		for automatic in [false, true] {
			let receiver = surface.update(cx, |s, cx| {
				s.recap_focus(false);

				let (cancel, receiver) = watch::channel(false);

				s.recap.cancel = Some(cancel);
				s.recap.automatic = automatic;
				s.recap.task = Some(cx.spawn(async |_, _| future::pending::<()>().await));

				s.recap_focus(true);

				receiver
			});

			assert_eq!(receiver.has_changed().is_err(), automatic);
		}
	}
	#[gpui::test]
	fn closing_a_manual_recap_keeps_its_progress_baseline_pending(cx: &mut gpui::TestAppContext) {
		let surface = cx.new(AgentSurface::new);

		surface.update(cx, |s, _| {
			let state = TaskRecapStatus {
				work_id: EntityId::new("work").unwrap(),
				thread_id: Some(WireText::new("thread").unwrap()),
				request_id: Some(WireText::new("manual").unwrap()),
				phase: Phase::Ready,
				recap: Some(decodex_protocol::TaskRecap {
					summary: WireText::new("Done").unwrap(),
					next_action: None,
				}),
			};

			s.record_recap_result(&state);
			s.reset_recap();

			assert_eq!(
				s.automatic_recap.baseline,
				Some(("work".into(), "thread".into(), "manual".into()))
			);
		});
	}

	#[gpui::test]
	fn running_work_preserves_completed_recap_baseline(cx: &mut gpui::TestAppContext) {
		let surface = cx.new(AgentSurface::new);

		surface.update(cx, |s, cx| {
			s.visual_workspace_fixture(cx);

			let work = s.selected.clone().unwrap();
			let snapshot = s.snapshot.as_mut().unwrap();

			snapshot.runtime_source = Some(EntityId::new("runtime").unwrap());

			snapshot.pending_events.clear();

			let item = snapshot.work_items.iter_mut().find(|w| w.id == work).unwrap();

			item.codex_thread_id = Some("thread".into());
			item.dispatch_state = AgentDispatchStateDto::Idle;

			s.poll_automatic_recap(true, cx);

			s.automatic_recap.last_recapped = vec!["a".into(), "b".into(), "c".into()];
			s.snapshot
				.as_mut()
				.unwrap()
				.work_items
				.iter_mut()
				.find(|w| w.id == work)
				.unwrap()
				.dispatch_state = AgentDispatchStateDto::Running;

			s.poll_automatic_recap(true, cx);

			assert_eq!(s.automatic_recap.last_recapped, vec!["a", "b", "c"]);
		});
	}
	#[gpui::test]
	fn lifecycle_changes_cancel_automatic_progress_before_its_completion(
		cx: &mut gpui::TestAppContext,
	) {
		let surface = cx.new(AgentSurface::new);

		for change in [
			"disconnect",
			"failed",
			"profile",
			"source",
			"thread",
			"activity",
			"pending",
			"removed",
			"navigation",
		] {
			surface.update(cx, |s, cx| {
				s.visual_workspace_fixture(cx);

				let work = s.selected.clone().unwrap();
				let snapshot = s.snapshot.as_mut().unwrap();

				snapshot.runtime_source = Some(EntityId::new("runtime").unwrap());

				snapshot.pending_events.clear();

				let item = snapshot.work_items.iter_mut().find(|w| w.id == work).unwrap();

				item.codex_thread_id = Some("thread".into());
				item.dispatch_state = AgentDispatchStateDto::Idle;

				s.automatic_source(Instant::now()).unwrap();

				s.automatic_recap.last_recapped = vec!["old-completed-turn".into()];

				let epoch = s.automatic_recap.epoch;

				s.automatic_recap.task = Some(cx.spawn(async |_, _| future::pending::<()>().await));

				let mut next = s.snapshot.clone().unwrap();

				match change {
					"disconnect" => s.mark_stale(cx),
					"failed" => s.apply_result(Err(())),
					"profile" => s.bind_profile(None, cx),
					"source" => next.runtime_source = Some(EntityId::new("replacement").unwrap()),
					"thread" =>
						next.work_items.iter_mut().find(|w| w.id == work).unwrap().codex_thread_id =
							Some("replacement".into()),
					"activity" =>
						next.work_items
							.iter_mut()
							.find(|w| w.id == work)
							.unwrap()
							.updated_at_micros += 1,
					"pending" =>
						next.work_items.iter_mut().find(|w| w.id == work).unwrap().dispatch_state =
							AgentDispatchStateDto::Running,
					"removed" => next.work_items.retain(|w| w.id != work),
					"navigation" => s.open_page("verify", cx),
					_ => unreachable!(),
				}

				if matches!(change, "source" | "thread" | "activity" | "pending" | "removed") {
					s.apply_result(Ok(AgentSnapshotResult::Available(next)));
				}

				assert!(s.automatic_recap.task.is_none(), "old progress read after {change}");
				assert_ne!(
					s.automatic_recap.epoch, epoch,
					"old completion authority after {change}"
				);

				if matches!(change, "disconnect" | "failed" | "activity" | "pending") {
					assert_eq!(s.automatic_recap.last_recapped, vec!["old-completed-turn"]);
				}
			});
		}
	}
	#[gpui::test]
	fn unchanged_snapshot_preserves_automatic_progress_and_manual_baseline(
		cx: &mut gpui::TestAppContext,
	) {
		let surface = cx.new(AgentSurface::new);

		surface.update(cx, |s, cx| {
			s.visual_workspace_fixture(cx);

			let work = s.selected.clone().unwrap();
			let snapshot = s.snapshot.as_mut().unwrap();

			snapshot.runtime_source = Some(EntityId::new("runtime").unwrap());

			snapshot.pending_events.clear();

			let item = snapshot.work_items.iter_mut().find(|w| w.id == work).unwrap();

			item.codex_thread_id = Some("thread".into());
			item.dispatch_state = AgentDispatchStateDto::Idle;

			s.automatic_source(Instant::now()).unwrap();

			s.automatic_recap.baseline = Some((work, "thread".into(), "manual".into()));

			let epoch = s.automatic_recap.epoch;

			s.automatic_recap.task = Some(cx.spawn(async |_, _| future::pending::<()>().await));
			s.generation += 1;

			s.apply_result(Ok(AgentSnapshotResult::Available(s.snapshot.clone().unwrap())));

			assert!(s.automatic_recap.task.is_some());
			assert_eq!(s.automatic_recap.epoch, epoch);
			assert!(s.automatic_recap.baseline.is_some());

			let mut next = s.snapshot.clone().unwrap();

			next.runtime_source = Some(EntityId::new("other-runtime").unwrap());

			s.apply_result(Ok(AgentSnapshotResult::Available(next)));

			assert!(s.automatic_recap.baseline.is_none());
		});
	}

	#[gpui::test]
	fn stale_connection_cannot_read_progress_or_generate_recap(cx: &mut gpui::TestAppContext) {
		let (_root, profile, _) = tests::profiles();
		let surface = cx.new(AgentSurface::new);

		surface.update(cx, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.profile = Some(profile);

			let work = s.selected.clone().unwrap();
			let snapshot = s.snapshot.as_mut().unwrap();

			snapshot.runtime_source = Some(EntityId::new("runtime").unwrap());

			snapshot.pending_events.clear();

			let item = snapshot.work_items.iter_mut().find(|w| w.id == work).unwrap();

			item.codex_thread_id = Some("thread".into());
			item.dispatch_state = AgentDispatchStateDto::Idle;

			s.automatic_source(Instant::now()).unwrap();

			s.automatic_recap.enabled = true;
			s.automatic_recap.focused = false;
			s.automatic_recap.away = Some(Instant::now() - DELAY);
			s.automatic_recap.quiet = s.automatic_recap.away;
			s.state = LoadState::Stale;

			s.poll_automatic_recap(true, cx);

			assert!(s.automatic_recap.task.is_none());

			s.request_recap(&work, true, true, cx);

			assert!(s.recap.task.is_none());
			assert!(s.recap.cancel.is_none());
			assert!(s.recap.feedback.contains("Wait for the service connection"));
		});
	}
}
#[cfg(test)]
#[path = "agent_recap_progress_tests.rs"]
mod progress_tests;

#[cfg(feature = "visual-capture")]
#[allow(dead_code, reason = "shared with the opt-in isolated native capture binary")]
impl AgentSurface {
	/// Exercise the real driver with an elapsed fixture clock, only in the capture binary.
	pub(crate) fn visual_begin_automatic_recap(
		&mut self,
		profile: ClientProfile,
		cx: &mut Context<Self>,
	) {
		self.profile = Some(profile);

		self.feedback.clear();
		self.poll_automatic_recap(true, cx);
		self.recap_focus(false);

		let elapsed = Instant::now() - DELAY;

		self.automatic_recap.away = Some(elapsed);
		self.automatic_recap.quiet = Some(elapsed);

		self.poll_automatic_recap(true, cx);
	}

	/// Return public recap evidence after a normal desktop poll.
	pub(crate) fn visual_automatic_recap_evidence(
		&mut self,
		cx: &mut Context<Self>,
	) -> serde_json::Value {
		self.poll_automatic_recap(true, cx);

		serde_json::json!({"automatic":self.recap.automatic,"state":self.recap.state,"completed_turns":self.automatic_recap.last_recapped})
	}
}
