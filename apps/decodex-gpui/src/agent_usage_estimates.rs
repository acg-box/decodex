//! Source-bound conversation dollars, read in the background without model inference.
use gpui::AnyElement;
use tokio::runtime::Builder;

use crate::shell::agent_surface::{
	AgentClient, AgentSnapshotDto, AgentSurface, Context, EntityId, InteractiveElement,
	IntoElement, ParentElement, StatefulInteractiveElement, Styled,
};
#[cfg(test)]
use crate::shell::agent_surface::{
	AgentDispatchStateDto, AgentSnapshotResult, AgentWorkItemDto, AgentWorkStatusDto,
};
use decodex_protocol::AgentUsageEstimateResult;

impl AgentSurface {
	pub(super) fn conversation_cost(&self) -> Option<AnyElement> {
		let work = self.conversation_work()?;
		let (_, Some(AgentUsageEstimateResult::Available { estimate, .. })) =
			self.usage_estimate.as_ref().filter(|(id, _)| id == &work.id)?
		else {
			return None;
		};
		if work.codex_thread_id.as_deref() != Some(&estimate.thread_id) {
			return None;
		}
		let label = amount_label(estimate)?;
		Some(
			gpui::div()
				.id("composer-conversation-cost")
				.debug_selector(|| "composer-conversation-cost".into())
				.aria_label(format!("Conversation cost: {label} USD"))
				.flex_shrink_0()
				.whitespace_nowrap()
				.text_size(gpui::px(11.))
				.line_height(gpui::px(16.))
				.text_color(crate::ui_theme::rgb(crate::ui_theme::TEXT))
				.child(label)
				.into_any_element(),
		)
	}

	pub(super) fn refresh_conversation_cost(&mut self, cx: &mut Context<Self>) {
		if !self.read_connection_ready() {
			return;
		}
		let Some(work) = self.conversation_work() else { return };
		let Some((_, thread)) = self.usage_estimate_binding(&work.id) else { return };
		// A side chat must not inherit its parent's amount. Read once during a turn,
		// then refresh its total after completion, without polling while it streams.
		if work.codex_thread_id.as_ref() != Some(&thread) {
			return;
		}
		let observed = self.usage_estimate.as_ref().is_some_and(|(id, _)| id == &work.id);
		if observed
			&& (work.active_turn_id.is_some()
				|| self.usage_estimate_updated_at == Some(work.updated_at_micros))
		{
			return;
		}
		self.load_usage_estimate(&work.id, cx);
	}

	pub(super) fn clear_usage_estimate(&mut self) {
		self.usage_estimate_epoch = self.usage_estimate_epoch.wrapping_add(1);
		self.usage_estimate = None;
		self.usage_estimate_updated_at = None;
		self.usage_estimate_task = None;
	}

	fn usage_estimate_binding(&self, work: &str) -> Option<(EntityId, String)> {
		let snapshot = self.snapshot.as_ref()?;

		Some((
			snapshot.runtime_source.clone()?,
			snapshot.work_items.iter().find(|w| w.id == work)?.codex_thread_id.clone()?,
		))
	}

	pub(super) fn invalidate_usage_estimate(&mut self, next: &AgentSnapshotDto) {
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
		result: AgentUsageEstimateResult,
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
			self.usage_estimate = Some((work.into(), Some(AgentUsageEstimateResult::Unavailable)));

			cx.notify();

			return;
		};

		self.usage_estimate_epoch = self.usage_estimate_epoch.wrapping_add(1);

		let epoch = self.usage_estimate_epoch;

		if self.usage_estimate.as_ref().is_none_or(|(id, _)| id != work) {
			self.usage_estimate = Some((work.into(), None));
		}
		self.usage_estimate_updated_at =
			self.conversation_record(work).map(|work| work.updated_at_micros);

		let work = work.to_owned();
		let request = cx.background_executor().spawn(async move {
			let runtime = Builder::new_current_thread().enable_all().build().ok()?;

			runtime.block_on(AgentClient::new(profile).usage_estimate(work_id)).ok()
		});

		self.usage_estimate_task = Some(cx.spawn(async move |surface, cx| {
			let result = request.await.unwrap_or(AgentUsageEstimateResult::Unavailable);
			let _ = surface.update(cx, |s, cx| {
				s.finish_usage_estimate(work, epoch, binding, result);
				cx.notify();
			});
		}));

		cx.notify();
	}
}

fn amount_label(estimate: &decodex_protocol::ThreadUsageEstimate) -> Option<String> {
	let amount = micros(estimate.reported_usd_micros()?);
	Some(if estimate.native_usage_usd_micros.is_some() {
		format!("${amount}")
	} else {
		format!("≈ ${amount}")
	})
}

#[cfg(any(test, feature = "visual-capture"))]
impl AgentSurface {
	pub(crate) fn visual_conversation_cost(&mut self, page: &str) {
		let id = self.selected.clone().expect("selected fixture conversation");
		self.snapshot
			.as_mut()
			.unwrap()
			.work_items
			.iter_mut()
			.find(|work| work.id == id)
			.unwrap()
			.codex_thread_id = Some("cost-fixture-thread".into());
		self.usage_estimate = Some((
			id.clone(),
			Some(AgentUsageEstimateResult::Available {
				work_id: EntityId::new(id).unwrap(),
				account_id: EntityId::new("cost-fixture-account").unwrap(),
				observed_at_micros: 1,
				estimate: decodex_protocol::ThreadUsageEstimate {
					thread_id: "cost-fixture-thread".into(),
					estimated_usage_credits_micros: 46_000_000,
					estimated_usage_usd_micros: (page == "cost-estimated").then_some(1_234_567),
					native_usage_usd_micros: (page == "cost").then_some(1_234_567),
					groups: vec![],
				},
			}),
		));
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

#[cfg(test)]
mod tests {

	use futures_util::{SinkExt as _, StreamExt as _};
	#[cfg(test)] use gpui::AppContext as _;
	use tokio_tungstenite::tungstenite::Message;

	#[cfg(test)]
	use crate::shell::agent_surface::usage_estimates::{
		AgentDispatchStateDto, AgentSnapshotResult, AgentWorkItemDto, AgentWorkStatusDto,
	};
	use crate::shell::agent_surface::{
		usage_estimates::{
			self, AgentSnapshotDto, AgentSurface, AgentUsageEstimateResult, EntityId,
		},
		wire_test_support,
	};
	use decodex_protocol::{
		CURRENT_VERSION, ClientMessage, QueryPayload, QueryResultEnvelope, QueryResultPayload,
		ServerId, ServerMessage,
	};

	#[gpui::test]
	fn conversation_dollars_follow_context_keep_zero_and_hide_unknowns(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.visual_workspace_page("markdown", cx);
			s.snapshot
				.as_mut()
				.unwrap()
				.work_items
				.iter_mut()
				.find(|w| w.id == "agent")
				.unwrap()
				.codex_thread_id = Some("thread".into());
		});
		for width in [640., 1_400.] {
			visual.simulate_resize(gpui::size(gpui::px(width), gpui::px(1_000.)));
			for (native, estimated, expected) in [
				(None, None, None),
				(None, Some(0), Some("≈ $0")),
				(Some(0), Some(9_000_000), Some("$0")),
				(Some(1_234_567), None, Some("$1.234567")),
			] {
				surface.update(visual, |s, cx| {
					s.usage_estimate = Some((
						"agent".into(),
						Some(AgentUsageEstimateResult::Available {
							work_id: EntityId::new("agent").unwrap(),
							account_id: EntityId::new("account").unwrap(),
							observed_at_micros: 1,
							estimate: decodex_protocol::ThreadUsageEstimate {
								thread_id: "thread".into(),
								estimated_usage_credits_micros: 46_000_000,
								estimated_usage_usd_micros: estimated,
								native_usage_usd_micros: native,
								groups: vec![],
							},
						}),
					));
					let estimate = match &s.usage_estimate.as_ref().unwrap().1 {
						Some(AgentUsageEstimateResult::Available { estimate, .. }) => estimate,
						_ => unreachable!(),
					};
					assert_eq!(super::amount_label(estimate).as_deref(), expected);
					cx.notify();
				});
				visual.update(crate::ui_frame::draw_current_frame);
				let cost = visual.debug_bounds("composer-conversation-cost");
				assert_eq!(cost.is_some(), expected.is_some());
				if let Some(cost) = cost {
					let context = visual.debug_bounds("composer-context-detail").unwrap();
					let send = visual.debug_bounds("composer-send").unwrap();
					assert!(cost.left() >= context.right(), "cost follows Context at {width}");
					assert!(
						cost.right() <= send.left(),
						"cost stays inside the toolbar at {width}"
					);
					assert!((cost.center().y - context.center().y).abs() <= gpui::px(1.));
				}
				assert!(visual.debug_bounds("conversation-cost-estimate").is_none());
			}
		}
		surface.update(visual, |s, _| {
			s.native_agents.selected = Some(("agent".into(), "other-thread".into()));
			assert!(s.conversation_cost().is_none(), "a side chat cannot show its parent's cost");
		});
	}

	#[gpui::test]
	fn estimates_discard_changed_sources_and_late_reopened_requests(cx: &mut gpui::TestAppContext) {
		let surface = cx.new(AgentSurface::new);
		let snapshot = |source: &str, thread: &str| AgentSnapshotDto {
			agents: Default::default(),
			guardian: Default::default(),
			portfolio: Default::default(),
			archived_work_ids: Default::default(),
			agent_slots: Default::default(),
			context_references: vec![],
			connection_initializing: false,
			runtime_source: Some(EntityId::new(source).unwrap()),
			projects: vec![],
			dependencies: vec![],
			pending_events: vec![],
			work_items: vec![AgentWorkItemDto {
				id: "root".into(),
				parent_goal_id: None,
				kind: decodex_protocol::AgentWorkKindDto::Goal,
				title: "Task".into(),
				codex_thread_id: Some(thread.into()),
				active_turn_id: None,
				dispatch_state: AgentDispatchStateDto::Idle,
				status: AgentWorkStatusDto::Open,
				next_check_at_micros: None,
				created_at_micros: 1,
				updated_at_micros: 1,
			}],
		};
		let estimate = || AgentUsageEstimateResult::Available {
			work_id: EntityId::new("root").unwrap(),
			account_id: EntityId::new("account-a").unwrap(),
			observed_at_micros: 1,
			estimate: decodex_protocol::ThreadUsageEstimate {
				thread_id: "thread-a".into(),
				estimated_usage_credits_micros: 5_000_000,
				estimated_usage_usd_micros: Some(100_000),
				native_usage_usd_micros: None,
				groups: vec![],
			},
		};

		surface.update(cx, |s, _| {
			for next in [
				Some(snapshot("source-b", "thread-a")),
				Some(snapshot("source-a", "thread-b")),
				None,
			] {
				s.apply_result(Ok(AgentSnapshotResult::Available(snapshot(
					"source-a", "thread-a",
				))));

				let binding = s.usage_estimate_binding("root").unwrap();
				let epoch = s.usage_estimate_epoch;

				s.finish_usage_estimate("root".into(), epoch, binding.clone(), estimate());

				assert!(s.usage_estimate.is_some());

				s.apply_result(next.map(AgentSnapshotResult::Available).ok_or(()));

				assert!(s.usage_estimate.is_none());

				s.finish_usage_estimate("root".into(), epoch, binding, estimate());

				assert!(
					s.usage_estimate.is_none(),
					"Late old-source result cannot repopulate the panel"
				);
			}

			s.apply_result(Ok(AgentSnapshotResult::Available(snapshot("source-a", "thread-a"))));

			let binding = s.usage_estimate_binding("root").unwrap();
			let previous = s.usage_estimate_epoch;

			s.clear_usage_estimate();

			s.usage_estimate = Some(("root".into(), None));

			s.finish_usage_estimate("root".into(), previous, binding.clone(), estimate());

			assert!(matches!(s.usage_estimate, Some((_, None))));

			s.finish_usage_estimate("root".into(), s.usage_estimate_epoch, binding, estimate());

			assert!(matches!(
				s.usage_estimate,
				Some((_, Some(AgentUsageEstimateResult::Available { .. })))
			));
		});
	}

	#[test]
	fn task_estimates_keep_micros_exact_and_absence_distinct_from_zero() {
		assert_eq!(usage_estimates::micros(9_007_199_254_740_993), "9007199254.740993");
		assert_eq!(usage_estimates::micros(0), "0");
		assert_eq!(usage_estimates::micros(1), "0.000001");
	}

	#[gpui::test]
	fn unrelated_refresh_does_not_strand_an_estimate_read(cx: &mut gpui::TestAppContext) {
		let (_directory, profile, server) = wire_test_support::fixture(|listener| async move {
			for _ in 0..2 {
				let mut socket = wire_test_support::accept(&listener).await;
				let Message::Text(text) = socket.next().await.unwrap().unwrap() else {
					panic!("query")
				};
				let ClientMessage::Query(query) = serde_json::from_str(&text).unwrap() else {
					panic!("read only")
				};

				assert!(
					matches!(query.payload, QueryPayload::GetAgentUsageEstimate { work_id } if work_id.as_str() == "agent")
				);

				let reply = ServerMessage::QueryResult(QueryResultEnvelope {
					version: CURRENT_VERSION,
					server_id: ServerId::new(super::super::wire_test_support::SERVER).unwrap(),
					query_id: query.query_id,
					payload: QueryResultPayload::AgentUsageEstimate(
						AgentUsageEstimateResult::NotReported,
					),
				});

				socket
					.send(Message::Text(serde_json::to_string(&reply).unwrap().into()))
					.await
					.unwrap();
			}
		});
		let surface = cx.new(AgentSurface::new);

		surface.update(cx, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.profile = Some(profile);

			let snapshot = s.snapshot.as_mut().unwrap();

			snapshot.runtime_source = Some(EntityId::new("source").unwrap());
			snapshot.work_items.iter_mut().find(|w| w.id == "agent").unwrap().codex_thread_id =
				Some("thread".into());

			s.refresh_conversation_cost(cx);

			assert!(s.usage_estimate_task.is_some());

			s.generation += 1;
		});

		cx.run_until_parked();
		surface.read_with(cx, |s, _| {
			assert!(
				s.usage_estimate_task.is_none(),
				"completed read must release the refresh guard"
			);
			assert!(matches!(
				s.usage_estimate,
				Some((_, Some(AgentUsageEstimateResult::NotReported)))
			));
		});
		surface.update(cx, |s, cx| {
			let epoch = s.usage_estimate_epoch;
			for _ in 0..20 {
				s.refresh_conversation_cost(cx);
			}
			assert_eq!(
				s.usage_estimate_epoch, epoch,
				"an unchanged idle conversation must not poll billing"
			);
			assert!(s.usage_estimate_task.is_none());
			let work = s
				.snapshot
				.as_mut()
				.unwrap()
				.work_items
				.iter_mut()
				.find(|work| work.id == "agent")
				.unwrap();
			work.updated_at_micros += 1;
			work.active_turn_id = Some("running-turn".into());
			s.refresh_conversation_cost(cx);
			assert!(s.usage_estimate_task.is_none(), "streaming must not poll billing");
			s.snapshot
				.as_mut()
				.unwrap()
				.work_items
				.iter_mut()
				.find(|work| work.id == "agent")
				.unwrap()
				.active_turn_id = None;
			s.refresh_conversation_cost(cx);
			assert!(s.usage_estimate_task.is_some(), "completion refreshes the conversation total");
		});
		cx.run_until_parked();
		server.join().unwrap();
		surface.update(cx, |s, cx| {
			assert!(s.usage_estimate_task.is_none());
			s.mark_stale(cx);

			assert!(s.usage_estimate.is_none());
			assert!(s.usage_estimate_task.is_none());
		});
	}
}
