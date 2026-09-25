//! Read native account-scoped task estimates on explicit user request.
use super::*;
use decodex_protocol::ChiefUsageEstimateResult;

impl ChiefSurface {
	pub(super) fn usage_estimate_panel(
		&self,
		work: &str,
		cx: &mut Context<Self>,
	) -> gpui::AnyElement {
		let owner = work.to_owned();
		let mut panel = div().flex().flex_col().gap_2().child(
			div().debug_selector(|| "task-usage-toggle".into()).child(self.workspace_action(
				"task-usage-toggle".into(),
				"Usage estimate".into(),
				move |s, cx| {
					if s.usage_estimate.as_ref().is_some_and(|(work, _)| work == &owner) {
						s.clear_usage_estimate();
						cx.notify();
					} else {
						s.load_usage_estimate(&owner, cx);
					}
				},
				cx,
			)),
		);
		if let Some((_, result)) = self.usage_estimate.as_ref().filter(|(owner, _)| owner == work) {
			let refresh = work.to_owned();
			panel=panel.child(self.workspace_action("task-usage-refresh".into(),"Refresh estimate".into(),move|s,cx|s.load_usage_estimate(&refresh,cx),cx))
				.child(muted("Backend estimate for the account shown below. It can lag recent activity and is not a final bill or remaining quota."))
				.child(div().id("task-usage-details").debug_selector(||"task-usage-details".into()).max_h(px(320.)).overflow_y_scroll()
					.child(result.as_ref().map(estimate_text).unwrap_or_else(||"Reading estimate…".into())));
		}
		panel.into_any_element()
	}

	pub(super) fn clear_usage_estimate(&mut self) {
		self.usage_estimate_epoch = self.usage_estimate_epoch.wrapping_add(1);
		self.usage_estimate = None;
		self.usage_estimate_task = None;
	}

	fn usage_estimate_binding(&self, work: &str) -> Option<(EntityId, String)> {
		let snapshot = self.snapshot.as_ref()?;
		Some((
			snapshot.runtime_source.clone()?,
			snapshot.work_items.iter().find(|w| w.id == work)?.codex_thread_id.clone()?,
		))
	}

	pub(super) fn invalidate_usage_estimate(&mut self, next: &ChiefSnapshotDto) {
		let Some((work, _)) = &self.usage_estimate else { return };
		let next_binding = next.runtime_source.clone().zip(
			next.work_items.iter().find(|w| &w.id == work).and_then(|w| w.codex_thread_id.clone()),
		);
		if self.usage_estimate_binding(work) != next_binding
			|| self.selected.as_deref() != Some(work)
		{
			self.clear_usage_estimate();
		}
	}

	fn finish_usage_estimate(
		&mut self,
		work: String,
		epoch: u64,
		binding: (EntityId, String),
		result: ChiefUsageEstimateResult,
	) {
		if self.usage_estimate_epoch != epoch
			|| self.selected.as_deref() != Some(&work)
			|| self.usage_estimate_binding(&work).as_ref() != Some(&binding)
		{
			return;
		}
		self.usage_estimate = Some((work, Some(result)));
		self.usage_estimate_task = None;
	}

	fn load_usage_estimate(&mut self, work: &str, cx: &mut Context<Self>) {
		if self.selected.as_deref() != Some(work) || self.usage_estimate_task.is_some() {
			return;
		}
		let (Some(profile), Ok(work_id), Some(binding)) = (
			self.profile.clone(),
			EntityId::new(work.to_owned()),
			self.usage_estimate_binding(work),
		) else {
			self.usage_estimate = Some((work.into(), Some(ChiefUsageEstimateResult::Unavailable)));
			cx.notify();
			return;
		};
		self.usage_estimate_epoch = self.usage_estimate_epoch.wrapping_add(1);
		let epoch = self.usage_estimate_epoch;
		self.usage_estimate = Some((work.into(), None));
		let work = work.to_owned();
		let generation = self.generation;
		let request = cx.background_executor().spawn(async move {
			let runtime =
				tokio::runtime::Builder::new_current_thread().enable_all().build().ok()?;
			runtime.block_on(ChiefClient::new(profile).usage_estimate(work_id)).ok()
		});
		self.usage_estimate_task = Some(cx.spawn(async move |surface, cx| {
			let result = request.await.unwrap_or(ChiefUsageEstimateResult::Unavailable);
			let _ = surface.update(cx, |s, cx| {
				if s.generation != generation || s.selected.as_deref() != Some(&work) {
					return;
				}
				s.finish_usage_estimate(work, epoch, binding, result);
				cx.notify();
			});
		}));
		cx.notify();
	}
}

fn micros(value: u64) -> String {
	let fraction = format!("{:06}", value % 1_000_000);
	let fraction = fraction.trim_end_matches('0');
	if fraction.is_empty() {
		format!("{}", value / 1_000_000)
	} else {
		format!("{}.{fraction}", value / 1_000_000)
	}
}
fn estimate_text(result: &ChiefUsageEstimateResult) -> String {
	let ChiefUsageEstimateResult::Available { account_id, observed_at_micros, estimate, .. } =
		result
	else {
		return match result {
			ChiefUsageEstimateResult::NotReported =>
				"The provider has not reported an estimate for this task and account.",
			ChiefUsageEstimateResult::Unsupported =>
				"This provider does not support task usage estimates.",
			ChiefUsageEstimateResult::CapacityExceeded =>
				"The complete estimate exceeds the display limit.",
			_ =>
				"The estimate could not be read, or its account or connection changed. Refresh to try again.",
		}
		.into();
	};
	let observed = time::OffsetDateTime::from_unix_timestamp(*observed_at_micros / 1_000_000)
		.ok()
		.map(|t| {
			format!(
				"{}-{:02}-{:02} {:02}:{:02}:{:02} UTC",
				t.year(),
				t.month() as u8,
				t.day(),
				t.hour(),
				t.minute(),
				t.second()
			)
		})
		.unwrap_or_else(|| "Unknown".into());
	let mut lines = vec![
		format!("Account: {}\nObserved: {observed}", account_id.as_str()),
		format!("Estimated credits: {}", micros(estimate.estimated_usage_credits_micros)),
		format!(
			"Estimated USD: {}",
			estimate
				.estimated_usage_usd_micros
				.map(micros)
				.unwrap_or_else(|| "Not reported".into())
		),
	];
	for group in &estimate.groups {
		let unknown =
			|v: Option<u64>| v.map(|v| v.to_string()).unwrap_or_else(|| "Not reported".into());
		lines.push(format!("{} · effort: {} · speed: {}\nEstimated credits: {}\nInput: {} · cached input: {} · new input: {}\nOutput: {} · total: {}",
		group.model.as_deref().unwrap_or("Model not reported"),group.reasoning_effort.as_deref().unwrap_or("Not reported"),group.speed.as_deref().unwrap_or("Not reported"),micros(group.estimated_usage_credits_micros),unknown(group.input_tokens),unknown(group.cached_input_tokens),unknown(group.net_new_input_tokens),unknown(group.output_tokens),unknown(group.total_tokens)));
	}
	lines.join("\n\n")
}

#[cfg(test)]
mod tests {
	use super::*;
	#[gpui::test]
	fn task_usage_estimate_is_explicit_and_clears_when_task_changes(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		surface.update(visual, |s, _| {
			let work = |id: &str| ChiefWorkItemDto {
				id: id.into(),
				parent_goal_id: None,
				kind: decodex_protocol::ChiefWorkKindDto::Goal,
				title: id.into(),
				codex_thread_id: Some(format!("thread-{id}")),
				active_turn_id: None,
				dispatch_state: ChiefDispatchStateDto::Idle,
				status: ChiefWorkStatusDto::Open,
				next_check_at_micros: None,
				created_at_micros: 1,
				updated_at_micros: 1,
			};
			s.apply_result(Ok(ChiefSnapshotResult::Available(ChiefSnapshotDto {
				runtime_source: None,
				workspaces: vec![],
				work_items: vec![work("root"), work("other")],
				dependencies: vec![],
				pending_events: vec![],
			})));
			assert!(s.usage_estimate.is_none());
			s.composer_menu = Some("agent-settings");
			s.composer_menu_content = Some("agent-settings");
		});
		visual.update(|window, cx| {
			window.resize(gpui::size(px(1180.), px(1400.)));
			window.draw(cx).clear();
		});
		std::thread::sleep(std::time::Duration::from_millis(220));
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		let bounds = visual.debug_bounds("task-usage-toggle").expect("usage toggle");
		visual.simulate_click(bounds.center(), gpui::Modifiers::default());
		surface.update(visual, |s, cx| {
			assert!(
				matches!(&s.usage_estimate,Some((id,Some(ChiefUsageEstimateResult::Unavailable))) if id=="root")
			);
			assert!(s.usage_estimate_task.is_none());
			s.open_page("other", cx);
			assert!(s.usage_estimate.is_none());
			assert!(s.usage_estimate_task.is_none());
		});
	}

	#[gpui::test]
	fn estimates_discard_changed_sources_and_late_reopened_requests(cx: &mut gpui::TestAppContext) {
		let surface = cx.new(ChiefSurface::new);
		let snapshot = |source: &str, thread: &str| ChiefSnapshotDto {
			runtime_source: Some(EntityId::new(source).unwrap()),
			workspaces: vec![],
			dependencies: vec![],
			pending_events: vec![],
			work_items: vec![ChiefWorkItemDto {
				id: "root".into(),
				parent_goal_id: None,
				kind: decodex_protocol::ChiefWorkKindDto::Goal,
				title: "Task".into(),
				codex_thread_id: Some(thread.into()),
				active_turn_id: None,
				dispatch_state: ChiefDispatchStateDto::Idle,
				status: ChiefWorkStatusDto::Open,
				next_check_at_micros: None,
				created_at_micros: 1,
				updated_at_micros: 1,
			}],
		};
		let estimate = || ChiefUsageEstimateResult::Available {
			work_id: EntityId::new("root").unwrap(),
			account_id: EntityId::new("account-a").unwrap(),
			observed_at_micros: 1,
			estimate: decodex_protocol::ThreadUsageEstimate {
				thread_id: "thread-a".into(),
				estimated_usage_credits_micros: 5_000_000,
				estimated_usage_usd_micros: Some(100_000),
				groups: vec![],
			},
		};
		surface.update(cx, |s, _| {
			for next in [
				Some(snapshot("source-b", "thread-a")),
				Some(snapshot("source-a", "thread-b")),
				None,
			] {
				s.apply_result(Ok(ChiefSnapshotResult::Available(snapshot(
					"source-a", "thread-a",
				))));
				let binding = s.usage_estimate_binding("root").unwrap();
				let epoch = s.usage_estimate_epoch;
				s.finish_usage_estimate("root".into(), epoch, binding.clone(), estimate());
				assert!(s.usage_estimate.is_some());
				s.apply_result(next.map(ChiefSnapshotResult::Available).ok_or(()));
				assert!(s.usage_estimate.is_none());
				s.finish_usage_estimate("root".into(), epoch, binding, estimate());
				assert!(
					s.usage_estimate.is_none(),
					"Late old-source result cannot repopulate the panel"
				);
			}
			s.apply_result(Ok(ChiefSnapshotResult::Available(snapshot("source-a", "thread-a"))));
			let binding = s.usage_estimate_binding("root").unwrap();
			let previous = s.usage_estimate_epoch;
			s.clear_usage_estimate();
			s.usage_estimate = Some(("root".into(), None));
			s.finish_usage_estimate("root".into(), previous, binding.clone(), estimate());
			assert!(matches!(s.usage_estimate, Some((_, None))));
			s.finish_usage_estimate("root".into(), s.usage_estimate_epoch, binding, estimate());
			assert!(matches!(
				s.usage_estimate,
				Some((_, Some(ChiefUsageEstimateResult::Available { .. })))
			));
		});
	}

	#[test]
	fn task_estimates_keep_micros_exact_and_absence_distinct_from_zero() {
		assert_eq!(micros(9007199254740993), "9007199254.740993");
		assert_eq!(micros(0), "0");
		assert_eq!(micros(1), "0.000001");
		assert!(estimate_text(&ChiefUsageEstimateResult::NotReported).contains("not reported"));
		assert!(
			estimate_text(&ChiefUsageEstimateResult::Unavailable).contains("could not be read")
		);
	}
}
