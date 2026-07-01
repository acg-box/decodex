use serde::Deserialize;

use crate::{
	config::{
		ProjectAutonomyConfig, ProjectCodexConfig, ProjectGitHubConfig, ProjectPathsConfig,
		ProjectPrivacyClassifierConfig, ProjectTrackerConfig, validation,
	},
	prelude::Result,
};

#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ServiceConfigDocument {
	pub(super) service_id: String,
	pub(super) tracker: ProjectTrackerConfig,
	pub(super) github: ProjectGitHubConfig,
	#[serde(default)]
	pub(super) codex: ProjectCodexConfig,
	#[serde(default)]
	pub(super) autonomy: ProjectAutonomyConfig,
	#[serde(default)]
	pub(super) privacy_classifier: ProjectPrivacyClassifierConfig,
	#[serde(default)]
	pub(super) paths: ProjectPathsConfig,
}
impl ServiceConfigDocument {
	pub(super) fn validate(&self) -> Result<()> {
		validation::validate_service_id("service_id", &self.service_id)?;

		self.tracker.validate()?;
		self.github.validate()?;
		self.codex.validate()?;
		self.autonomy.validate()?;
		self.privacy_classifier.validate()?;
		self.paths.validate()?;

		Ok(())
	}
}
