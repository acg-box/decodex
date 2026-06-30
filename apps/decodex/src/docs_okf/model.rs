//! Data types and constants for OKF docs checks.

use std::path::PathBuf;

use serde::Serialize;

pub(super) const REQUIRED_CONCEPT_KEYS: &[&str] =
	&["type", "title", "description", "status", "authority", "owner", "last_verified"];
pub(super) const REQUIRED_DOCS_FILES: &[&str] = &[
	"index.md",
	"log.md",
	"policy.md",
	"decisions/index.md",
	"evidence/index.md",
	"reference/index.md",
	"research/index.md",
	"runbook/index.md",
	"spec/index.md",
];
pub(super) const ALLOWED_CONCEPT_TYPES: &[&str] = &[
	"Decision",
	"Drift Audit",
	"Evidence",
	"Policy",
	"Reference",
	"Research Contract",
	"Runbook",
	"Spec",
];
pub(super) const ALLOWED_STATUSES: &[&str] = &["draft", "active", "deprecated", "superseded"];
pub(super) const ALLOWED_AUTHORITIES: &[&str] =
	&["normative", "procedural", "current_state", "rationale", "evidence", "non_authoritative"];
pub(super) const ALLOWED_PROMOTION_TARGETS: &[&str] =
	&["docs/spec", "docs/runbook", "docs/reference", "docs/decisions", "docs/evidence"];
pub(super) const RESEARCH_CONTRACT_HEADINGS: &[&str] = &[
	"Question",
	"Scope",
	"Evidence",
	"Options",
	"Judgment",
	"Challenge",
	"Decision",
	"Promotion",
	"Drift Impact",
	"Citations",
];
pub(super) const DRIFT_AUDIT_HEADINGS: &[&str] = &[
	"Watched Claims",
	"Evidence Anchors",
	"Reverse Checks",
	"Verdict",
	"Required Updates",
	"Citations",
];

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

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct DocsCheckReport {
	pub(super) scope: DocsCheckScope,
	pub(super) docs_root: PathBuf,
	pub(super) concept_count: usize,
	pub(super) link_count: usize,
	pub(super) issues: Vec<DocsCheckIssue>,
}
impl DocsCheckReport {
	/// Return whether the check found at least one docs issue.
	pub(crate) fn has_issues(&self) -> bool {
		!self.issues.is_empty()
	}
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct OkfCheckReport {
	pub(super) profile: OkfCheckProfile,
	pub(super) bundle_root: PathBuf,
	pub(super) concept_count: usize,
	pub(super) link_count: usize,
	pub(super) issues: Vec<DocsCheckIssue>,
}
impl OkfCheckReport {
	/// Return whether the check found at least one OKF issue.
	pub(crate) fn has_issues(&self) -> bool {
		!self.issues.is_empty()
	}

	/// Return the profile used for this OKF check.
	pub(crate) fn profile(&self) -> OkfCheckProfile {
		self.profile
	}
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct OkfInitReport {
	pub(super) profile: OkfCheckProfile,
	pub(super) bundle_root: PathBuf,
	pub(super) created: Vec<PathBuf>,
	pub(super) unchanged: Vec<PathBuf>,
}
impl OkfInitReport {
	/// Return the profile used for this OKF init.
	pub(crate) fn profile(&self) -> OkfCheckProfile {
		self.profile
	}
}

#[derive(Debug, Default)]
pub(crate) struct OkfQuery {
	/// Match a concept `type` value.
	pub(crate) concept_type: Option<String>,
	/// Match one or more exact tag values.
	pub(crate) tags: Vec<String>,
	/// Match a substring in the `resource` field.
	pub(crate) resource: Option<String>,
	/// Match a substring in `source_refs`.
	pub(crate) source_ref: Option<String>,
	/// Match a substring in `code_refs`.
	pub(crate) code_ref: Option<String>,
	/// Match a substring in `related`.
	pub(crate) related: Option<String>,
	/// Match a substring in concept path, title, or description.
	pub(crate) text: Option<String>,
}

#[derive(Debug, Eq, PartialEq, Serialize)]
pub(crate) struct OkfConceptSummary {
	/// Concept id, derived from the bundle-relative file path without `.md`.
	pub(crate) id: String,
	/// Bundle-relative Markdown file path.
	pub(crate) path: String,
	/// Concept type from frontmatter.
	pub(crate) concept_type: String,
	/// Human-readable title, derived from frontmatter or path.
	pub(crate) title: String,
	/// One-line concept summary from frontmatter.
	pub(crate) description: Option<String>,
	/// Resource URI from frontmatter.
	pub(crate) resource: Option<String>,
	/// Tags from frontmatter.
	pub(crate) tags: Vec<String>,
	/// External source references from frontmatter.
	pub(crate) source_refs: Vec<String>,
	/// Repository file references from frontmatter.
	pub(crate) code_refs: Vec<String>,
	/// Related concept references from frontmatter.
	pub(crate) related: Vec<String>,
}

#[derive(Debug, Eq, PartialEq, Serialize)]
pub(crate) struct OkfGraphEdge {
	/// Source concept id.
	pub(crate) source: String,
	/// Target concept id.
	pub(crate) target: String,
	/// Relationship source, such as `markdown` or `related`.
	pub(crate) kind: String,
}

#[derive(Debug, Eq, PartialEq, Serialize)]
pub(crate) struct OkfBrokenLink {
	/// Source concept id.
	pub(crate) source: String,
	/// Unresolved link target.
	pub(crate) target: String,
	/// Relationship source, such as `markdown` or `related`.
	pub(crate) kind: String,
}

#[derive(Debug, Eq, PartialEq, Serialize)]
pub(crate) struct OkfGraph {
	/// Concepts in the bundle.
	pub(crate) concepts: Vec<OkfConceptSummary>,
	/// Resolved graph edges between concepts.
	pub(crate) edges: Vec<OkfGraphEdge>,
	/// Unresolved graph edges.
	pub(crate) broken_links: Vec<OkfBrokenLink>,
	/// Concepts with no inbound or outbound resolved edges.
	pub(crate) orphan_concepts: Vec<String>,
}

#[derive(Debug, Eq, PartialEq)]
pub(super) struct DocsCheckIssue {
	pub(super) path: Option<PathBuf>,
	pub(super) message: String,
}

#[derive(Debug)]
pub(super) struct DocsFile {
	pub(super) path: PathBuf,
	pub(super) relative_path: PathBuf,
	pub(super) content: Option<String>,
	pub(super) read_error: Option<String>,
}

pub(super) struct OkfScaffoldFile {
	pub(super) relative_path: &'static str,
	pub(super) content: &'static str,
}
