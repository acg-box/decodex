//! Preserve pre-creation input independently of existing task execution overrides.
use crate::shell::agent_surface::{AgentSurface, Context, ConversationReasoningEffort};
use decodex_protocol::{DesktopCreationIntent, DesktopCreationSetup};

pub(super) const DEFAULT_MODEL: &str = "gpt-6-astra";

impl AgentSurface {
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
			permission_mode: self.permission_mode,
		};

		let mut baseline = empty_setup();
		if !self.creation_intent.service_tier {
			// A displayed shared default is not a saved user override.
			baseline.fast = setup.fast;
			baseline.service_tier.clone_from(&setup.service_tier);
		}
		(self.creation_setup_present || setup != baseline).then_some(setup)
	}

	pub(super) fn restore_creation_setup(
		&mut self,
		setup: Option<&DesktopCreationSetup>,
		cx: &mut Context<Self>,
	) {
		self.creation_setup_present = setup.is_some();

		let mut empty = empty_setup();
		empty.service_tier = crate::session_settings::default_speed().ok();
		empty.fast = empty.service_tier.as_ref().is_some_and(|tier| tier.as_str() == "priority");
		let setup = setup.unwrap_or(&empty);

		self.model.update(cx, |input, cx| input.set_content(&setup.model, cx));
		self.cwd.update(cx, |input, cx| input.set_content(&setup.working_directory, cx));
		self.account.update(cx, |input, cx| input.set_content(&setup.account, cx));

		self.effort = setup.reasoning_effort.clone();
		self.creation_inherit_effort = setup.inherit_effort;
		self.creation_intent = setup.intent.clone().unwrap_or(DesktopCreationIntent {
			model: true,
			reasoning: true,
			service_tier: true,
		});
		self.creation_defaults = None;
		self.creation_defaults_applied = setup.defaults_applied;
		self.fast = setup.fast;
		self.service_tier = setup.service_tier.clone();
		self.permission_mode = setup.permission_mode;
		// Discovery from a different setup must never be treated as current after restoration.
		self.capabilities = None;
		self.capabilities_context = None;
		self.capabilities_checked = None;
	}
}

impl AgentSurface {
	pub(super) fn creation_effort(&self) -> Option<ConversationReasoningEffort> {
		(!self.creation_inherit_effort).then(|| self.effort.clone())
	}
}

fn empty_setup() -> DesktopCreationSetup {
	let defaults = crate::session_settings::Defaults::load();
	DesktopCreationSetup {
		defaults_applied: false,
		intent: Some(decodex_protocol::DesktopCreationIntent {
			service_tier: false,
			..Default::default()
		}),
		inherit_effort: false,
		model: DEFAULT_MODEL.into(),
		working_directory: String::new(),
		account: String::new(),
		reasoning_effort: ConversationReasoningEffort::High,
		fast: false,
		service_tier: None,
		permission_mode: defaults.permissions,
	}
}
