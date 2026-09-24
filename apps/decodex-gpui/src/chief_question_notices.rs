//! Live question notices for the observed conversation; history is never an arrival.
use super::{ChiefHistoryResult, ChiefSurface, LoadState};
#[cfg(test)] use decodex_protocol::{ChiefAsyncQuestionDto, EntityId};
use std::collections::{BTreeMap, BTreeSet};
use unicode_segmentation::UnicodeSegmentation as _;

#[derive(Default)]
pub(super) struct QuestionNotices {
	scope: Option<String>,
	ready: bool,
	seen: BTreeSet<String>,
	pending: BTreeMap<String, String>,
	identity: Option<String>,
}

impl QuestionNotices {
	pub(super) fn begin(&mut self, scope: Option<String>) {
		if self.scope != scope {
			*self = Self { scope, ..Self::default() };
		}
	}

	fn observe(&mut self, history: &ChiefHistoryResult, enabled: bool) {
		let ChiefHistoryResult::Available {
			questions,
			questions_truncated: false,
			questions_recovering: false,
			..
		} = history
		else {
			self.ready = false;
			self.pending.clear();
			self.identity = None;
			return;
		};
		self.pending.retain(|id, title| {
			questions.iter().any(|q| &q.id == id && q.arrived_live && &q.title == title)
		});
		if !enabled {
			self.pending.clear();
		}
		let mut added = Vec::new();
		for question in questions {
			let unseen = self.seen.insert(question.id.clone());
			if self.scope.is_some() && self.ready && enabled && unseen && question.arrived_live {
				self.pending.insert(question.id.clone(), question.title.clone());
				added.push(&question.id);
			}
		}
		if !added.is_empty() {
			self.identity = Some(serde_json::json!([self.scope, added]).to_string());
		} else if self.pending.is_empty() {
			self.identity = None;
		}
		self.ready = self.scope.is_some();
	}

	fn notice(&self) -> Option<(String, String)> {
		if self.pending.is_empty() {
			return None;
		}
		let title = if self.pending.len() == 1 {
			let text = self.pending.values().next()?.trim();
			if text.is_empty() {
				"Question requested".to_owned()
			} else {
				let mut title = text.graphemes(true).take(30).collect::<String>();
				if title.len() < text.len() {
					title.push('…');
				}
				title
			}
		} else {
			format!("{} questions requested", self.pending.len())
		};
		Some((self.identity.clone()?, title))
	}
}

impl ChiefSurface {
	pub(super) fn question_notice_scope(&self, work: &str) -> Option<String> {
		if self.displayed_load_state() != &LoadState::Ready || self.native_agents.selected.is_some()
		{
			return None;
		}
		let snapshot = self.snapshot.as_ref()?;
		let source = snapshot.runtime_source.as_ref()?;
		let thread = snapshot.work_items.iter().find(|w| w.id == work)?.codex_thread_id.as_ref()?;
		Some(serde_json::json!([work, thread, source]).to_string())
	}

	pub(super) fn observe_question_notices(&mut self, history: &ChiefHistoryResult) {
		self.question_notices.observe(history, crate::shell::question_notice_preference(None));
	}

	pub(crate) fn question_arrival_notice(&self) -> Option<(String, String)> {
		if !crate::shell::question_notice_preference(None) {
			return None;
		}
		let scope = self.question_notice_scope(self.selected.as_deref()?)?;
		if self.question_notices.scope.as_ref() != Some(&scope) {
			return None;
		}
		self.question_notices.notice()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use gpui::AppContext as _;
	fn history(questions: Vec<ChiefAsyncQuestionDto>) -> ChiefHistoryResult {
		ChiefHistoryResult::Available {
			questions,
			questions_truncated: false,
			questions_recovering: false,
			misalignment: None,
			usage: None,
			entries: vec![],
			has_more: false,
			next_before: None,
			live: vec![],
		}
	}
	fn question(id: &str, live: bool) -> ChiefAsyncQuestionDto {
		ChiefAsyncQuestionDto {
			id: id.into(),
			title: "Same title".into(),
			options: vec![],
			arrived_live: live,
		}
	}
	#[gpui::test]
	fn notices_require_the_current_work_thread_and_runtime_source(cx: &mut gpui::TestAppContext) {
		let surface = cx.new(ChiefSurface::new);
		surface.update(cx, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.state = LoadState::Ready;
			s.selected = Some("chief".into());
			let snapshot = s.snapshot.as_mut().unwrap();
			snapshot.runtime_source = Some(EntityId::new("runtime-a").unwrap());
			snapshot.work_items.iter_mut().find(|w| w.id == "chief").unwrap().codex_thread_id =
				Some("thread-a".into());
			s.question_notices.begin(s.question_notice_scope("chief"));
			s.observe_question_notices(&history(vec![]));
			s.observe_question_notices(&history(vec![question("live", true)]));
			assert!(s.question_arrival_notice().is_some());
			s.selected = Some("other".into());
			assert!(s.question_arrival_notice().is_none());
			s.selected = Some("chief".into());
			s.snapshot.as_mut().unwrap().runtime_source = Some(EntityId::new("runtime-b").unwrap());
			assert!(s.question_arrival_notice().is_none());
			s.question_notices.begin(s.question_notice_scope("chief"));
			s.observe_question_notices(&history(vec![question("reconnected", true)]));
			assert!(s.question_arrival_notice().is_none());
			s.observe_question_notices(&history(vec![question("fresh", true)]));
			assert!(s.question_arrival_notice().is_some());
			s.mark_stale(cx);
			assert!(s.question_arrival_notice().is_none());
		});
	}

	#[test]
	fn live_arrivals_exclude_initial_history_duplicates_and_resolved_questions() {
		let mut notices = QuestionNotices::default();
		notices.begin(Some("scope".into()));
		notices.observe(&history(vec![question("old", true)]), true);
		assert!(notices.notice().is_none());
		let current = history(vec![
			question("old", true),
			question("recovered", false),
			question("new", true),
		]);
		notices.observe(&current, true);
		let first = notices.notice().unwrap();
		assert_eq!(first.1, "Same title");
		notices.observe(&current, true);
		assert_eq!(notices.notice().unwrap(), first);
		notices.observe(&history(vec![question("other", true)]), true);
		let second = notices.notice().unwrap();
		assert_eq!(second.1, first.1);
		assert_ne!(second.0, first.0, "dismissal uses question identity, not its title");
		notices.observe(&history(vec![]), true);
		assert!(notices.notice().is_none());
		notices.observe(&history(vec![question("new", true)]), true);
		assert!(notices.notice().is_none(), "reverted known question is not a new arrival");
	}
	#[test]
	fn reconnect_recovery_navigation_and_disabled_notifications_seed_a_baseline() {
		let mut notices = QuestionNotices::default();
		notices.begin(Some("first".into()));
		notices.observe(&history(vec![]), true);
		notices.observe(&history(vec![question("disabled", true)]), false);
		notices.observe(&history(vec![question("disabled", true)]), true);
		assert!(notices.notice().is_none());
		notices.observe(&ChiefHistoryResult::Unavailable, true);
		notices.observe(&history(vec![question("offline", true)]), true);
		assert!(notices.notice().is_none());
		notices.begin(Some("second".into()));
		notices.observe(&history(vec![question("elsewhere", true)]), true);
		assert!(notices.notice().is_none());
		let mut recovery = history(vec![]);
		if let ChiefHistoryResult::Available { questions_recovering, .. } = &mut recovery {
			*questions_recovering = true;
		}
		notices.observe(&recovery, true);
		notices.observe(&history(vec![question("rebuilt", true)]), true);
		assert!(notices.notice().is_none());
		notices.observe(&history(vec![question("a", true), question("b", true)]), true);
		assert_eq!(notices.notice().unwrap().1, "2 questions requested");
	}
	#[test]
	fn incomplete_history_and_replaced_questions_cannot_keep_old_notices() {
		let mut notices = QuestionNotices::default();
		notices.begin(Some("scope".into()));
		notices.observe(&history(vec![]), true);
		notices.observe(&history(vec![question("live", true)]), true);
		assert!(notices.notice().is_some());
		let mut replacement = question("live", false);
		replacement.title = "Replaced".into();
		notices.observe(&history(vec![replacement]), true);
		assert!(notices.notice().is_none());
		let mut incomplete = history(vec![]);
		if let ChiefHistoryResult::Available { questions_truncated, .. } = &mut incomplete {
			*questions_truncated = true;
		}
		notices.observe(&incomplete, true);
		notices.observe(&history(vec![question("outside-page", true)]), true);
		assert!(notices.notice().is_none());
	}

	#[test]
	fn resolving_part_of_a_batch_does_not_make_a_dismissed_notice_new() {
		let mut notices = QuestionNotices::default();
		notices.begin(Some("scope".into()));
		notices.observe(&history(vec![]), true);
		notices.observe(&history(vec![question("a", true), question("b", true)]), true);
		let original = notices.notice().unwrap().0;
		notices.observe(&history(vec![question("b", true)]), true);
		assert_eq!(notices.notice().unwrap().0, original);
		notices.observe(&history(vec![question("b", true), question("c", true)]), true);
		assert_ne!(notices.notice().unwrap().0, original);
	}

	#[test]
	fn titles_keep_complete_graphemes() {
		let mut notices = QuestionNotices::default();
		notices.begin(Some("scope".into()));
		notices.observe(&history(vec![]), true);
		let mut q = question("new", true);
		q.title = "👨‍👩‍👧‍👦".repeat(31);
		notices.observe(&history(vec![q]), true);
		assert_eq!(notices.notice().unwrap().1, format!("{}…", "👨‍👩‍👧‍👦".repeat(30)));
	}
}
