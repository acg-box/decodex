//! Actual usage in existing Agent preferences and Work details. No billing or control policy.
use gpui::{AnyElement, ClipboardItem, InteractiveElement as _, StatefulInteractiveElement as _};
use tokio::runtime::Builder;

use crate::shell::agent_surface::{
	AgentClient, AgentSnapshotDto, AgentSurface, Context, EntityId, IntoElement, ParentElement,
	Styled, Task, mcp_forms,
};
use decodex_protocol::{UsageEvidenceResult, UsageEvidenceScope, UsageEvidenceTotals, work::Work};

#[derive(Default)]
pub(super) struct Panel {
	scope: Option<UsageEvidenceScope>,
	result: Option<UsageEvidenceResult>,
	feedback: String,
	task: Option<Task<()>>,
	epoch: u64,
}

impl AgentSurface {
	pub(super) fn reset_usage_evidence(&mut self) {
		self.usage_evidence =
			Panel { epoch: self.usage_evidence.epoch.wrapping_add(1), ..Default::default() };
	}

	pub(super) fn invalidate_usage_evidence(&mut self, next: &AgentSnapshotDto) {
		if self.snapshot.as_ref().is_none_or(|s| s.runtime_source != next.runtime_source) {
			self.reset_usage_evidence();
		}
	}

	fn load_usage_evidence(&mut self, scope: UsageEvidenceScope, cx: &mut Context<Self>) {
		let Some(profile) = self.profile.clone() else { return };
		let Some(source) = self.snapshot.as_ref().and_then(|s| s.runtime_source.clone()) else {
			return;
		};

		if self.usage_evidence.task.is_some() && self.usage_evidence.scope.as_ref() == Some(&scope)
		{
			return;
		}

		self.reset_usage_evidence();

		self.usage_evidence.scope = Some(scope.clone());
		self.usage_evidence.feedback = "Reading retained native receipts…".into();

		let epoch = self.usage_evidence.epoch;
		let generation = self.generation;
		let expected = scope.clone();
		let future = cx.background_executor().spawn(async move {
			let runtime = Builder::new_current_thread().enable_all().build().map_err(|_| ())?;

			runtime.block_on(AgentClient::new(profile).usage_evidence(scope)).map_err(|_| ())
		});

		self.usage_evidence.task = Some(cx.spawn(async move |surface, cx| {
			let result = future.await;
			let _ = surface.update(cx, |s, cx| {
				if s.generation != generation
					|| s.usage_evidence.epoch != epoch
					|| s.usage_evidence.scope.as_ref() != Some(&expected)
				{
					return;
				}

				s.usage_evidence.task = None;

				match result {
					Ok(value)
						if matches!(&value,UsageEvidenceResult::Available{source:actual,..} if actual==&source)
							&& s.snapshot.as_ref().and_then(|v| v.runtime_source.as_ref())
								== Some(&source) =>
					{
						s.usage_evidence.result = Some(value);

						s.usage_evidence.feedback.clear();
					},
					_ =>
						s.usage_evidence.feedback =
							"Usage evidence is unavailable. Refresh to try again.".into(),
				}

				cx.notify();
			});
		}));

		cx.notify();
	}

	pub(super) fn work_usage_panel(&self, work: &Work, cx: &mut Context<Self>) -> AnyElement {
		self.usage_evidence_panel(
			UsageEvidenceScope::Work {
				work_id: EntityId::new(work.id.clone()).expect("saved Work identity"),
			},
			cx,
		)
	}

	pub(super) fn agent_usage_panel(&self, agent: &str, cx: &mut Context<Self>) -> AnyElement {
		if self.native_agents.selected.is_some()
			|| !self.snapshot.as_ref().is_some_and(|s| s.agents.iter().any(|a| a.id == agent))
		{
			return gpui::div().into_any_element();
		}

		self.usage_evidence_panel(
			UsageEvidenceScope::Agent {
				agent_id: EntityId::new(agent).expect("saved Agent identity"),
			},
			cx,
		)
	}

	pub(super) fn usage_evidence_panel(
		&self,
		scope: UsageEvidenceScope,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let opened = self.usage_evidence.scope.as_ref() == Some(&scope);
		let selected = scope.clone();
		let mut panel = gpui::div().flex().flex_col().gap_2().child(mcp_forms::mcp_button(
			format!("usage-{}", scope.id().as_str()),
			"Actual usage".into(),
			opened,
			cx,
			move |s, cx| {
				if s.usage_evidence.scope.as_ref() == Some(&selected) {
					s.reset_usage_evidence();
					cx.notify();
				} else {
					s.load_usage_evidence(selected.clone(), cx);
				}
			},
		));

		if !opened {
			return panel.into_any_element();
		}
		if !self.usage_evidence.feedback.is_empty() {
			panel = panel.child(self.usage_evidence.feedback.clone());
		}

		if let Some(UsageEvidenceResult::Available { totals, sources, sources_truncated, .. }) =
			&self.usage_evidence.result
		{
			panel = panel.child(totals_text(&scope, totals));

			panel=panel.child("Known retained receipts only. Work includes turns linked by saved evidence; those turns can contain other activity. Missing history and unobserved native child usage remain unknown. Durations can overlap; these values are not Work elapsed time or a bill.");

			let mut preview = gpui::div()
				.id("usage-source-preview")
				.max_h(gpui::px(240.))
				.overflow_y_scroll()
				.flex()
				.flex_col()
				.gap_1();

			for row in sources {
				preview = preview.child(format!(
					"{} · thread {} · turn {}\nWork: {}{} · receipt {} · {} input / {} output · {} ms",
					row.agent_id,
					row.thread_id,
					row.turn_id,
					if row.work_ids.is_empty() {
						"Unattributed".into()
					} else {
						row.work_ids.join(", ")
					},
					if row.work_count > row.work_ids.len() as u64 { " …" } else { "" },
					row.event_id.map(|v| v.to_string()).unwrap_or_else(|| "Not retained".into()),
					row.input_tokens.map(|v| v.to_string()).unwrap_or_else(|| "Unknown".into()),
					row.output_tokens.map(|v| v.to_string()).unwrap_or_else(|| "Unknown".into()),
					row.duration_ms.map(|v| v.to_string()).unwrap_or_else(|| "Unknown".into())
				));
			}

			panel = panel.child(preview);

			if *sources_truncated {
				panel = panel.child(
					"Source preview is bounded. Totals cover all retained matching receipts.",
				);
			}

			if let Ok(text) = serde_json::to_string_pretty(
				self.usage_evidence.result.as_ref().expect("visible evidence"),
			) {
				panel = panel.child(mcp_forms::mcp_button(
					"usage-copy".into(),
					"Copy usage evidence".into(),
					false,
					cx,
					move |_, cx| cx.write_to_clipboard(ClipboardItem::new_string(text.clone())),
				));
			}
		}

		if self.usage_evidence.task.is_none() {
			panel = panel.child(mcp_forms::mcp_button(
				"usage-refresh".into(),
				"Refresh usage".into(),
				false,
				cx,
				move |s, cx| s.load_usage_evidence(scope.clone(), cx),
			));
		}

		panel.into_any_element()
	}
}

fn totals_text(scope: &UsageEvidenceScope, t: &UsageEvidenceTotals) -> String {
	let tokens = if t.token_turns == 0 {
		"Tokens unknown".into()
	} else {
		format!("{} input · {} output tokens", t.input_tokens, t.output_tokens)
	};
	let duration = if t.duration_turns == 0 {
		"Duration unknown".into()
	} else {
		format!("{} ms recorded turn duration", t.duration_ms)
	};
	let coverage = format!(
		"{} retained turns · tokens on {} · duration on {}",
		t.turns, t.token_turns, t.duration_turns
	);
	let shared = if matches!(scope, UsageEvidenceScope::Work { .. }) {
		format!("{} known shared turns excluded from linked-turn totals", t.shared_turns)
	} else {
		format!(
			"{} shared turns counted once · {} turns without Work attribution",
			t.shared_turns, t.unattributed_turns
		)
	};

	format!("{tokens}\n{duration}\n{coverage}\n{shared}")
}

#[cfg(test)]
mod tests {

	use crate::shell::agent_surface::usage_evidence::{
		self, EntityId, UsageEvidenceScope, UsageEvidenceTotals,
	};

	#[test]
	fn usage_labels_keep_unknown_coverage_shared_work_and_parallel_duration_distinct() {
		let scope = UsageEvidenceScope::Work { work_id: EntityId::new("work").unwrap() };
		let mut t = UsageEvidenceTotals {
			turns: 2,
			token_turns: 0,
			duration_turns: 0,
			shared_turns: 1,
			unattributed_turns: 0,
			input_tokens: 0,
			output_tokens: 0,
			duration_ms: 0,
		};

		assert!(usage_evidence::totals_text(&scope, &t).contains("Tokens unknown"));
		assert!(usage_evidence::totals_text(&scope, &t).contains("1 known shared turns excluded"));

		t.token_turns = 1;
		t.duration_turns = 1;

		let text = usage_evidence::totals_text(&scope, &t);

		assert!(text.contains("0 input"), "a known zero differs from missing counters");
		assert!(text.contains("recorded turn duration"));
	}
}
