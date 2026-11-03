//! Shared status vocabulary for component summaries. Instructions belong in details.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ComponentStatus {
	Checking,
	Installed,
	Enabled,
	Disabled,
	Connected,
	Disconnected,
	NeedsSetup,
	NotInstalled,
	Working,
	Error,
}

impl ComponentStatus {
	pub(crate) fn label(self) -> &'static str {
		match self {
			Self::Checking => "Checking",
			Self::Installed => "Installed",
			Self::Enabled => "Enabled",
			Self::Disabled => "Disabled",
			Self::Connected => "Connected",
			Self::Disconnected => "Not connected",
			Self::NeedsSetup => "Needs setup",
			Self::NotInstalled => "Not installed",
			Self::Working => "Working",
			Self::Error => "Error",
		}
	}
}
