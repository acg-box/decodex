//! Exact saved action reviews and explicit approval submission, independent of execution.
use super::*;
use decodex_protocol::{
	ChiefGuardianReviewsResult as Reviews, ChiefGuardianStatus as Status,
	ChiefGuardianSubmission as Submission,
};

#[derive(Default)]
pub(super) struct Panel {
	owner: Option<String>,
	result: Option<Reviews>,
	before: Option<i64>,
	epoch: u64,
	expanded: bool,
	reviewed: Option<(i64, String)>,
	pending: std::collections::BTreeMap<i64, String>,
	stale: bool,
	feedback: String,
	request: Option<Task<()>>,
	mutation: Option<Task<()>>,
	mutation_key: Option<String>,
}

impl Panel {
	fn finish_submission(
		&mut self,
		row: i64,
		key: &str,
		result: Result<ChiefCommandResponse, ()>,
	) -> bool {
		if self.mutation_key.as_deref() != Some(key) {
			return false;
		}
		self.mutation = None;
		self.mutation_key = None;
		self.feedback = match result {
			Ok(ChiefCommandResponse::Accepted { .. }) =>
				"Approval submitted to Codex. This does not rerun the action.",
			Ok(ChiefCommandResponse::Rejected { .. }) => {
				self.pending.remove(&row);
				"Approval was not accepted. Refresh and review the current details."
			},
			Err(()) => {
				self.pending.remove(&row);
				"No approval was sent. Check the service connection before trying again."
			},
			Ok(ChiefCommandResponse::PotentiallyDispatched { .. }) =>
				"Approval submission is unconfirmed. It will not be sent again automatically.",
		}
		.into();
		true
	}

	fn observe(&mut self, result: Reviews) {
		if let Reviews::Available { reviews, .. } = &result {
			for review in reviews {
				if review.submission.is_some()
					&& let Some(key) = &review.submission_key
					&& self.pending.get(&review.row_id) == Some(key)
				{
					self.pending.remove(&review.row_id);
				}
			}
			self.stale = false;
			self.result = Some(result);
		} else {
			self.stale = true;
			if self.result.is_none() {
				self.result = Some(result);
			}
		}
	}
}

impl ChiefSurface {
	#[cfg(feature = "visual-capture")]
	#[allow(dead_code, reason = "the main binary shares this module with the capture binary")]
	pub(crate) fn visual_guardian_reviews(&mut self, result: Option<Reviews>) {
		self.guardian.owner = self.selected.clone();
		self.guardian.expanded = true;
		self.guardian.reviewed = match &result {
			Some(Reviews::Available { reviews, .. }) =>
				reviews.first().map(|r| (r.row_id, r.digest.clone())),
			_ => None,
		};
		self.guardian.result = result;
	}

	pub(super) fn load_guardian_reviews(&mut self, cx: &mut Context<Self>) {
		let Some(work) = self.selected.clone() else {
			return;
		};
		if self.guardian.owner.as_ref() != Some(&work) {
			let epoch = self.guardian.epoch.wrapping_add(1);
			self.guardian = Panel { owner: Some(work.clone()), epoch, ..Default::default() };
		}
		if self.guardian.request.is_some() {
			return;
		}
		let Some(profile) = self.profile.clone() else {
			return;
		};
		let Ok(work_id) = EntityId::new(work.clone()) else {
			return;
		};
		let generation = self.generation;
		let epoch = self.guardian.epoch;
		let before = self.guardian.before;
		let request = cx.background_executor().spawn(async move {
			let runtime =
				tokio::runtime::Builder::new_current_thread().enable_all().build().ok()?;
			runtime.block_on(ChiefClient::new(profile).guardian_reviews(work_id, before)).ok()
		});
		self.guardian.request = Some(cx.spawn(async move |surface, cx| {
			let result = request.await.unwrap_or(Reviews::Unavailable);
			let _ = surface.update(cx, |s, cx| {
				if s.generation != generation
					|| s.selected.as_ref() != Some(&work)
					|| s.guardian.epoch != epoch
				{
					return;
				}
				s.guardian.request = None;
				s.guardian.observe(result);
				cx.notify();
			});
		}));
	}

	pub(super) fn guardian_needs_refresh(&self) -> bool {
		self.guardian.owner == self.selected
			&& (self.guardian.expanded
				|| self.guardian.stale
				|| !self.guardian.pending.is_empty()
				|| matches!(&self.guardian.result,Some(Reviews::Available {reviews,..}) if reviews.iter().any(|r|r.status==Status::InProgress || r.submission==Some(Submission::Pending))))
	}

	pub(super) fn guardian_disconnected(&mut self) {
		self.guardian.epoch = self.guardian.epoch.wrapping_add(1);
		self.guardian.request = None;
		self.guardian.mutation = None;
		self.guardian.mutation_key = None;
		self.guardian.stale = true;
	}

	fn guardian_page(&mut self, before: Option<i64>, cx: &mut Context<Self>) {
		self.guardian.request = None;
		self.guardian.epoch = self.guardian.epoch.wrapping_add(1);
		self.guardian.before = before;
		self.guardian.result = None;
		self.guardian.reviewed = None;
		self.load_guardian_reviews(cx);
		cx.notify();
	}

	fn approve_guardian(&mut self, work: &str, row: i64, digest: &str, cx: &mut Context<Self>) {
		if self.selected.as_deref() != Some(work)
			|| self.guardian.owner.as_deref() != Some(work)
			|| self.guardian.mutation.is_some()
			|| self.guardian.stale
			|| self.guardian.pending.contains_key(&row)
			|| self.guardian.reviewed.as_ref() != Some(&(row, digest.into()))
		{
			return;
		}
		let Some(Reviews::Available { reviews, .. }) = &self.guardian.result else {
			return;
		};
		if !reviews.iter().any(|r| r.row_id == row && r.digest == digest && r.can_approve) {
			return;
		}
		let Some(profile) = self.profile.clone() else {
			self.guardian.feedback = "No service profile is configured.".into();
			cx.notify();
			return;
		};
		let (Ok(work_id), Ok(review_digest)) = (EntityId::new(work), WireText::new(digest)) else {
			return;
		};
		let generation = self.generation;
		let owner = work.to_owned();
		self.guardian.feedback = "Submitting approval to Codex…".into();
		let key = IdempotencyKey::new(unique_command()).expect("bounded identity");
		self.guardian.pending.insert(row, key.as_str().into());
		let submitted_key = key.as_str().to_owned();
		self.guardian.mutation_key = Some(submitted_key.clone());
		let request = cx.background_executor().spawn(async move {
			let runtime = tokio::runtime::Builder::new_current_thread()
				.enable_all()
				.build()
				.map_err(|_| ())?;
			runtime
				.block_on(ChiefClient::new(profile).execute(
					ChiefActionDto::ApproveGuardianDenial {
						work_id,
						review_row: row,
						review_digest,
					},
					key,
				))
				.map_err(|_| ())
		});
		self.guardian.mutation = Some(cx.spawn(async move |surface, cx| {
			let result = request.await;
			let _ = surface.update(cx, |s, cx| {
				if s.generation != generation
					|| s.guardian.owner.as_ref() != Some(&owner)
					|| s.guardian.mutation_key.as_ref() != Some(&submitted_key)
				{
					return;
				}
				if !s.guardian.finish_submission(row, &submitted_key, result) {
					return;
				}
				s.guardian.request = None;
				s.guardian.epoch = s.guardian.epoch.wrapping_add(1);
				s.load_guardian_reviews(cx);
				cx.notify();
			});
		}));
		cx.notify();
	}

	pub(super) fn guardian_panel(
		&self,
		work: &ChiefWorkItemDto,
		cx: &mut Context<Self>,
	) -> gpui::AnyElement {
		if self.guardian.owner.as_deref() != Some(&work.id) {
			return div().into_any_element();
		}
		let Some(result) = &self.guardian.result else {
			return div().into_any_element();
		};
		let Reviews::Available { reviews, next_before } = result else {
			return div()
				.child("Saved action reviews are unavailable.")
				.child(button("guardian-refresh".into(), "Refresh reviews".into(), cx, |s, cx| {
					s.guardian_page(None, cx)
				}))
				.into_any_element();
		};
		if reviews.is_empty() && self.guardian.before.is_none() {
			return div().into_any_element();
		}
		let denied = reviews
			.iter()
			.filter(|r| r.status == Status::Denied && r.submission != Some(Submission::Submitted))
			.count();
		let title = if denied > 0 {
			format!("Action reviews · {denied} denied on this page")
		} else {
			"Action reviews".into()
		};
		let mut panel = div().flex().flex_col().gap_2().min_w_0().child(button(
			"guardian-toggle".into(),
			title,
			cx,
			|s, cx| {
				s.guardian.expanded = !s.guardian.expanded;
				cx.notify();
			},
		));
		if !self.guardian.expanded {
			return panel.into_any_element();
		}
		if self.guardian.stale {
			panel = panel.child(
				"Showing saved review details. Reconnecting before another approval can be submitted.",
			);
		}
		if !self.guardian.feedback.is_empty() {
			panel = panel.child(self.guardian.feedback.clone());
		}
		let mut body = div().flex().flex_col().gap_3().min_w_0();
		for review in reviews {
			body = body.child(self.guardian_review_card(work, review, cx));
		}
		panel = panel.child(
			div().id("guardian-review-list").max_h(px(360.)).overflow_y_scroll().child(body),
		);
		if self.guardian.before.is_some() {
			panel = panel.child(button(
				"guardian-latest".into(),
				"Latest reviews".into(),
				cx,
				|s, cx| s.guardian_page(None, cx),
			));
		}
		if let Some(cursor) = *next_before {
			panel = panel.child(button(
				"guardian-older".into(),
				"Older reviews".into(),
				cx,
				move |s, cx| s.guardian_page(Some(cursor), cx),
			));
		}
		panel.into_any_element()
	}

	fn guardian_review_card(
		&self,
		work: &ChiefWorkItemDto,
		review: &decodex_protocol::ChiefGuardianReviewDto,
		cx: &mut Context<Self>,
	) -> gpui::Div {
		let state = match review.status {
			Status::InProgress => "No final review result received",
			Status::Approved => "Allowed by Codex review",
			Status::Denied => "Denied by Codex review",
			Status::TimedOut => "Review timed out",
			Status::Aborted => "Review stopped",
		};
		let mut card = div()
			.flex()
			.flex_col()
			.gap_2()
			.min_w_0()
			.py_2()
			.child(format!("{} · {state}", review.action_label));
		if review.status == Status::InProgress && !review.current_process {
			card = card.child(muted(
				"Saved from an earlier or disconnected process; the result is unknown.",
			));
		}
		if let Some(risk) = &review.risk_level {
			card = card.child(format!("Risk: {risk}"));
		}
		if let Some(level) = &review.user_authorization {
			card = card.child(format!("User authorization assessed by Codex: {level}"));
		}
		if let Some(submission) = review.submission {
			card = card.child(match submission {
				Submission::Pending => "Approval submission unconfirmed. No automatic retry.",
				Submission::Submitted =>
					"User approval submitted. Action execution is not confirmed by this receipt.",
				Submission::Rejected => "The approval submission was rejected.",
			});
		}
		let identity = (review.row_id, review.digest.clone());
		if self.guardian.reviewed.as_ref() != Some(&identity) {
			card = card.child(button(
				format!("guardian-review-{}", review.row_id),
				"Review action and findings".into(),
				cx,
				move |s, cx| {
					s.guardian.reviewed = Some(identity.clone());
					cx.notify();
				},
			));
		} else {
			if let Some(reason) = &review.rationale {
				card = card.child(reason.clone());
			}
			if let Some(action) = &review.action_json {
				card = card.child(div().min_w_0().text_size(px(12.)).child(action.clone()));
			}
			if let Some(reason) = &review.details_unavailable {
				card = card.child(reason.clone());
			}
			if let Some(reason) = &review.approval_unavailable {
				card = card.child(muted(reason));
			}
			if review.can_approve
				&& !self.guardian.stale
				&& !self.guardian.pending.contains_key(&review.row_id)
				&& self.guardian.mutation.is_none()
			{
				let owner = work.id.clone();
				let row = review.row_id;
				let digest = review.digest.clone();
				card = card
					.child(muted("Send your approval to Codex. This does not rerun the action."))
					.child(button(
						format!("guardian-approve-{row}"),
						"Approve this action".into(),
						cx,
						move |s, cx| s.approve_guardian(&owner, row, &digest, cx),
					));
			}
		}
		card
	}
}

fn button(
	id: String,
	label: String,
	cx: &mut Context<ChiefSurface>,
	action: impl Fn(&mut ChiefSurface, &mut Context<ChiefSurface>) + 'static,
) -> gpui::AnyElement {
	let action = std::rc::Rc::new(action);
	let click = action.clone();
	div()
		.id(SharedString::from(id.clone()))
		.debug_selector(move || id)
		.role(Role::Button)
		.tab_index(0)
		.aria_label(label.clone())
		.cursor_pointer()
		.text_color(rgb(ui_theme::BLUE))
		.py_1()
		.on_click(cx.listener(move |s, _, _, cx| click(s, cx)))
		.on_key_down(cx.listener(move |s, event: &gpui::KeyDownEvent, _, cx| {
			if ["enter", "space"].contains(&event.keystroke.key.as_str()) {
				cx.stop_propagation();
				action(s, cx);
			}
		}))
		.child(label)
		.into_any_element()
}

#[cfg(test)]
mod tests {
	use super::*;
	fn review() -> decodex_protocol::ChiefGuardianReviewDto {
		decodex_protocol::ChiefGuardianReviewDto {
			row_id: 1,
			digest: "original".into(),
			action_label: "Shell command".into(),
			status: Status::Denied,
			risk_level: Some("high".into()),
			user_authorization: Some("low".into()),
			rationale: Some("This command was not requested.".into()),
			action_json: Some("{\"command\":\"echo fixture\",\"cwd\":\"/tmp\"}".into()),
			details_unavailable: None,
			current_process: true,
			submission: None,
			submission_key: None,
			can_approve: true,
			approval_unavailable: None,
		}
	}
	fn page(review: decodex_protocol::ChiefGuardianReviewDto) -> Reviews {
		Reviews::Available { reviews: vec![review], next_before: None }
	}
	fn seed(s: &mut ChiefSurface) {
		s.apply_result(Ok(ChiefSnapshotResult::Available(ChiefSnapshotDto {
			workspaces: vec![],
			dependencies: vec![],
			pending_events: vec![],
			work_items: vec![ChiefWorkItemDto {
				id: "root".into(),
				parent_goal_id: None,
				kind: decodex_protocol::ChiefWorkKindDto::Goal,
				title: "Chief".into(),
				codex_thread_id: Some("thread".into()),
				active_turn_id: None,
				dispatch_state: ChiefDispatchStateDto::Idle,
				status: ChiefWorkStatusDto::Open,
				next_check_at_micros: None,
				created_at_micros: 1,
				updated_at_micros: 1,
			}],
		})));
		s.guardian = Panel {
			owner: Some("root".into()),
			expanded: true,
			result: Some(page(review())),
			..Default::default()
		};
	}
	#[test]
	fn an_old_receipt_cannot_clear_a_newer_unconfirmed_submission() {
		let mut panel = Panel::default();
		panel.pending.insert(1, "new-command".into());
		let mut old = review();
		old.submission = Some(Submission::Rejected);
		old.submission_key = Some("old-command".into());
		panel.observe(page(old.clone()));
		assert!(panel.pending.contains_key(&1));
		old.submission_key = Some("new-command".into());
		old.submission = None;
		panel.observe(page(old.clone()));
		assert!(panel.pending.contains_key(&1));
		old.submission = Some(Submission::Pending);
		old.can_approve = false;
		panel.observe(page(old));
		assert!(!panel.pending.contains_key(&1));
		let retained = panel.result.clone();
		panel.observe(Reviews::Unavailable);
		assert!(panel.stale);
		assert_eq!(panel.result, retained);
	}
	#[test]
	fn completion_is_bound_to_the_active_command_and_preserves_unknown_sends() {
		let mut panel = Panel { mutation_key: Some("new".into()), ..Default::default() };
		panel.pending.insert(1, "new".into());
		assert!(!panel.finish_submission(1, "old", Err(())));
		assert!(panel.pending.contains_key(&1));
		assert!(panel.finish_submission(
			1,
			"new",
			Ok(ChiefCommandResponse::PotentiallyDispatched {
				failure: decodex_protocol::ClientFailure::ProtocolDisconnected
			})
		));
		assert!(panel.pending.contains_key(&1));
		assert!(panel.feedback.contains("unconfirmed"));
		panel.mutation_key = Some("new".into());
		assert!(panel.finish_submission(1, "new", Err(())));
		assert!(!panel.pending.contains_key(&1));
		assert!(panel.feedback.contains("No approval was sent"));
	}
	#[gpui::test]
	fn review_is_separate_from_approval_and_stale_or_disconnected_details_cannot_submit(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		surface.update(visual, |s, _| seed(s));
		visual.update(|window, cx| {
			window.resize(gpui::size(px(1180.0), px(1200.0)));
			window.draw(cx).clear();
		});
		assert!(visual.debug_bounds("guardian-approve-1").is_none());
		let bounds = visual.debug_bounds("guardian-review-1").expect("review control");
		visual.simulate_click(bounds.center(), gpui::Modifiers::default());
		surface.update(visual, |s, _| {
			assert!(s.guardian.feedback.is_empty());
			assert!(s.guardian.pending.is_empty());
		});
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		let bounds = visual.debug_bounds("guardian-approve-1").expect("explicit approval");
		visual.simulate_click(bounds.center(), gpui::Modifiers::default());
		surface.update(visual, |s, cx| {
			assert_eq!(s.guardian.feedback, "No service profile is configured.");
			s.guardian.feedback.clear();
			cx.notify();
		});
		visual.simulate_keystrokes("space");
		surface.update(visual, |s, cx| {
			assert_eq!(s.guardian.feedback, "No service profile is configured.");
			s.guardian.feedback.clear();
			if let Some(Reviews::Available { reviews, .. }) = &mut s.guardian.result {
				reviews[0].digest = "changed".into();
			}
			s.approve_guardian("root", 1, "original", cx);
			assert!(s.guardian.feedback.is_empty());
			s.guardian.reviewed = Some((1, "changed".into()));
			s.guardian_disconnected();
			s.approve_guardian("root", 1, "changed", cx);
			assert!(s.guardian.feedback.is_empty());
		});
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		assert!(visual.debug_bounds("guardian-approve-1").is_none());
	}
}
