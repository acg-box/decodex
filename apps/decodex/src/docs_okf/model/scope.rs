#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DocsCheckScope {
	/// Run every OKF docs check.
	All,
	/// Validate routing/index files, JSON absence, and concept frontmatter.
	Index,
	/// Validate local Markdown links.
	Links,
	/// Validate semantic-drift routing files.
	Drift,
}
impl DocsCheckScope {
	/// Return the CLI/report label for this check scope.
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::All => "all",
			Self::Index => "index",
			Self::Links => "links",
			Self::Drift => "drift",
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OkfCheckProfile {
	/// Validate only the portable OKF v0.1 conformance surface.
	Core,
	/// Validate OKF plus agent navigation and graph quality.
	Wiki,
	/// Validate wiki quality plus repository-memory anchors.
	RepoMemory,
	/// Validate the strict Decodex docs profile.
	Decodex,
}
impl OkfCheckProfile {
	/// Return the CLI/report label for this profile.
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::Core => "core",
			Self::Wiki => "wiki",
			Self::RepoMemory => "repo-memory",
			Self::Decodex => "decodex",
		}
	}
}
