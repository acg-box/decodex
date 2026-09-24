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
		setup.reasoning_effort.as_str(),
		setup.working_directory.chars().take(120).collect::<String>()
	)
}
