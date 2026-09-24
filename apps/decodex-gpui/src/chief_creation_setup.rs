//! Preserve pre-creation input independently of existing task execution overrides.
use super::*;
use decodex_protocol::DesktopCreationSetup;
pub(super) const DEFAULT_MODEL: &str = "gpt-6-astra";

impl ChiefSurface {
	pub(super) fn creation_setup(&self, cx: &Context<Self>) -> Option<DesktopCreationSetup> {
		if self.composer_manager.is_some() || self.root_id().is_some() {
			return None;
		}
		let setup = DesktopCreationSetup {
			defaults_applied: self.creation_defaults_applied,
			intent: Some(self.creation_intent.clone()),
			inherit_effort: self.creation_inherit_effort,
			model: self.model.read(cx).content().into(),
			working_directory: self.cwd.read(cx).content().into(),
			account: self.account.read(cx).content().into(),
			reasoning_effort: self.effort.clone(),
			fast: self.fast,
			service_tier: self.service_tier.clone(),
			sandbox: self.sandbox,
		};
		(self.creation_setup_present || setup != empty_setup()).then_some(setup)
	}

	pub(super) fn restore_creation_setup(
		&mut self,
		setup: Option<&DesktopCreationSetup>,
		cx: &mut Context<Self>,
	) {
		self.creation_setup_present = setup.is_some();
		let empty = empty_setup();
		let setup = setup.unwrap_or(&empty);
		self.model.update(cx, |input, cx| input.set_content(&setup.model, cx));
		self.cwd.update(cx, |input, cx| input.set_content(&setup.working_directory, cx));
		self.account.update(cx, |input, cx| input.set_content(&setup.account, cx));
		self.effort = setup.reasoning_effort.clone();
		self.creation_inherit_effort = setup.inherit_effort;
		self.creation_intent =
			setup.intent.clone().unwrap_or(decodex_protocol::DesktopCreationIntent {
				model: true,
				reasoning: true,
				service_tier: true,
			});
		self.creation_defaults = None;
		self.creation_defaults_applied = setup.defaults_applied;
		self.fast = setup.fast;
		self.service_tier = setup.service_tier.clone();
		self.sandbox = setup.sandbox;
		// Discovery from a different setup must never be treated as current after restoration.
		self.capabilities = None;
		self.capabilities_context = None;
		self.capabilities_checked = None;
	}
}
fn empty_setup() -> DesktopCreationSetup {
	DesktopCreationSetup {
		defaults_applied: false,
		intent: Some(Default::default()),
		inherit_effort: false,
		model: DEFAULT_MODEL.into(),
		working_directory: String::new(),
		account: String::new(),
		reasoning_effort: ConversationReasoningEffort::High,
		fast: false,
		service_tier: None,
		sandbox: ChiefSandboxDto::ReadOnly,
	}
}

pub(super) fn summary(setup: &DesktopCreationSetup) -> String {
	format!(
		"New task · {} · {} · {}",
		setup.model.chars().take(80).collect::<String>(),
		if setup.inherit_effort { "Inherited" } else { setup.reasoning_effort.as_str() },
		setup.working_directory.chars().take(120).collect::<String>()
	)
}

impl ChiefSurface {
	pub(super) fn creation_effort(&self) -> Option<ConversationReasoningEffort> {
		(!self.creation_inherit_effort).then(|| self.effort.clone())
	}

	pub(super) fn creation_effort_toggle(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
		if self.composer_manager.is_some() || self.root_id().is_some() {
			return div().into_any_element();
		}
		let label = if self.creation_inherit_effort {
			"Use explicit reasoning"
		} else {
			"Use native reasoning"
		};
		div()
			.id("creation-native-effort")
			.debug_selector(|| "creation-native-effort".into())
			.role(Role::Button)
			.tab_index(0)
			.aria_label(label)
			.cursor_pointer()
			.px_2()
			.py_1()
			.child(label)
			.on_click(cx.listener(|s, _, _, cx| s.toggle_creation_effort(cx)))
			.on_key_down(cx.listener(|s, event: &gpui::KeyDownEvent, _, cx| {
				if !event.is_held && matches!(event.keystroke.key.as_str(), "enter" | "space") {
					s.toggle_creation_effort(cx);
					cx.stop_propagation();
				}
			}))
			.into_any_element()
	}

	fn toggle_creation_effort(&mut self, cx: &mut Context<Self>) {
		if self.composer_manager.is_some() || self.root_id().is_some() {
			return;
		}
		self.creation_inherit_effort = !self.creation_inherit_effort;
		self.creation_intent.reasoning = true;
		self.apply_creation_defaults(cx);
		self.creation_setup_present = true;
		self.save_draft_document(cx);
		cx.notify();
	}
}
