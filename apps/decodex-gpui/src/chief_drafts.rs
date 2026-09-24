//! Keep editable input and uncertain submission state with its exact service profile.
use super::*;
use std::{collections::BTreeMap, mem};

#[derive(Default)]
pub(super) struct Profiles {
	pub(super) execution: execution_intent::Intents,
	pub(super) texts: BTreeMap<String, String>,
	pub(super) files: BTreeMap<String, Vec<decodex_protocol::ChiefAttachmentDto>>,
	pub(super) tasks: BTreeMap<String, Vec<decodex_protocol::ChiefTaskReferenceDto>>,
	active: Option<ClientProfile>,
	saved: Vec<(ClientProfile, Drafts)>,
}

#[derive(Default)]
struct Drafts {
	execution: execution_intent::Intents,
	restored_questions: Vec<decodex_protocol::DesktopQuestionDraft>,
	question_inputs: BTreeMap<(String, String), Entity<ComposerInput>>,
	question_choices: BTreeMap<(String, String), async_questions::ChoiceDraft>,
	question_threads: BTreeMap<String, String>,
	collapsed_questions: std::collections::BTreeSet<String>,
	text: String,
	manager: Option<String>,
	attachments: Vec<decodex_protocol::ChiefAttachmentDto>,
	references: Vec<decodex_protocol::ChiefTaskReferenceDto>,
	texts: BTreeMap<String, String>,
	files: BTreeMap<String, Vec<decodex_protocol::ChiefAttachmentDto>>,
	tasks: BTreeMap<String, Vec<decodex_protocol::ChiefTaskReferenceDto>>,
	uncertain: bool,
	feedback: String,
}

impl ChiefSurface {
	pub(super) fn draft_owner_available(&self) -> bool {
		self.composer_manager.as_ref().is_none_or(|owner| {
			self.snapshot.as_ref().is_some_and(|snapshot| {
				snapshot.work_items.iter().any(|work| {
					&work.id == owner
						&& (work.parent_goal_id.is_none()
							|| work.kind == decodex_protocol::ChiefWorkKindDto::Manager)
				})
			})
		})
	}

	pub(super) fn bind_drafts(&mut self, profile: Option<&ClientProfile>, cx: &mut Context<Self>) {
		// Disconnects preserve the last owner's input. An initial profile adopts the
		// local draft seeded before a service was selected.
		let Some(profile) = profile else { return };
		let Some(previous) = self.draft_profiles.active.replace(profile.clone()) else { return };
		if previous == *profile {
			return;
		}
		let saved = Drafts {
			execution: mem::take(&mut self.draft_profiles.execution),
			restored_questions: mem::take(&mut self.restored_question_drafts),
			question_inputs: mem::take(&mut self.async_question_inputs),
			question_choices: mem::take(&mut self.async_question_choices),
			question_threads: mem::take(&mut self.async_question_threads),
			collapsed_questions: mem::take(&mut self.collapsed_async_questions),
			text: self.composer.read(cx).content().into(),
			manager: mem::take(&mut self.composer_manager),
			attachments: mem::take(&mut self.attachments),
			references: mem::take(&mut self.task_references),
			texts: mem::take(&mut self.draft_profiles.texts),
			files: mem::take(&mut self.draft_profiles.files),
			tasks: mem::take(&mut self.draft_profiles.tasks),
			uncertain: self.uncertain,
			feedback: if self.uncertain { self.feedback.clone() } else { String::new() },
		};
		let restored = self
			.draft_profiles
			.saved
			.iter()
			.position(|(owner, _)| owner == profile)
			.map(|index| self.draft_profiles.saved.remove(index).1)
			.unwrap_or_default();
		self.draft_profiles.saved.push((previous, saved));
		self.composer.update(cx, |input, cx| input.set_content(&restored.text, cx));
		self.draft_profiles.execution = restored.execution;
		self.restored_question_drafts = restored.restored_questions;
		self.async_question_inputs = restored.question_inputs;
		self.async_question_choices = restored.question_choices;
		self.async_question_threads = restored.question_threads;
		self.collapsed_async_questions = restored.collapsed_questions;
		self.composer_manager = restored.manager;
		self.attachments = restored.attachments;
		self.task_references = restored.references;
		self.draft_profiles.texts = restored.texts;
		self.draft_profiles.files = restored.files;
		self.draft_profiles.tasks = restored.tasks;
		self.uncertain = restored.uncertain;
		self.feedback = restored.feedback;
		self.composer_menu = None;
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::os::unix::fs::{MetadataExt, PermissionsExt};

	#[gpui::test]
	fn profile_round_trip_restores_drafts_without_cross_service_input(
		cx: &mut gpui::TestAppContext,
	) {
		let root = tempfile::tempdir_in("/tmp").unwrap();
		let path = root.path().canonicalize().unwrap();
		std::fs::create_dir(path.join("server")).unwrap();
		std::fs::set_permissions(path.join("server"), std::fs::Permissions::from_mode(0o700))
			.unwrap();
		let uid = std::fs::metadata(&path).unwrap().uid();
		let config = path.join("config.toml");
		std::fs::write(&config, format!("version = 1\nactive_profile = \"local\"\ncache = {{}}\n[profiles.local]\nkind = \"local\"\npolicy = \"same_uid\"\nservice_owner_uid = {uid}\nexpected_server_identity = \"018f0f9e-7b6e-4a31-8f4c-1d2e3f405162\"\n")).unwrap();
		std::fs::set_permissions(config, std::fs::Permissions::from_mode(0o600)).unwrap();
		let first = ClientProfile::load(&path, None).unwrap();
		let second = first.clone().with_expected_server_id(
			decodex_protocol::ServerId::new("018f0f9e-7b6e-4a31-8f4c-1d2e3f405163").unwrap(),
		);
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		surface.update(visual, |s, cx| {
			s.composer.update(cx, |input, cx| input.set_content("seed", cx));
			s.bind_profile(Some(first.clone()), cx);
			assert_eq!(s.composer.read(cx).content(), "seed");
			s.composer_manager = Some("root".into());
			let file = decodex_protocol::ChiefAttachmentDto {
				path: ConversationWorkingDirectory::new("/tmp/first.png").unwrap(),
				image: true,
			};
			let reference = decodex_protocol::ChiefTaskReferenceDto {
				work_id: EntityId::new("task").unwrap(),
				thread_id: WireText::new("thread").unwrap(),
				title: WireText::new("First service task").unwrap(),
			};
			s.attachments = vec![file.clone()];
			s.task_references = vec![reference.clone()];
			s.draft_profiles.texts.insert("manager".into(), "manager draft".into());
			s.draft_profiles.files.insert("manager".into(), vec![file.clone()]);
			s.draft_profiles.tasks.insert("manager".into(), vec![reference.clone()]);
			s.sending = true;
			s.bind_profile(None, cx);
			assert!(s.uncertain);
			s.composer.update(cx, |input, cx| input.set_content("edited offline", cx));
			s.bind_profile(Some(first.clone()), cx);
			assert_eq!(s.composer.read(cx).content(), "edited offline");
			assert_eq!(s.attachments, vec![file.clone()]);
			assert!(s.uncertain);
			s.bind_profile(Some(second.clone()), cx);
			assert_eq!(s.composer.read(cx).content(), "");
			assert!(s.attachments.is_empty() && s.task_references.is_empty());
			assert!(s.draft_profiles.texts.is_empty() && s.draft_profiles.files.is_empty());
			assert!(s.draft_profiles.tasks.is_empty());
			assert!(!s.uncertain);
			s.composer_manager = Some("root".into());
			s.composer.update(cx, |input, cx| input.set_content("second draft", cx));
			s.bind_profile(Some(first), cx);
			assert_eq!(s.composer.read(cx).content(), "edited offline");
			assert_eq!(s.selected.as_deref(), Some("root"));
			assert!(!s.draft_owner_available(), "missing owner cannot retarget the restored draft");
			assert_eq!(s.composer_manager.as_deref(), Some("root"));
			assert_eq!(s.attachments, vec![file.clone()]);
			assert_eq!(s.task_references, vec![reference.clone()]);
			assert_eq!(s.draft_profiles.texts["manager"], "manager draft");
			assert_eq!(s.draft_profiles.files["manager"], vec![file]);
			assert_eq!(s.draft_profiles.tasks["manager"], vec![reference]);
			assert!(s.uncertain && s.feedback.contains("acceptance"));
			assert!(s.command_task.is_none() && !s.sending);
			s.bind_profile(Some(second), cx);
			assert_eq!(s.composer.read(cx).content(), "second draft");
			assert!(s.command_task.is_none() && !s.uncertain);
		});
	}
	pub(super) fn profiles() -> (tempfile::TempDir, ClientProfile, ClientProfile) {
		let root = tempfile::tempdir_in("/tmp").unwrap();
		let path = root.path().canonicalize().unwrap();
		std::fs::create_dir(path.join("server")).unwrap();
		std::fs::set_permissions(path.join("server"), std::fs::Permissions::from_mode(0o700))
			.unwrap();
		let uid = std::fs::metadata(&path).unwrap().uid();
		let config = path.join("config.toml");
		std::fs::write(&config, format!("version = 1\nactive_profile = \"local\"\ncache = {{}}\n[profiles.local]\nkind = \"local\"\npolicy = \"same_uid\"\nservice_owner_uid = {uid}\nexpected_server_identity = \"018f0f9e-7b6e-4a31-8f4c-1d2e3f405162\"\n")).unwrap();
		std::fs::set_permissions(config, std::fs::Permissions::from_mode(0o600)).unwrap();
		let first = ClientProfile::load(&path, None).unwrap();
		let second = first.clone().with_expected_server_id(
			decodex_protocol::ServerId::new("018f0f9e-7b6e-4a31-8f4c-1d2e3f405163").unwrap(),
		);
		(root, first, second)
	}

	#[gpui::test]
	fn async_editors_survive_disconnect_and_profile_round_trip(cx: &mut gpui::TestAppContext) {
		let (_root, first, second) = profiles();
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		surface.update(visual, |s, cx| {
			let key = ("work".into(), "question".into());
			s.bind_profile(Some(first.clone()), cx);
			let input = cx.new(|cx| ComposerInput::with_placeholder(40, "Answer", "Answer", cx));
			input.update(cx, |input, cx| input.set_content("First service answer", cx));
			s.async_question_inputs.insert(key.clone(), input.clone());
			s.async_question_choices.insert(key.clone(), Default::default());
			s.async_question_threads.insert("work".into(), "first-thread".into());
			s.collapsed_async_questions.insert("work".into());
			s.bind_profile(None, cx);
			assert_eq!(s.async_question_inputs[&key], input);
			input.update(cx, |input, cx| input.set_content("Edited offline", cx));
			s.bind_profile(Some(first.clone()), cx);
			assert_eq!(s.async_question_inputs[&key], input);
			s.bind_profile(Some(second.clone()), cx);
			assert!(s.async_question_inputs.is_empty());
			assert!(s.async_question_choices.is_empty());
			assert!(s.async_question_threads.is_empty());
			assert!(s.collapsed_async_questions.is_empty());
			let other = cx.new(|cx| ComposerInput::with_placeholder(40, "Answer", "Answer", cx));
			other.update(cx, |input, cx| input.set_content("Second service answer", cx));
			s.async_question_inputs.insert(key.clone(), other.clone());
			s.async_question_threads.insert("work".into(), "second-thread".into());
			s.bind_profile(Some(first), cx);
			assert_eq!(s.async_question_inputs[&key], input);
			assert_eq!(input.read(cx).content(), "Edited offline");
			assert!(s.async_question_choices.contains_key(&key));
			assert_eq!(s.async_question_threads["work"], "first-thread");
			assert!(s.collapsed_async_questions.contains("work"));
			assert!(s.command_task.is_none() && !s.sending);
			s.bind_profile(Some(second), cx);
			assert_eq!(s.async_question_inputs[&key], other);
			assert_eq!(other.read(cx).content(), "Second service answer");
			assert_eq!(s.async_question_threads["work"], "second-thread");
		});
	}
}
