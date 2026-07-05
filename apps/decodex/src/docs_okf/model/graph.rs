use serde::Serialize;

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
