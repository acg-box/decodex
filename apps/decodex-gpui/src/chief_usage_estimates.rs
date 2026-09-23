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
						s.usage_estimate = None;
						s.usage_estimate_task = None;
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

	fn load_usage_estimate(&mut self, work: &str, cx: &mut Context<Self>) {
		if self.selected.as_deref() != Some(work) || self.usage_estimate_task.is_some() {
			return;
		}
		let (Some(profile), Ok(work_id)) = (self.profile.clone(), EntityId::new(work.to_owned()))
		else {
			self.usage_estimate = Some((work.into(), Some(ChiefUsageEstimateResult::Unavailable)));
			cx.notify();
			return;
		};
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
				s.usage_estimate = Some((work, Some(result)));
				s.usage_estimate_task = None;
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
