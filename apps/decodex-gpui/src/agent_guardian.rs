//! Exact saved action reviews and explicit approval submission, independent of execution.
use std::{collections::BTreeMap, rc::Rc};

use gpui::{AnyElement, Div, KeyDownEvent};
use tokio::runtime::Builder;
use ui_theme::BLUE;

use crate::{shell::agent_surface::*, ui_loading};
use decodex_protocol::{
	AgentGuardianDetailResult as Detail, AgentGuardianReviewDto,
	AgentGuardianReviewsResult as Reviews, AgentGuardianStatus as Status,
	AgentGuardianSubmission as Submission,
};

#[derive(Default)]
pub(super) struct Panel {
	owner: Option<String>,
	result: Option<Reviews>,
	before: Option<i64>,
	epoch: u64,
	expanded: bool,
	reviewed: Option<(i64, String)>,
	pending: BTreeMap<i64, String>,
	stale: bool,
	pub(super) feedback: String,
	request: Option<Task<()>>,
	mutation: Option<Task<()>>,
	mutation_key: Option<String>,
	detail: Option<DetailReader>,
	detail_request: Option<Task<()>>,
	detail_serial: u64,
}
impl Panel {
	fn finish_submission(
		&mut self,
		row: i64,
		key: &str,
		result: Result<AgentCommandResponse, ()>,
	) -> bool {
		if self.mutation_key.as_deref() != Some(key) {
			return false;
		}

		self.mutation = None;
		self.mutation_key = None;
		self.feedback = match result {
			Ok(AgentCommandResponse::Accepted { .. }) =>
				"Approval submitted to Codex. This does not rerun the action.",
			Ok(AgentCommandResponse::Rejected { .. }) => {
				self.pending.remove(&row);
				"Approval was not accepted. Refresh and review the current details."
			},
			Err(()) => {
				self.pending.remove(&row);
				"No approval was sent. Check the service connection before trying again."
			},
			Ok(AgentCommandResponse::PotentiallyDispatched { .. }) =>
				"Approval submission is unconfirmed. It will not be sent again automatically.",
		}
		.into();

		true
	}

	fn observe(&mut self, result: Reviews) {
		if let Reviews::Available { reviews, .. } = &result {
			if self.detail.as_ref().is_some_and(|detail| {
				!reviews.iter().any(|review| {
					review.row_id == detail.row
						&& review.digest == detail.digest
						&& review.details_paged
				})
			}) {
				self.detail = None;
				self.detail_request = None;
				self.detail_serial = self.detail_serial.wrapping_add(1);
			}

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

struct DetailReader {
	row: i64,
	digest: String,
	page: Option<Detail>,
	starts: Vec<usize>,
	visited_end: usize,
	complete: bool,
}
impl DetailReader {
	fn observe(&mut self, result: Detail) {
		if let Detail::Available { offset, text, total_bytes, .. } = &result {
			if matches!(&self.page, Some(Detail::Available { total_bytes: previous, .. }) if previous != total_bytes)
			{
				self.observe(Detail::Unavailable);

				return;
			}
			if *offset <= self.visited_end {
				self.visited_end = self.visited_end.max(offset + text.len());
			}

			self.complete = self.visited_end == *total_bytes;

			if !self.starts.contains(offset) {
				self.starts.push(*offset);
			}
		} else {
			self.complete = false;
			self.visited_end = 0;

			self.starts.clear();
		}

		self.page = Some(result);
	}
}

impl AgentSurface {
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
		let epoch = self.guardian.epoch;
		let before = self.guardian.before;
		let request = cx.background_executor().spawn(async move {
			let runtime = Builder::new_current_thread().enable_all().build().ok()?;

			runtime.block_on(AgentClient::new(profile).guardian_reviews(work_id, before)).ok()
		});

		self.guardian.request = Some(cx.spawn(async move |surface, cx| {
			let result = request.await.unwrap_or(Reviews::Unavailable);
			let _ = surface.update(cx, |s, cx| {
				if s.selected.as_ref() != Some(&work) || s.guardian.epoch != epoch {
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

	pub(super) fn invalidate_guardian(&mut self, next: &AgentSnapshotDto) {
		let Some(work) = &self.guardian.owner else { return };
		let before =
			self.snapshot.as_ref().and_then(|s| s.work_items.iter().find(|w| &w.id == work));
		let after = next.work_items.iter().find(|w| &w.id == work);

		if !matches!((before, after), (Some(a), Some(b)) if a.codex_thread_id == b.codex_thread_id)
			|| self.snapshot.as_ref().is_none_or(|s| s.runtime_source != next.runtime_source)
		{
			self.guardian_disconnected();
		}
	}

	pub(super) fn guardian_disconnected(&mut self) {
		self.guardian.epoch = self.guardian.epoch.wrapping_add(1);
		self.guardian.request = None;
		self.guardian.mutation = None;
		self.guardian.mutation_key = None;
		self.guardian.stale = true;
		self.guardian.detail_request = None;
		self.guardian.detail_serial = self.guardian.detail_serial.wrapping_add(1);
	}

	fn guardian_page(&mut self, before: Option<i64>, cx: &mut Context<Self>) {
		self.guardian.request = None;
		self.guardian.epoch = self.guardian.epoch.wrapping_add(1);
		self.guardian.before = before;
		self.guardian.result = None;
		self.guardian.reviewed = None;
		self.guardian.detail = None;
		self.guardian.detail_request = None;
		self.guardian.detail_serial = self.guardian.detail_serial.wrapping_add(1);

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

		if !reviews.iter().any(|r| {
			r.row_id == row
				&& r.digest == digest
				&& r.can_approve
				&& self.guardian_detail_complete(r)
		}) {
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
		let owner = work.to_owned();

		self.guardian.feedback = "Submitting approval to Codex…".into();

		let key = IdempotencyKey::new(unique_command()).expect("bounded identity");

		self.guardian.pending.insert(row, key.as_str().into());

		let submitted_key = key.as_str().to_owned();

		self.guardian.mutation_key = Some(submitted_key.clone());

		let request = cx.background_executor().spawn(async move {
			let runtime = Builder::new_current_thread().enable_all().build().map_err(|_| ())?;

			runtime
				.block_on(AgentClient::new(profile).execute(
					AgentActionDto::ApproveGuardianDenial {
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
				s.finish_guardian_submission(&owner, row, &submitted_key, result, cx);
			});
		}));

		cx.notify();
	}

	fn finish_guardian_submission(
		&mut self,
		owner: &str,
		row: i64,
		key: &str,
		result: Result<AgentCommandResponse, ()>,
		cx: &mut Context<Self>,
	) {
		if self.guardian.owner.as_deref() != Some(owner)
			|| self.guardian.mutation_key.as_deref() != Some(key)
		{
			return;
		}
		if !self.guardian.finish_submission(row, key, result) {
			return;
		}

		// Refreshing this list does not invalidate an exact saved-detail read.
		self.guardian.request = None;

		self.load_guardian_reviews(cx);
		cx.notify();
	}

	fn guardian_detail_complete(&self, review: &AgentGuardianReviewDto) -> bool {
		!review.details_paged
			|| self.guardian.detail.as_ref().is_some_and(|detail| {
				detail.row == review.row_id && detail.digest == review.digest && detail.complete
			})
	}

	fn load_guardian_detail(
		&mut self,
		row: i64,
		digest: String,
		offset: usize,
		cx: &mut Context<Self>,
	) {
		let (Some(work), Some(profile)) = (self.selected.clone(), self.profile.clone()) else {
			return;
		};

		if self.guardian.owner.as_ref() != Some(&work) {
			return;
		}

		let Some(Reviews::Available { reviews, .. }) = &self.guardian.result else {
			return;
		};

		if !reviews.iter().any(|r| r.row_id == row && r.digest == digest && r.details_paged) {
			return;
		}

		let (Ok(work_id), Ok(review_digest)) =
			(EntityId::new(work.clone()), WireText::new(&digest))
		else {
			return;
		};

		if !self.guardian.detail.as_ref().is_some_and(|d| d.row == row && d.digest == digest) {
			self.guardian.detail = Some(DetailReader {
				row,
				digest: digest.clone(),
				page: None,
				starts: Vec::new(),
				visited_end: 0,
				complete: false,
			});
		}

		self.guardian.detail_serial = self.guardian.detail_serial.wrapping_add(1);

		let serial = self.guardian.detail_serial;
		let epoch = self.guardian.epoch;
		let request = cx.background_executor().spawn(async move {
			let runtime = Builder::new_current_thread().enable_all().build().ok()?;

			runtime
				.block_on(AgentClient::new(profile).guardian_detail(
					work_id,
					row,
					review_digest,
					offset,
				))
				.ok()
		});

		self.guardian.detail_request = Some(cx.spawn(async move |surface, cx| {
			let result = request.await.unwrap_or(Detail::Unavailable);
			let _ = surface.update(cx, |s, cx| {
				if s.selected.as_ref() != Some(&work)
					|| s.guardian.epoch != epoch
					|| s.guardian.detail_serial != serial
				{
					return;
				}

				s.guardian.detail_request = None;

				if let Some(detail) = &mut s.guardian.detail
					&& detail.row == row
					&& detail.digest == digest
				{
					detail.observe(result);
				}

				cx.notify();
			});
		}));

		cx.notify();
	}

	pub(super) fn guardian_panel(
		&self,
		work: &AgentWorkItemDto,
		cx: &mut Context<Self>,
	) -> AnyElement {
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

	fn guardian_detail_panel(
		&self,
		review: &AgentGuardianReviewDto,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let row = review.row_id;
		let digest = review.digest.clone();
		let reader = self.guardian.detail.as_ref().filter(|d| d.row == row && d.digest == digest);
		let mut panel = div().flex().flex_col().gap_2().min_w_0();

		if let Some(Detail::Available { offset, text, next_offset, .. }) =
			reader.and_then(|d| d.page.as_ref())
		{
			let position =
				reader.and_then(|d| d.starts.iter().position(|start| start == offset)).unwrap_or(0);

			panel = panel.child(format!("Action details · page {}", position + 1)).child(
				div()
					.id("guardian-detail-text")
					.max_h(px(240.))
					.overflow_scroll()
					.text_size(px(12.))
					.child(text.clone()),
			);

			if self.guardian.detail_request.is_none() {
				if let Some(previous) =
					reader.and_then(|d| position.checked_sub(1).map(|i| d.starts[i]))
				{
					let digest = digest.clone();

					panel = panel.child(button(
						"guardian-detail-previous".into(),
						"Previous details".into(),
						cx,
						move |s, cx| s.load_guardian_detail(row, digest.clone(), previous, cx),
					));
				}
				if let Some(next) = *next_offset {
					let digest = digest.clone();

					panel = panel.child(button(
						"guardian-detail-next".into(),
						"Next details".into(),
						cx,
						move |s, cx| s.load_guardian_detail(row, digest.clone(), next, cx),
					));
				}
			}
			if reader.is_some_and(|d| d.complete) {
				panel = panel.child(muted("All detail pages opened."));
			}
		} else if self.guardian.detail_request.is_none() {
			panel = panel.child("Open the complete action and findings before approving.").child(
				button(
					"guardian-detail-reload".into(),
					"Read complete details".into(),
					cx,
					move |s, cx| s.load_guardian_detail(row, digest.clone(), 0, cx),
				),
			);

			if matches!(reader.and_then(|d| d.page.as_ref()), Some(Detail::Unavailable)) {
				panel = panel.child("Details are unavailable or changed. Refresh the review list.");
			}
		}

		if self.guardian.detail_request.is_some() {
			panel = panel.child(ui_loading::loading("Loading review"));
		}

		panel.into_any_element()
	}

	fn guardian_review_card(
		&self,
		work: &AgentWorkItemDto,
		review: &AgentGuardianReviewDto,
		cx: &mut Context<Self>,
	) -> Div {
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

					s.load_guardian_detail(identity.0, identity.1.clone(), 0, cx);
					cx.notify();
				},
			));
		} else {
			if review.details_paged {
				card = card.child(self.guardian_detail_panel(review, cx));
			}

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
				&& self.guardian_detail_complete(review)
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
	cx: &mut Context<AgentSurface>,
	action: impl Fn(&mut AgentSurface, &mut Context<AgentSurface>) + 'static,
) -> AnyElement {
	let action = Rc::new(action);
	let click = action.clone();

	div()
		.id(SharedString::from(id.clone()))
		.debug_selector(move || id)
		.role(Role::Button)
		.tab_index(0)
		.aria_label(label.clone())
		.cursor_pointer()
		.text_color(rgb(BLUE))
		.py_1()
		.on_click(cx.listener(move |s, _, _, cx| click(s, cx)))
		.on_key_down(cx.listener(move |s, event: &KeyDownEvent, _, cx| {
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
	use crate::shell::agent_surface::guardian::*;
	use decodex_protocol::{
		CURRENT_VERSION, ClientMessage, CommandPayload, QueryPayload, QueryResultEnvelope,
		QueryResultPayload, ServerId, ServerMessage,
	};

	use futures_util::{SinkExt as _, StreamExt as _};
	use tokio_tungstenite::tungstenite::Message;

	use crate::shell::agent_surface::wire_test_support;

	#[gpui::test]
	fn paged_guardian_action_requires_every_page_and_discards_changed_evidence(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		let page = |offset: usize, text: &str, next_offset| Detail::Available {
			row_id: 1,
			digest: "original".into(),
			offset,
			text: text.into(),
			total_bytes: 15,
			next_offset,
		};

		surface.update(visual, |s, _| {
			seed(s);

			if let Some(Reviews::Available { reviews, .. }) = &mut s.guardian.result {
				reviews[0].details_paged = true;
				reviews[0].action_json = None;
			}

			s.guardian.reviewed = Some((1, "original".into()));
			s.guardian.detail = Some(DetailReader {
				row: 1,
				digest: "original".into(),
				page: None,
				starts: Vec::new(),
				visited_end: 0,
				complete: false,
			});

			s.guardian.detail.as_mut().unwrap().observe(page(0, "first", Some(5)));
		});

		visual.update(|window, cx| {
			window.resize(gpui::size(px(1_180.), px(1_200.)));
			window.draw(cx).clear();
		});

		assert!(visual.debug_bounds("guardian-detail-next").is_some());
		assert!(visual.debug_bounds("guardian-approve-1").is_none());

		surface.update(visual, |s, cx| {
			let detail = s.guardian.detail.as_mut().unwrap();

			detail.observe(page(10, "FINAL", None));

			assert!(!detail.complete, "a skipped page must not enable approval");

			detail.observe(page(5, " next", Some(10)));
			detail.observe(page(10, "FINAL", None));

			assert!(detail.complete);

			cx.notify();
		});

		visual.update(|window, cx| {
			window.draw(cx).clear();
		});

		assert!(visual.debug_bounds("guardian-approve-1").is_some());

		surface.update(visual, |s, cx| {
			let mut changed = review();

			changed.details_paged = true;
			changed.digest = "changed".into();

			s.guardian.observe(Reviews::Available { reviews: vec![changed], next_before: None });

			assert!(s.guardian.detail.is_none());

			s.approve_guardian("root", 1, "original", cx);

			assert!(s.guardian.feedback.is_empty());

			cx.notify();
		});

		visual.update(|window, cx| {
			window.draw(cx).clear();
		});

		assert!(visual.debug_bounds("guardian-approve-1").is_none());
	}
	fn review() -> decodex_protocol::AgentGuardianReviewDto {
		decodex_protocol::AgentGuardianReviewDto {
			row_id: 1,
			digest: "original".into(),
			action_label: "Shell command".into(),
			status: Status::Denied,
			risk_level: Some("high".into()),
			user_authorization: Some("low".into()),
			rationale: Some("This command was not requested.".into()),
			action_json: Some("{\"command\":\"echo fixture\",\"cwd\":\"/tmp\"}".into()),
			details_unavailable: None,
			details_paged: false,
			current_process: true,
			submission: None,
			submission_key: None,
			can_approve: true,
			approval_unavailable: None,
		}
	}
	fn page(review: decodex_protocol::AgentGuardianReviewDto) -> Reviews {
		Reviews::Available { reviews: vec![review], next_before: None }
	}
	fn seed(s: &mut AgentSurface) {
		s.apply_result(Ok(AgentSnapshotResult::Available(AgentSnapshotDto {
			runtime_source: None,
			workspaces: vec![],
			dependencies: vec![],
			pending_events: vec![],
			work_items: vec![AgentWorkItemDto {
				id: "root".into(),
				parent_goal_id: None,
				kind: decodex_protocol::AgentWorkKindDto::Goal,
				title: "Agent".into(),
				codex_thread_id: Some("thread".into()),
				active_turn_id: None,
				dispatch_state: AgentDispatchStateDto::Idle,
				status: AgentWorkStatusDto::Open,
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
			Ok(AgentCommandResponse::PotentiallyDispatched {
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
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		surface.update(visual, |s, _| seed(s));
		visual.update(|window, cx| {
			window.resize(gpui::size(px(1_180.0), px(1_200.0)));
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
	#[gpui::test]
	fn ordinary_refresh_keeps_guardian_list_details_and_submission_receipt(
		cx: &mut gpui::TestAppContext,
	) {
		let (_dir, profile, server) = wire_test_support::fixture(|listener| async move {
			for index in 0..4 {
				let mut socket = wire_test_support::accept(&listener).await;
				let Message::Text(text) = socket.next().await.unwrap().unwrap() else {
					panic!("text request")
				};
				let request: ClientMessage = serde_json::from_str(&text).unwrap();

				if index == 2 {
					let ClientMessage::Command(command) = request else {
						panic!("one explicit submission")
					};
					let CommandPayload::Agent { action } = command.payload else {
						panic!("Agent action")
					};

					assert!(
						matches!(&*action, AgentActionDto::ApproveGuardianDenial { work_id, review_row: 1, review_digest } if work_id.as_str() == "root" && review_digest.as_str() == "original")
					);

					socket.close(None).await.unwrap();

					continue;
				}

				let ClientMessage::Query(query) = request else {
					panic!("readback, not another approval")
				};
				let payload = if index == 1 {
					assert!(
						matches!(&query.payload, QueryPayload::GetAgentGuardianDetail { work_id, review_row: 1, review_digest, offset: 0 } if work_id.as_str() == "root" && review_digest.as_str() == "original")
					);

					QueryResultPayload::AgentGuardianDetail(Detail::Available {
						row_id: 1,
						digest: "original".into(),
						offset: 0,
						text: "whole".into(),
						total_bytes: 5,
						next_offset: None,
					})
				} else {
					assert!(
						matches!(&query.payload, QueryPayload::GetAgentGuardianReviews { work_id, before: None } if work_id.as_str() == "root")
					);

					let mut saved = review();

					saved.details_paged = true;

					if index == 3 {
						saved.submission = Some(Submission::Pending);
						saved.can_approve = false;
					}

					QueryResultPayload::AgentGuardianReviews(page(saved))
				};
				let response = ServerMessage::QueryResult(QueryResultEnvelope {
					version: CURRENT_VERSION,
					server_id: ServerId::new(super::super::wire_test_support::SERVER).unwrap(),
					query_id: query.query_id,
					payload,
				});

				socket
					.send(Message::Text(serde_json::to_string(&response).unwrap().into()))
					.await
					.unwrap();
			}
		});
		let surface = cx.new(AgentSurface::new);

		surface.update(cx, |s, _| {
			seed(s);

			s.profile = Some(profile);
		});

		for stage in 0..3 {
			surface.update(cx, |s, cx| {
				match stage {
					0 => s.load_guardian_reviews(cx),
					1 => {
						s.guardian.reviewed = Some((1, "original".into()));

						s.load_guardian_detail(1, "original".into(), 0, cx);
					},
					_ => s.approve_guardian("root", 1, "original", cx),
				}

				s.generation += 1;

				s.apply_result(Ok(AgentSnapshotResult::Available(s.snapshot.clone().unwrap())));
			});

			cx.run_until_parked();
			surface.read_with(cx, |s, _| match stage {
				0 => assert!(s.guardian.request.is_none(), "refresh stranded the review list"),
				1 => {
					assert!(s.guardian.detail_request.is_none(), "refresh stranded detail read");
					assert!(s.guardian.detail.as_ref().unwrap().complete);
				},
				_ => {
					assert!(s.guardian.mutation.is_none(), "refresh stranded submission receipt");
					assert!(s.guardian.feedback.contains("unconfirmed"));
					assert!(
						s.guardian.pending.contains_key(&1),
						"a lost reply must not reopen submission"
					);
				},
			});
		}

		server.join().unwrap();
	}

	#[gpui::test]
	fn guardian_history_stays_visible_but_stale_after_source_or_snapshot_failure(
		cx: &mut gpui::TestAppContext,
	) {
		let surface = cx.new(AgentSurface::new);

		for change in ["source", "thread", "removed", "failed", "unavailable"] {
			surface.update(cx, |s, _| {
				seed(s);

				s.guardian.pending.insert(1, "unconfirmed-command".into());

				let saved = s.guardian.result.clone();
				let epoch = s.guardian.epoch;

				match change {
					"source" => {
						let mut next = s.snapshot.clone().unwrap();

						next.runtime_source = Some(EntityId::new("new-source").unwrap());

						s.apply_result(Ok(AgentSnapshotResult::Available(next)));
					},
					"thread" => {
						let mut next = s.snapshot.clone().unwrap();

						next.work_items[0].codex_thread_id = Some("replacement-thread".into());

						s.apply_result(Ok(AgentSnapshotResult::Available(next)));
					},
					"removed" => {
						let mut next = s.snapshot.clone().unwrap();

						next.work_items.clear();
						s.apply_result(Ok(AgentSnapshotResult::Available(next)));
					},
					"failed" => s.apply_result(Err(())),
					_ => s.apply_result(Ok(AgentSnapshotResult::Unavailable)),
				}

				assert!(s.guardian.stale, "{change}");
				assert_ne!(s.guardian.epoch, epoch);
				assert_eq!(s.guardian.result, saved);
				assert!(s.guardian.pending.contains_key(&1));
			});
		}
	}
	#[gpui::test]
	fn submission_readback_keeps_a_concurrent_detail_read(cx: &mut gpui::TestAppContext) {
		use decodex_protocol::{
			CURRENT_VERSION, ClientMessage, QueryPayload, QueryResultEnvelope, QueryResultPayload,
			ServerId, ServerMessage,
		};

		let (_dir, profile, server) = wire_test_support::fixture(|listener| async move {
			let (mut lists, mut details) = (0, 0);

			for _ in 0..2 {
				let mut socket = wire_test_support::accept(&listener).await;
				let Message::Text(text) = socket.next().await.unwrap().unwrap() else {
					panic!("text query")
				};
				let ClientMessage::Query(query) = serde_json::from_str(&text).unwrap() else {
					panic!("readback only")
				};
				let payload = match &query.payload {
					QueryPayload::GetAgentGuardianDetail {
						work_id,
						review_row: 1,
						review_digest,
						offset: 0,
					} => {
						assert_eq!(work_id.as_str(), "root");
						assert_eq!(review_digest.as_str(), "original");

						details += 1;

						QueryResultPayload::AgentGuardianDetail(Detail::Available {
							row_id: 1,
							digest: "original".into(),
							offset: 0,
							text: "whole".into(),
							total_bytes: 5,
							next_offset: None,
						})
					},
					QueryPayload::GetAgentGuardianReviews { work_id, before: None } => {
						assert_eq!(work_id.as_str(), "root");

						lists += 1;

						let mut saved = review();

						saved.details_paged = true;
						saved.submission = Some(Submission::Pending);
						saved.can_approve = false;

						QueryResultPayload::AgentGuardianReviews(page(saved))
					},
					_ => panic!("exact review readback"),
				};
				let response = ServerMessage::QueryResult(QueryResultEnvelope {
					version: CURRENT_VERSION,
					server_id: ServerId::new(super::super::wire_test_support::SERVER).unwrap(),
					query_id: query.query_id,
					payload,
				});

				socket
					.send(Message::Text(serde_json::to_string(&response).unwrap().into()))
					.await
					.unwrap();
			}

			assert_eq!((lists, details), (1, 1));
		});
		let surface = cx.new(AgentSurface::new);

		surface.update(cx, |s, cx| {
			seed(s);

			if let Some(Reviews::Available { reviews, .. }) = &mut s.guardian.result {
				reviews[0].details_paged = true;
			}

			s.profile = Some(profile);
			s.guardian.mutation_key = Some("submission".into());

			s.guardian.pending.insert(1, "submission".into());
			s.load_guardian_detail(1, "original".into(), 0, cx);

			assert!(s.guardian.detail_request.is_some());

			// Settle the submission before the detail future can deliver its response.
			s.finish_guardian_submission(
				"root",
				1,
				"submission",
				Ok(AgentCommandResponse::PotentiallyDispatched {
					failure: decodex_protocol::ClientFailure::ProtocolDisconnected,
				}),
				cx,
			);
		});

		cx.run_until_parked();
		server.join().unwrap();
		surface.read_with(cx, |s, _| {
			assert!(s.guardian.detail_request.is_none(), "submission readback stranded concurrent details");
			assert!(s.guardian.detail.as_ref().unwrap().complete);
			assert!(matches!(s.guardian.detail.as_ref().and_then(|d| d.page.as_ref()), Some(Detail::Available { text, .. }) if text == "whole"));
			assert!(s.guardian.pending.contains_key(&1));
			assert!(s.guardian.feedback.contains("unconfirmed"));
		});
	}
}
