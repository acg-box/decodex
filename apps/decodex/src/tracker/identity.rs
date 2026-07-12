use std::{fmt, str::FromStr};

use crate::prelude::{Result, eyre};

/// Immutable tracker provider identity used before project routing.
#[derive(
	Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, serde::Deserialize, serde::Serialize,
)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TrackerProvider {
	Linear,
}
impl TrackerProvider {
	pub(crate) const fn as_str(self) -> &'static str {
		match self {
			Self::Linear => "linear",
		}
	}
}
impl FromStr for TrackerProvider {
	type Err = eyre::Error;

	fn from_str(value: &str) -> Result<Self> {
		match value {
			"linear" => Ok(Self::Linear),
			_ => eyre::bail!("Unsupported tracker provider `{value}`."),
		}
	}
}

/// Globally stable issue identity. Mutable team and display identifiers are projections.
#[derive(
	Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, serde::Deserialize, serde::Serialize,
)]
pub(crate) struct TrackerIssueKey {
	provider: TrackerProvider,
	workspace_id: String,
	immutable_issue_id: String,
}
impl TrackerIssueKey {
	pub(crate) fn new(
		provider: TrackerProvider,
		workspace_id: &str,
		immutable_issue_id: &str,
	) -> Result<Self> {
		if workspace_id.trim().is_empty() || immutable_issue_id.trim().is_empty() {
			eyre::bail!("Tracker issue identity requires workspace and immutable issue ids.");
		}

		Ok(Self {
			provider,
			workspace_id: workspace_id.to_owned(),
			immutable_issue_id: immutable_issue_id.to_owned(),
		})
	}

	pub(crate) const fn provider(&self) -> TrackerProvider {
		self.provider
	}

	pub(crate) fn workspace_id(&self) -> &str {
		&self.workspace_id
	}

	pub(crate) fn immutable_issue_id(&self) -> &str {
		&self.immutable_issue_id
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum WorkspaceIssueSelector {
	ImmutableId(String),
	Identifier(String),
}

/// Project-independent issue selector accepted by v2 issue resolution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct WorkspaceQualifiedIssueReference {
	provider: TrackerProvider,
	workspace_id: String,
	selector: WorkspaceIssueSelector,
}
impl WorkspaceQualifiedIssueReference {
	pub(crate) const fn provider(&self) -> TrackerProvider {
		self.provider
	}

	pub(crate) fn workspace_id(&self) -> &str {
		&self.workspace_id
	}

	pub(crate) fn selector(&self) -> &WorkspaceIssueSelector {
		&self.selector
	}
}
impl FromStr for WorkspaceQualifiedIssueReference {
	type Err = eyre::Error;

	fn from_str(value: &str) -> Result<Self> {
		let mut parts = value.splitn(4, ':');
		let provider = parts.next().unwrap_or_default().parse()?;
		let workspace_id = parts.next().unwrap_or_default();
		let selector_kind = parts.next().unwrap_or_default();
		let selector_value = parts.next().unwrap_or_default();

		if workspace_id.is_empty() || selector_value.is_empty() {
			eyre::bail!(
				"Issue reference must be workspace-qualified as provider:workspace:id:value or provider:workspace:identifier:value."
			);
		}
		let selector = match selector_kind {
			"id" => WorkspaceIssueSelector::ImmutableId(selector_value.to_owned()),
			"identifier" => WorkspaceIssueSelector::Identifier(selector_value.to_owned()),
			_ => eyre::bail!("Issue reference must declare `id` or `identifier` selector kind."),
		};

		Ok(Self { provider, workspace_id: workspace_id.to_owned(), selector })
	}
}
impl fmt::Display for WorkspaceQualifiedIssueReference {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		let (kind, value) = match &self.selector {
			WorkspaceIssueSelector::ImmutableId(value) => ("id", value),
			WorkspaceIssueSelector::Identifier(value) => ("identifier", value),
		};
		write!(formatter, "{}:{}:{kind}:{value}", self.provider.as_str(), self.workspace_id)
	}
}

#[cfg(test)]
mod tests {
	use super::{TrackerProvider, WorkspaceIssueSelector, WorkspaceQualifiedIssueReference};

	#[test]
	fn qualified_reference_requires_workspace_and_explicit_selector_kind() {
		for invalid in
			["PUB-1711", "linear:PUB-1711", "linear::identifier:PUB-1711", "linear:ws:PUB-1711"]
		{
			assert!(invalid.parse::<WorkspaceQualifiedIssueReference>().is_err(), "{invalid}");
		}
	}

	#[test]
	fn qualified_reference_keeps_resolution_independent_of_project() {
		let reference = "linear:workspace-1:identifier:PUB-1711"
			.parse::<WorkspaceQualifiedIssueReference>()
			.expect("qualified reference");

		assert_eq!(reference.provider(), TrackerProvider::Linear);
		assert_eq!(reference.workspace_id(), "workspace-1");
		assert_eq!(
			reference.selector(),
			&WorkspaceIssueSelector::Identifier("PUB-1711".to_owned())
		);
		assert_eq!(reference.to_string(), "linear:workspace-1:identifier:PUB-1711");
	}
}
