//! Review provider findings before an explicit, source-bound continuation.
use super::*;

impl ChiefSurface {
	fn acknowledge_misalignment(&mut self, work: &str, digest: &str, cx: &mut Context<Self>) {
		let Some((owner, ChiefHistoryResult::Available { misalignment: Some(review), .. })) =
			&self.history
		else {
			return;
		};
		if owner != work
			|| review.review_id != digest
			|| self.misalignment_reviewed.as_ref() != Some(&(work.into(), digest.into()))
			|| review.explanation.is_none()
			|| review.continuation.is_none()
		{
			return;
		}
		let (Ok(work_id), Ok(review_id)) =
			(EntityId::new(work), decodex_protocol::WireText::new(digest))
		else {
			return;
		};
		self.execute(ChiefActionDto::ContinueMisalignment { work_id, review_id }, None, cx);
	}

	pub(super) fn misalignment_panel(
		&self,
		work: &ChiefWorkItemDto,
		cx: &mut Context<Self>,
	) -> gpui::AnyElement {
		let Some((owner, ChiefHistoryResult::Available { misalignment: Some(review), .. })) =
			&self.history
		else {
			return div().into_any_element();
		};
		if owner != &work.id {
			return div().into_any_element();
		}
		let mut panel=div().p_3().rounded(px(8.0)).border_1().border_color(rgba(0xffffff30)).flex().flex_col().gap_3()
            .child("Conversation paused as a precaution")
            .child(muted("Codex could not confirm that the agent was following your instructions. Review the findings before continuing."));
		let identity = (work.id.clone(), review.review_id.clone());
		if self.misalignment_reviewed.as_ref() != Some(&identity) {
			let keyboard_identity = identity.clone();
			return panel
				.child(
					div()
						.id("misalignment-review")
						.debug_selector(|| "misalignment-review".into())
						.role(Role::Button)
						.tab_index(0)
						.aria_label("Review findings")
						.p_2()
						.cursor_pointer()
						.on_click(cx.listener(move |s, _, _, cx| {
							s.misalignment_reviewed = Some(identity.clone());
							cx.notify();
						}))
						.on_key_down(cx.listener(move |s, event: &gpui::KeyDownEvent, _, cx| {
							if matches!(event.keystroke.key.as_str(), "enter" | "space") {
								s.misalignment_reviewed = Some(keyboard_identity.clone());
								cx.stop_propagation();
								cx.notify();
							}
						}))
						.child("Review findings"),
				)
				.into_any_element();
		}
		if let Some(message) = &review.continuation {
			panel = panel.child("Continuation request (quoted)").child(format!("{message:?}"));
		}
		panel = panel.child(review.explanation.clone().unwrap_or_else(|| {
			"Detailed findings are unavailable. Start or resume another conversation.".into()
		}));
		if review.explanation.is_some() && review.continuation.is_some() {
			let owner = work.id.clone();
			let digest = review.review_id.clone();
			let keyboard_owner = owner.clone();
			let keyboard_digest = digest.clone();
			panel = panel.child(
				div()
					.id("misalignment-continue")
					.debug_selector(|| "misalignment-continue".into())
					.role(Role::Button)
					.tab_index(0)
					.aria_label("Acknowledge findings and continue")
					.p_2()
					.cursor_pointer()
					.on_click(cx.listener(move |s, _, _, cx| {
						s.acknowledge_misalignment(&owner, &digest, cx)
					}))
					.on_key_down(cx.listener(move |s, event: &gpui::KeyDownEvent, _, cx| {
						if matches!(event.keystroke.key.as_str(), "enter" | "space") {
							s.acknowledge_misalignment(&keyboard_owner, &keyboard_digest, cx);
							cx.stop_propagation();
						}
					}))
					.child("Acknowledge findings and continue"),
			);
		}
		panel.into_any_element()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	#[gpui::test]
	fn review_requires_second_click_and_stale_findings_cannot_continue(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		surface.update(visual, |s, _| {
			s.apply_result(Ok(ChiefSnapshotResult::Available(ChiefSnapshotDto {
				runtime_source: None,
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
			s.history = Some((
				"root".into(),
				ChiefHistoryResult::Available {
					questions: vec![],
					questions_truncated: false,
					questions_recovering: false,
					misalignment: Some(Box::new(decodex_protocol::ChiefMisalignmentDto {
						review_id: "review-one".into(),
						explanation: Some("Please review the scope.".into()),
						continuation: Some("Continue with clarified scope.".into()),
					})),
					usage: None,
					entries: vec![],
					has_more: false,
					next_before: None,
					live: vec![],
				},
			));
			s.feedback.clear();
		});
		visual.update(|window, cx| {
			window.resize(gpui::size(px(1180.0), px(1200.0)));
			window.draw(cx).clear();
		});
		assert!(visual.debug_bounds("misalignment-continue").is_none());
		let bounds = visual.debug_bounds("misalignment-review").expect("review entry");
		visual.simulate_click(bounds.center(), gpui::Modifiers::default());
		surface.update(visual, |s, _| {
			assert!(s.feedback.is_empty());
			assert!(s.command_task.is_none());
		});
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		let bounds = visual.debug_bounds("misalignment-continue").expect("explicit continuation");
		visual.simulate_click(bounds.center(), gpui::Modifiers::default());
		surface.update(visual, |s, cx| {
			assert_eq!(s.feedback, "No service profile is configured.");
			s.feedback.clear();
			if let Some((_, ChiefHistoryResult::Available { misalignment: Some(review), .. })) =
				&mut s.history
			{
				review.review_id = "review-two".into();
			}
			s.acknowledge_misalignment("root", "review-one", cx);
			assert!(s.feedback.is_empty());
		});
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		assert!(visual.debug_bounds("misalignment-continue").is_none());
		assert!(visual.debug_bounds("misalignment-review").is_some());
	}
}
