//! OKF-style documentation validation for the repository docs bundle.

use std::{
	collections::BTreeSet,
	fs,
	io::ErrorKind,
	path::{Component, Path, PathBuf},
};

use regex::Regex;
use reqwest::Url;
use serde::Serialize;
use serde_json::Value as JsonValue;
use serde_yaml::{Mapping, Value};
use time::{Date, Month};

use crate::prelude::Result;

const REQUIRED_CONCEPT_KEYS: &[&str] =
	&["type", "title", "description", "status", "authority", "owner", "last_verified"];
const REQUIRED_DOCS_FILES: &[&str] = &[
	"index.md",
	"log.md",
	"policy.md",
	"decisions/index.md",
	"evidence/index.md",
	"reference/index.md",
	"research/index.json",
	"runbook/index.md",
	"spec/index.md",
];
const RESEARCH_INDEX_SCHEMA: &str = "decodex.research_index/1";
const RESEARCH_REPORT_SCHEMA: &str = "decodex.research_report/1";
const REQUIRED_RESEARCH_REPORT_KEYS: &[&str] =
	&["schema", "title", "purpose", "scope", "status_summary", "evidence_ledger"];
const ALLOWED_CONCEPT_TYPES: &[&str] = &[
	"Decision",
	"Drift Audit",
	"Evidence",
	"Policy",
	"Reference",
	"Research Contract",
	"Runbook",
	"Spec",
];
const ALLOWED_STATUSES: &[&str] = &["draft", "active", "deprecated", "superseded"];
const ALLOWED_AUTHORITIES: &[&str] =
	&["normative", "procedural", "current_state", "rationale", "evidence", "non_authoritative"];
const ALLOWED_PROMOTION_TARGETS: &[&str] =
	&["docs/spec", "docs/runbook", "docs/reference", "docs/decisions"];
const RESEARCH_CONTRACT_HEADINGS: &[&str] = &[
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
const DRIFT_AUDIT_HEADINGS: &[&str] = &[
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
	/// Validate routing/index files, artifact placement, and concept frontmatter.
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
	scope: DocsCheckScope,
	docs_root: PathBuf,
	concept_count: usize,
	link_count: usize,
	issues: Vec<DocsCheckIssue>,
}
impl DocsCheckReport {
	/// Return whether the check found at least one docs issue.
	pub(crate) fn has_issues(&self) -> bool {
		!self.issues.is_empty()
	}
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct OkfCheckReport {
	profile: OkfCheckProfile,
	bundle_root: PathBuf,
	concept_count: usize,
	link_count: usize,
	issues: Vec<DocsCheckIssue>,
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
	profile: OkfCheckProfile,
	bundle_root: PathBuf,
	created: Vec<PathBuf>,
	unchanged: Vec<PathBuf>,
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
	/// Retrieval summary from frontmatter.
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
pub(crate) struct OkfRouteMatch {
	/// Matching concept summary.
	pub(crate) concept: OkfConceptSummary,
	/// Simple lexical relevance score.
	pub(crate) score: usize,
}

#[derive(Debug, Eq, PartialEq)]
struct DocsCheckIssue {
	path: Option<PathBuf>,
	message: String,
}

#[derive(Debug)]
struct DocsFile {
	path: PathBuf,
	relative_path: PathBuf,
	content: Option<String>,
	read_error: Option<String>,
}

struct OkfScaffoldFile {
	relative_path: &'static str,
	content: &'static str,
}

/// Initialize a portable OKF bundle with safe, idempotent scaffold files.
pub(crate) fn init_okf_bundle(root: &Path, profile: OkfCheckProfile) -> Result<OkfInitReport> {
	if profile == OkfCheckProfile::Decodex {
		color_eyre::eyre::bail!(
			"`decodex okf init` scaffolds portable profiles only; use Decodex docs policy for the `decodex` profile."
		);
	}

	let files = okf_scaffold_files(profile);

	ensure_scaffold_targets_available(root, &files)?;

	fs::create_dir_all(root)?;

	let mut report = OkfInitReport {
		profile,
		bundle_root: root.to_path_buf(),
		created: Vec::new(),
		unchanged: Vec::new(),
	};

	for file in files {
		write_scaffold_file(root, file.relative_path, file.content, &mut report)?;
	}

	Ok(report)
}

/// Render a stable human-readable OKF init report.
pub(crate) fn render_okf_init_report(report: &OkfInitReport) -> String {
	let mut output = String::new();

	output.push_str(&format!(
		"okf init: profile={} root={} created={} unchanged={}\n",
		report.profile().as_str(),
		report.bundle_root.display(),
		report.created.len(),
		report.unchanged.len()
	));

	for path in &report.created {
		output.push_str(&format!("- created {}\n", path.display()));
	}
	for path in &report.unchanged {
		output.push_str(&format!("- unchanged {}\n", path.display()));
	}

	output.push_str(&format!(
		"next: decodex okf check {} --profile {}\n",
		report.bundle_root.display(),
		report.profile.as_str()
	));

	output
}

/// Validate the Decodex docs bundle.
pub(crate) fn run_docs_check(root: &Path, scope: DocsCheckScope) -> Result<DocsCheckReport> {
	let docs_root = root.to_path_buf();

	if !docs_root.is_dir() {
		color_eyre::eyre::bail!("docs root `{}` does not exist.", docs_root.display());
	}

	let mut files = Vec::new();

	collect_files(&docs_root, &docs_root, &mut files)?;

	let mut report =
		DocsCheckReport { scope, docs_root, concept_count: 0, link_count: 0, issues: Vec::new() };

	if matches!(scope, DocsCheckScope::All | DocsCheckScope::Index | DocsCheckScope::Drift) {
		check_required_docs_layout(&files, &mut report);
	}

	check_markdown_readability(&files, &mut report);

	if matches!(scope, DocsCheckScope::All | DocsCheckScope::Index) {
		check_docs_artifact_types(&files, &mut report);
		check_research_json_artifacts(&files, &mut report);
		check_acronym_capitalization(&files, &mut report);
		check_concept_contracts(&files, &mut report);
	}
	if matches!(scope, DocsCheckScope::All | DocsCheckScope::Links) {
		check_links(&files, &mut report)?;
	}
	if matches!(scope, DocsCheckScope::All | DocsCheckScope::Drift) {
		check_drift_surface(&files, &mut report);
	}

	Ok(report)
}

/// Render a stable human-readable docs check report.
pub(crate) fn render_docs_check_report(report: &DocsCheckReport) -> String {
	let mut output = String::new();

	output.push_str(&format!(
		"docs {} check: concepts={} links={} root={}\n",
		report.scope.as_str(),
		report.concept_count,
		report.link_count,
		report.docs_root.display()
	));

	if report.issues.is_empty() {
		output.push_str("status: pass\n");

		return output;
	}

	output.push_str("status: fail\n");

	for issue in &report.issues {
		match &issue.path {
			Some(path) => output.push_str(&format!("- {}: {}\n", path.display(), issue.message)),
			None => output.push_str(&format!("- {}\n", issue.message)),
		}
	}

	output
}

/// Validate any OKF bundle with the selected profile.
pub(crate) fn run_okf_check(root: &Path, profile: OkfCheckProfile) -> Result<OkfCheckReport> {
	if profile == OkfCheckProfile::Decodex {
		return Ok(decodex_docs_report_as_okf(run_docs_check(root, DocsCheckScope::All)?));
	}

	let bundle_root = root.to_path_buf();

	if !bundle_root.is_dir() {
		color_eyre::eyre::bail!("OKF bundle root `{}` does not exist.", bundle_root.display());
	}

	let mut files = Vec::new();

	collect_files(&bundle_root, &bundle_root, &mut files)?;

	let mut report = OkfCheckReport {
		profile,
		bundle_root,
		concept_count: 0,
		link_count: 0,
		issues: Vec::new(),
	};

	check_okf_markdown_readability(&files, &mut report);
	check_okf_core_concepts(&files, &mut report);

	if matches!(profile, OkfCheckProfile::Wiki | OkfCheckProfile::RepoMemory) {
		check_okf_wiki_surface(&files, &mut report)?;
	}
	if profile == OkfCheckProfile::RepoMemory {
		check_okf_repo_memory_surface(&files, &mut report);
	}

	Ok(report)
}

/// Render a stable human-readable OKF check report.
pub(crate) fn render_okf_check_report(report: &OkfCheckReport) -> String {
	let mut output = String::new();

	output.push_str(&format!(
		"okf {} check: concepts={} links={} root={}\n",
		report.profile.as_str(),
		report.concept_count,
		report.link_count,
		report.bundle_root.display()
	));

	if report.issues.is_empty() {
		output.push_str("status: pass\n");

		return output;
	}

	output.push_str("status: fail\n");

	for issue in &report.issues {
		match &issue.path {
			Some(path) => output.push_str(&format!("- {}: {}\n", path.display(), issue.message)),
			None => output.push_str(&format!("- {}\n", issue.message)),
		}
	}

	output
}

/// Return the concept summaries matching an OKF query.
pub(crate) fn query_okf_bundle(root: &Path, query: &OkfQuery) -> Result<Vec<OkfConceptSummary>> {
	let files = read_okf_files(root)?;
	let mut concepts = Vec::new();

	for file in files.iter().filter(|file| is_concept_markdown(&file.relative_path)) {
		let Some(concept) = concept_summary(file) else {
			continue;
		};

		if okf_query_matches(&concept, query) {
			concepts.push(concept);
		}
	}

	concepts.sort_by(|left, right| left.path.cmp(&right.path));

	Ok(concepts)
}

/// Build an OKF concept graph from Markdown links and `related` frontmatter.
pub(crate) fn build_okf_graph(root: &Path) -> Result<OkfGraph> {
	let files = read_okf_files(root)?;
	let concept_paths = okf_concept_path_set(&files);
	let mut concepts = Vec::new();
	let mut edges = Vec::new();
	let mut broken_links = Vec::new();

	for file in files.iter().filter(|file| is_concept_markdown(&file.relative_path)) {
		let Some(concept) = concept_summary(file) else {
			continue;
		};
		let source = concept.id.clone();

		collect_markdown_graph_edges(
			file,
			root,
			&concept_paths,
			&source,
			&mut edges,
			&mut broken_links,
		)?;
		collect_related_graph_edges(
			file,
			root,
			&concept_paths,
			&source,
			&mut edges,
			&mut broken_links,
		);

		concepts.push(concept);
	}

	let orphan_concepts = okf_orphan_concepts(&concepts, &edges);

	concepts.sort_by(|left, right| left.id.cmp(&right.id));
	edges.sort_by(|left, right| {
		(&left.source, &left.target, &left.kind).cmp(&(&right.source, &right.target, &right.kind))
	});
	broken_links.sort_by(|left, right| {
		(&left.source, &left.target, &left.kind).cmp(&(&right.source, &right.target, &right.kind))
	});

	Ok(OkfGraph { concepts, edges, broken_links, orphan_concepts })
}

/// Render an OKF graph as JSON.
pub(crate) fn render_okf_graph_json(graph: &OkfGraph) -> Result<String> {
	Ok(format!("{}\n", serde_json::to_string_pretty(graph)?))
}

/// Render a compact text graph summary.
pub(crate) fn render_okf_graph_summary(root: &Path, graph: &OkfGraph) -> String {
	format!(
		"okf graph: concepts={} edges={} broken_links={} orphans={} root={}\n",
		graph.concepts.len(),
		graph.edges.len(),
		graph.broken_links.len(),
		graph.orphan_concepts.len(),
		root.display()
	)
}

/// Route an intent to the highest scoring OKF concepts.
pub(crate) fn route_okf_bundle(
	root: &Path,
	intent: &str,
	limit: usize,
) -> Result<Vec<OkfRouteMatch>> {
	let files = read_okf_files(root)?;
	let tokens = route_tokens(intent);
	let mut matches = Vec::new();

	if tokens.is_empty() {
		return Ok(matches);
	}

	for file in files.iter().filter(|file| is_concept_markdown(&file.relative_path)) {
		let Some(concept) = concept_summary(file) else {
			continue;
		};
		let score = route_score(file, &concept, &tokens);

		if score > 0 {
			matches.push(OkfRouteMatch { concept, score });
		}
	}

	matches.sort_by(|left, right| {
		right.score.cmp(&left.score).then_with(|| left.concept.path.cmp(&right.concept.path))
	});
	matches.truncate(limit);

	Ok(matches)
}

fn okf_scaffold_files(profile: OkfCheckProfile) -> Vec<OkfScaffoldFile> {
	vec![
		OkfScaffoldFile {
			relative_path: "index.md",
			content: "# OKF Bundle\n\n- [Overview](overview.md)\n\nUse this index to route agents and humans to the smallest relevant concept.\n",
		},
		OkfScaffoldFile {
			relative_path: "log.md",
			content: "# OKF Log\n\n- Initialized this portable OKF bundle scaffold.\n",
		},
		OkfScaffoldFile { relative_path: "overview.md", content: overview_concept(profile) },
	]
}

fn ensure_scaffold_targets_available(root: &Path, files: &[OkfScaffoldFile]) -> Result<()> {
	for file in files {
		ensure_scaffold_target_available(root, file.relative_path, file.content)?;
	}

	Ok(())
}

fn ensure_scaffold_target_available(root: &Path, relative_path: &str, content: &str) -> Result<()> {
	let path = root.join(relative_path);

	match fs::read_to_string(&path) {
		Ok(existing) if existing == content => Ok(()),
		Ok(_) => reject_divergent_scaffold(&path),
		Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
		Err(error) => Err(error.into()),
	}
}

fn overview_concept(profile: OkfCheckProfile) -> &'static str {
	match profile {
		OkfCheckProfile::Core =>
			"---\ntype: Knowledge Bundle\n---\n\n# OKF Bundle Overview\n\nThis concept introduces the bundle and should be replaced with repository-specific knowledge.\n",
		OkfCheckProfile::Wiki =>
			"---\ntype: Knowledge Bundle\ntitle: OKF Bundle Overview\ndescription: Entry concept for the repository knowledge bundle.\ntags: [okf]\n---\n\n# OKF Bundle Overview\n\nThis concept introduces the bundle and should be replaced with repository-specific knowledge.\n",
		OkfCheckProfile::RepoMemory =>
			"---\ntype: Knowledge Bundle\ntitle: OKF Bundle Overview\ndescription: Entry concept for the repository knowledge bundle.\ntags: [okf, repo-memory]\nsource_refs: []\ncode_refs: []\nrelated: []\ndrift_watch: [decodex okf check, decodex okf route]\n---\n\n# OKF Bundle Overview\n\nThis concept introduces the bundle and should be replaced with repository-specific knowledge.\n",
		OkfCheckProfile::Decodex => unreachable!("decodex profile is rejected before scaffold"),
	}
}

fn write_scaffold_file(
	root: &Path,
	relative_path: &str,
	content: &str,
	report: &mut OkfInitReport,
) -> Result<()> {
	let path = root.join(relative_path);

	if let Some(parent) = path.parent() {
		fs::create_dir_all(parent)?;
	}

	match fs::read_to_string(&path) {
		Ok(existing) if existing == content => {
			report.unchanged.push(PathBuf::from(relative_path));
		},
		Ok(_) => return reject_divergent_scaffold(&path),
		Err(error) if error.kind() == ErrorKind::NotFound => {
			fs::write(&path, content)?;

			report.created.push(PathBuf::from(relative_path));
		},
		Err(error) => return Err(error.into()),
	}

	Ok(())
}

fn reject_divergent_scaffold(path: &Path) -> Result<()> {
	color_eyre::eyre::bail!(
		"OKF scaffold target `{}` already exists with different content; move it or edit the bundle manually.",
		path.display()
	);
}

fn collect_files(root: &Path, dir: &Path, files: &mut Vec<DocsFile>) -> Result<()> {
	for entry in fs::read_dir(dir)? {
		let entry = entry?;
		let path = entry.path();
		let file_type = entry.file_type()?;

		if file_type.is_dir() {
			collect_files(root, &path, files)?;
		} else if file_type.is_file() {
			let relative_path = path.strip_prefix(root)?.to_path_buf();
			let (content, read_error) =
				if path.extension().is_some_and(|extension| extension == "md") {
					match fs::read_to_string(&path) {
						Ok(content) => (Some(content), None),
						Err(error) => (None, Some(error.to_string())),
					}
				} else {
					(None, None)
				};

			files.push(DocsFile { path, relative_path, content, read_error });
		}
	}

	Ok(())
}

fn read_okf_files(root: &Path) -> Result<Vec<DocsFile>> {
	if !root.is_dir() {
		color_eyre::eyre::bail!("OKF bundle root `{}` does not exist.", root.display());
	}

	let mut files = Vec::new();

	collect_files(root, root, &mut files)?;

	Ok(files)
}

fn decodex_docs_report_as_okf(report: DocsCheckReport) -> OkfCheckReport {
	OkfCheckReport {
		profile: OkfCheckProfile::Decodex,
		bundle_root: report.docs_root,
		concept_count: report.concept_count,
		link_count: report.link_count,
		issues: report.issues,
	}
}

fn check_okf_markdown_readability(files: &[DocsFile], report: &mut OkfCheckReport) {
	for file in files {
		if let Some(read_error) = &file.read_error {
			report.issues.push(issue(
				Some(file.relative_path.clone()),
				format!("Markdown file must be UTF-8 readable: {read_error}"),
			));
		}
	}
}

fn check_okf_core_concepts(files: &[DocsFile], report: &mut OkfCheckReport) {
	for file in files.iter().filter(|file| is_concept_markdown(&file.relative_path)) {
		report.concept_count += 1;

		let Some(content) = file.content.as_deref() else {
			continue;
		};
		let Some((frontmatter, _)) = split_yaml_frontmatter(content) else {
			report.issues.push(issue(
				Some(file.relative_path.clone()),
				String::from("concept must start with YAML frontmatter delimited by ---"),
			));

			continue;
		};
		let Some(fields) = parse_okf_frontmatter_mapping(frontmatter, &file.relative_path, report)
		else {
			continue;
		};

		read_required_okf_frontmatter_string(&fields, "type", &file.relative_path, report);
	}
}

fn check_okf_wiki_surface(files: &[DocsFile], report: &mut OkfCheckReport) -> Result<()> {
	check_okf_indexes(files, report);
	check_okf_wiki_frontmatter(files, report);
	check_okf_links(files, report)?;

	Ok(())
}

fn check_okf_indexes(files: &[DocsFile], report: &mut OkfCheckReport) {
	let paths = file_path_set(files);

	if !paths.contains(Path::new("index.md")) {
		report.issues.push(issue(
			Some(PathBuf::from("index.md")),
			String::from("wiki profile expects a root progressive-disclosure index.md"),
		));
	}

	for dir in docs_dirs_with_content(files) {
		let index_path = dir.join("index.md");

		if !paths.contains(&index_path) {
			report.issues.push(issue(
				Some(index_path),
				String::from("wiki profile expects each populated directory to have index.md"),
			));
		}
	}
}

fn check_okf_wiki_frontmatter(files: &[DocsFile], report: &mut OkfCheckReport) {
	for file in files.iter().filter(|file| is_concept_markdown(&file.relative_path)) {
		let Some(fields) = okf_frontmatter_fields(file, report) else {
			continue;
		};

		read_required_okf_frontmatter_string(&fields, "title", &file.relative_path, report);
		read_required_okf_frontmatter_string(&fields, "description", &file.relative_path, report);
		okf_frontmatter_string_list(&fields, "tags", &file.relative_path, report);
	}
}

fn check_okf_links(files: &[DocsFile], report: &mut OkfCheckReport) -> Result<()> {
	let link_pattern = Regex::new(r"!?\[[^\]]*\]\(([^)\s]+)(?:\s+[^)]*)?\)")?;

	for file in files.iter().filter(|file| is_markdown(&file.relative_path)) {
		let Some(content) = file.content.as_deref() else {
			continue;
		};

		for captures in link_pattern.captures_iter(content) {
			let Some(target_match) = captures.get(1) else {
				continue;
			};
			let target = target_match.as_str();

			if should_skip_link_target(target) {
				continue;
			}

			report.link_count += 1;

			if let Some(link_path) = resolve_link_target(&file.path, &report.bundle_root, target)
				&& !link_path.exists()
			{
				report.issues.push(issue(
					Some(file.relative_path.clone()),
					format!("link target `{target}` does not exist"),
				));
			}
		}
	}

	Ok(())
}

fn check_okf_repo_memory_surface(files: &[DocsFile], report: &mut OkfCheckReport) {
	for file in files.iter().filter(|file| is_concept_markdown(&file.relative_path)) {
		let Some(fields) = okf_frontmatter_fields(file, report) else {
			continue;
		};

		validate_repo_memory_frontmatter_fields(
			&fields,
			&file.relative_path,
			&report.bundle_root.clone(),
			report,
		);
	}
}

fn check_required_docs_layout(files: &[DocsFile], report: &mut DocsCheckReport) {
	let paths = file_path_set(files);

	for required in REQUIRED_DOCS_FILES {
		if !paths.contains(Path::new(required)) {
			report.issues.push(issue(None, format!("required docs file `{required}` is missing")));
		}
	}

	let dirs = docs_dirs_with_content(files);

	for dir in dirs {
		let index_path = docs_dir_index_path(&dir);

		if !paths.contains(&index_path) {
			report.issues.push(issue(
				Some(index_path),
				String::from("directory must have an OKF progressive-disclosure index file"),
			));
		}
	}
}

fn docs_dir_index_path(dir: &Path) -> PathBuf {
	if dir == Path::new("research") {
		PathBuf::from("research/index.json")
	} else {
		dir.join("index.md")
	}
}

fn check_markdown_readability(files: &[DocsFile], report: &mut DocsCheckReport) {
	for file in files {
		if let Some(read_error) = &file.read_error {
			report.issues.push(issue(
				Some(file.relative_path.clone()),
				format!("Markdown file must be UTF-8 readable: {read_error}"),
			));
		}
	}
}

fn check_docs_artifact_types(files: &[DocsFile], report: &mut DocsCheckReport) {
	for file in files {
		if is_research_json(&file.relative_path) {
			continue;
		}
		if is_under_research(&file.relative_path) {
			report.issues.push(issue(
				Some(file.relative_path.clone()),
				String::from("docs/research/ accepts only JSON research artifacts"),
			));
		} else if !is_markdown(&file.relative_path) {
			let message = if file.path.extension().is_some_and(|extension| extension == "json") {
				"docs/ JSON artifacts are allowed only under docs/research/"
			} else {
				"docs/ accepts Markdown concepts plus JSON research artifacts only"
			};

			report.issues.push(issue(Some(file.relative_path.clone()), String::from(message)));
		}
	}
}

fn check_research_json_artifacts(files: &[DocsFile], report: &mut DocsCheckReport) {
	for file in files.iter().filter(|file| is_research_json(&file.relative_path)) {
		let raw = match fs::read_to_string(&file.path) {
			Ok(raw) => raw,
			Err(error) => {
				report.issues.push(issue(
					Some(file.relative_path.clone()),
					format!("research JSON must be UTF-8 readable: {error}"),
				));

				continue;
			},
		};
		let parsed = match serde_json::from_str::<JsonValue>(&raw) {
			Ok(parsed) => parsed,
			Err(error) => {
				report.issues.push(issue(
					Some(file.relative_path.clone()),
					format!("research JSON must parse: {error}"),
				));

				continue;
			},
		};

		validate_research_json_schema(file, &parsed, report);
	}
}

fn validate_research_json_schema(
	file: &DocsFile,
	parsed: &JsonValue,
	report: &mut DocsCheckReport,
) {
	let schema = parsed.get("schema").and_then(JsonValue::as_str);

	if file.relative_path == Path::new("research/index.json") {
		if schema != Some(RESEARCH_INDEX_SCHEMA) {
			report.issues.push(issue(
				Some(file.relative_path.clone()),
				format!("research index JSON must use schema `{RESEARCH_INDEX_SCHEMA}`"),
			));
		}
		if !parsed.get("reports").is_some_and(JsonValue::is_array) {
			report.issues.push(issue(
				Some(file.relative_path.clone()),
				String::from("research index JSON must include a `reports` array"),
			));
		}

		return;
	}

	if schema != Some(RESEARCH_REPORT_SCHEMA) {
		report.issues.push(issue(
			Some(file.relative_path.clone()),
			format!("research report JSON must use schema `{RESEARCH_REPORT_SCHEMA}`"),
		));
	}

	for key in REQUIRED_RESEARCH_REPORT_KEYS {
		if parsed.get(*key).is_none() {
			report.issues.push(issue(
				Some(file.relative_path.clone()),
				format!("research report JSON is missing required key `{key}`"),
			));
		}
	}
}

fn check_acronym_capitalization(files: &[DocsFile], report: &mut DocsCheckReport) {
	for file in files.iter().filter(|file| is_markdown(&file.relative_path)) {
		let Some(content) = file.content.as_deref() else {
			continue;
		};

		if content.contains("Okf") {
			report.issues.push(issue(
				Some(file.relative_path.clone()),
				String::from(
					"use `OKF` in prose; lowercase `okf` is reserved for paths, slugs, tags, and URLs",
				),
			));
		}
	}
}

fn check_concept_contracts(files: &[DocsFile], report: &mut DocsCheckReport) {
	for file in files.iter().filter(|file| is_concept_markdown(&file.relative_path)) {
		report.concept_count += 1;

		let Some(content) = file.content.as_deref() else {
			continue;
		};
		let Some((frontmatter, body)) = split_yaml_frontmatter(content) else {
			report.issues.push(issue(
				Some(file.relative_path.clone()),
				String::from("concept must start with YAML frontmatter delimited by ---"),
			));

			continue;
		};
		let Some(fields) = parse_frontmatter_mapping(frontmatter, &file.relative_path, report)
		else {
			continue;
		};

		for required_key in REQUIRED_CONCEPT_KEYS {
			read_required_frontmatter_string(&fields, required_key, &file.relative_path, report);
		}

		validate_frontmatter_enum(
			&fields,
			"type",
			ALLOWED_CONCEPT_TYPES,
			&file.relative_path,
			report,
		);
		validate_frontmatter_enum(&fields, "status", ALLOWED_STATUSES, &file.relative_path, report);
		validate_frontmatter_enum(
			&fields,
			"authority",
			ALLOWED_AUTHORITIES,
			&file.relative_path,
			report,
		);
		validate_frontmatter_date(&fields, &file.relative_path, report);
		validate_type_specific_headings(&fields, body, &file.relative_path, report);
		validate_structured_frontmatter_fields(
			&fields,
			&file.relative_path,
			&report.docs_root.clone(),
			report,
		);
	}
}

fn check_links(files: &[DocsFile], report: &mut DocsCheckReport) -> Result<()> {
	let link_pattern = Regex::new(r"!?\[[^\]]*\]\(([^)\s]+)(?:\s+[^)]*)?\)")?;

	for file in files.iter().filter(|file| is_markdown(&file.relative_path)) {
		let Some(content) = file.content.as_deref() else {
			continue;
		};

		for captures in link_pattern.captures_iter(content) {
			let Some(target_match) = captures.get(1) else {
				continue;
			};
			let target = target_match.as_str();

			if should_skip_link_target(target) {
				continue;
			}

			report.link_count += 1;

			if let Some(link_path) = resolve_link_target(&file.path, &report.docs_root, target)
				&& !link_path.exists()
			{
				report.issues.push(issue(
					Some(file.relative_path.clone()),
					format!("link target `{target}` does not exist"),
				));
			}
		}
	}

	Ok(())
}

fn check_drift_surface(files: &[DocsFile], report: &mut DocsCheckReport) {
	let has_drift_concept = files.iter().any(|file| {
		is_concept_markdown(&file.relative_path)
			&& concept_type(file).is_some_and(|concept_type| concept_type == "Drift Audit")
	});

	if !has_drift_concept {
		report.issues.push(issue(
			Some(PathBuf::from("evidence/")),
			String::from(
				"at least one Drift Audit evidence concept must anchor the docs self-check loop",
			),
		));
	}
}

fn file_path_set(files: &[DocsFile]) -> BTreeSet<PathBuf> {
	files.iter().map(|file| file.relative_path.clone()).collect()
}

fn docs_dirs_with_content(files: &[DocsFile]) -> BTreeSet<PathBuf> {
	let mut dirs = BTreeSet::new();

	for file in files {
		let Some(parent) = file.relative_path.parent() else {
			continue;
		};

		if parent.as_os_str().is_empty() {
			continue;
		}

		dirs.insert(parent.to_path_buf());
	}

	dirs
}

fn is_markdown(path: &Path) -> bool {
	path.extension().is_some_and(|extension| extension == "md")
}

fn is_under_research(path: &Path) -> bool {
	path.starts_with("research")
}

fn is_research_json(path: &Path) -> bool {
	path.parent() == Some(Path::new("research"))
		&& path.extension().is_some_and(|extension| extension == "json")
}

fn is_concept_markdown(path: &Path) -> bool {
	is_markdown(path) && path.file_name().is_some_and(|name| name != "index.md" && name != "log.md")
}

fn concept_type(file: &DocsFile) -> Option<String> {
	let content = file.content.as_deref()?;
	let (frontmatter, _) = split_yaml_frontmatter(content)?;
	let Value::Mapping(fields) = serde_yaml::from_str::<Value>(frontmatter).ok()? else {
		return None;
	};

	frontmatter_string(&fields, "type").map(str::to_owned)
}

fn split_yaml_frontmatter(content: &str) -> Option<(&str, &str)> {
	let (body_start, closing_delimiter) = if let Some(body_start) = content.strip_prefix("---\n") {
		(body_start, "\n---\n")
	} else {
		(content.strip_prefix("---\r\n")?, "\r\n---\r\n")
	};
	let closing = body_start.find(closing_delimiter)?;
	let frontmatter = &body_start[..closing];
	let body = &body_start[(closing + closing_delimiter.len())..];

	Some((frontmatter, body))
}

fn parse_frontmatter_mapping(
	frontmatter: &str,
	path: &Path,
	report: &mut DocsCheckReport,
) -> Option<Mapping> {
	match serde_yaml::from_str::<Value>(frontmatter) {
		Ok(Value::Mapping(mapping)) => Some(mapping),
		Ok(_) => {
			report.issues.push(issue(
				Some(path.to_path_buf()),
				String::from("frontmatter must be a YAML mapping"),
			));

			None
		},
		Err(error) => {
			report.issues.push(issue(
				Some(path.to_path_buf()),
				format!("frontmatter must parse as YAML: {error}"),
			));

			None
		},
	}
}

fn okf_frontmatter_fields(file: &DocsFile, report: &mut OkfCheckReport) -> Option<Mapping> {
	let content = file.content.as_deref()?;
	let Some((frontmatter, _)) = split_yaml_frontmatter(content) else {
		report.issues.push(issue(
			Some(file.relative_path.clone()),
			String::from("concept must start with YAML frontmatter delimited by ---"),
		));

		return None;
	};

	parse_okf_frontmatter_mapping(frontmatter, &file.relative_path, report)
}

fn parse_okf_frontmatter_mapping(
	frontmatter: &str,
	path: &Path,
	report: &mut OkfCheckReport,
) -> Option<Mapping> {
	match serde_yaml::from_str::<Value>(frontmatter) {
		Ok(Value::Mapping(mapping)) => Some(mapping),
		Ok(_) => {
			report.issues.push(issue(
				Some(path.to_path_buf()),
				String::from("frontmatter must be a YAML mapping"),
			));

			None
		},
		Err(error) => {
			report.issues.push(issue(
				Some(path.to_path_buf()),
				format!("frontmatter must parse as YAML: {error}"),
			));

			None
		},
	}
}

fn read_required_okf_frontmatter_string(
	fields: &Mapping,
	key: &str,
	path: &Path,
	report: &mut OkfCheckReport,
) {
	match frontmatter_value(fields, key) {
		Some(Value::String(value)) if !value.trim().is_empty() => {},
		Some(Value::String(_)) | None => report.issues.push(issue(
			Some(path.to_path_buf()),
			format!("frontmatter key `{key}` is required and must be non-empty"),
		)),
		Some(_) => report.issues.push(issue(
			Some(path.to_path_buf()),
			format!("frontmatter key `{key}` must be a string"),
		)),
	}
}

fn okf_frontmatter_string_list(
	fields: &Mapping,
	key: &str,
	path: &Path,
	report: &mut OkfCheckReport,
) -> Option<Vec<String>> {
	match frontmatter_value(fields, key) {
		None => None,
		Some(Value::Sequence(items)) => {
			let mut values = Vec::new();

			for item in items {
				match item {
					Value::String(value) if !value.trim().is_empty() => {
						values.push(value.trim().to_owned());
					},
					Value::String(_) => report.issues.push(issue(
						Some(path.to_path_buf()),
						format!("frontmatter list `{key}` must not contain empty strings"),
					)),
					_ => report.issues.push(issue(
						Some(path.to_path_buf()),
						format!("frontmatter list `{key}` must contain only strings"),
					)),
				}
			}

			Some(values)
		},
		Some(_) => {
			report.issues.push(issue(
				Some(path.to_path_buf()),
				format!("frontmatter key `{key}` must be a list of strings"),
			));

			None
		},
	}
}

fn frontmatter_value<'a>(fields: &'a Mapping, key: &str) -> Option<&'a Value> {
	fields.get(Value::String(key.to_owned()))
}

fn frontmatter_string<'a>(fields: &'a Mapping, key: &str) -> Option<&'a str> {
	match frontmatter_value(fields, key) {
		Some(Value::String(value)) => Some(value.trim()),
		_ => None,
	}
}

fn frontmatter_string_list(
	fields: &Mapping,
	key: &str,
	path: &Path,
	report: &mut DocsCheckReport,
) -> Option<Vec<String>> {
	match frontmatter_value(fields, key) {
		None => None,
		Some(Value::Sequence(items)) => {
			let mut values = Vec::new();

			for item in items {
				match item {
					Value::String(value) if !value.trim().is_empty() => {
						values.push(value.trim().to_owned());
					},
					Value::String(_) => report.issues.push(issue(
						Some(path.to_path_buf()),
						format!("frontmatter list `{key}` must not contain empty strings"),
					)),
					_ => report.issues.push(issue(
						Some(path.to_path_buf()),
						format!("frontmatter list `{key}` must contain only strings"),
					)),
				}
			}

			Some(values)
		},
		Some(_) => {
			report.issues.push(issue(
				Some(path.to_path_buf()),
				format!("frontmatter key `{key}` must be a list of strings"),
			));

			None
		},
	}
}

fn read_required_frontmatter_string(
	fields: &Mapping,
	key: &str,
	path: &Path,
	report: &mut DocsCheckReport,
) {
	match frontmatter_value(fields, key) {
		Some(Value::String(value)) if !value.trim().is_empty() => {},
		Some(Value::String(_)) | None => report.issues.push(issue(
			Some(path.to_path_buf()),
			format!("frontmatter key `{key}` is required and must be non-empty"),
		)),
		Some(_) => report.issues.push(issue(
			Some(path.to_path_buf()),
			format!("frontmatter key `{key}` must be a string"),
		)),
	}
}

fn validate_frontmatter_enum(
	fields: &Mapping,
	key: &str,
	allowed_values: &[&str],
	path: &Path,
	report: &mut DocsCheckReport,
) {
	let Some(value) = frontmatter_string(fields, key) else {
		return;
	};

	if !value.is_empty() && !allowed_values.contains(&value) {
		report.issues.push(issue(
			Some(path.to_path_buf()),
			format!("frontmatter key `{key}` has unsupported value `{value}`"),
		));
	}
}

fn validate_frontmatter_date(fields: &Mapping, path: &Path, report: &mut DocsCheckReport) {
	let Some(value) = frontmatter_string(fields, "last_verified") else {
		return;
	};

	if !value.is_empty() && !is_valid_iso_date(value) {
		report.issues.push(issue(
			Some(path.to_path_buf()),
			format!("frontmatter key `last_verified` must be an ISO date, not `{value}`"),
		));
	}
}

fn validate_structured_frontmatter_fields(
	fields: &Mapping,
	path: &Path,
	docs_root: &Path,
	report: &mut DocsCheckReport,
) {
	for key in ["tags", "drift_watch"] {
		frontmatter_string_list(fields, key, path, report);
	}

	validate_source_refs(fields, path, report);
	validate_code_refs(fields, path, docs_root, report);
	validate_related_refs(fields, path, docs_root, report);
	validate_promotes_to(fields, path, report);
}

fn validate_repo_memory_frontmatter_fields(
	fields: &Mapping,
	path: &Path,
	bundle_root: &Path,
	report: &mut OkfCheckReport,
) {
	for key in ["tags", "drift_watch"] {
		okf_frontmatter_string_list(fields, key, path, report);
	}

	validate_okf_source_refs(fields, path, report);
	validate_okf_code_refs(fields, path, bundle_root, report);
	validate_okf_related_refs(fields, path, bundle_root, report);
}

fn validate_okf_source_refs(fields: &Mapping, path: &Path, report: &mut OkfCheckReport) {
	let Some(values) = okf_frontmatter_string_list(fields, "source_refs", path, report) else {
		return;
	};

	for value in values {
		if !is_http_url(&value) {
			report.issues.push(issue(
				Some(path.to_path_buf()),
				format!("source_refs entry `{value}` must be an http(s) URL"),
			));
		}
	}
}

fn validate_okf_code_refs(
	fields: &Mapping,
	path: &Path,
	bundle_root: &Path,
	report: &mut OkfCheckReport,
) {
	let Some(values) = okf_frontmatter_string_list(fields, "code_refs", path, report) else {
		return;
	};
	let repo_root = bundle_root.parent().unwrap_or(bundle_root);

	for value in values {
		validate_okf_code_ref_value(path, repo_root, &value, report);
	}
}

fn validate_okf_code_ref_value(
	path: &Path,
	repo_root: &Path,
	value: &str,
	report: &mut OkfCheckReport,
) {
	let value_path = Path::new(value);

	if value.contains('#') || value.contains('?') {
		report.issues.push(issue(
			Some(path.to_path_buf()),
			format!("code_refs entry `{value}` must be a file path without fragments"),
		));

		return;
	}
	if !is_normalized_relative_path(value_path) {
		report.issues.push(issue(
			Some(path.to_path_buf()),
			format!("code_refs entry `{value}` must be a normalized repository-relative file path"),
		));

		return;
	}

	let target_path = normalize_path(&repo_root.join(value_path));

	if !target_path.exists() {
		report.issues.push(issue(
			Some(path.to_path_buf()),
			format!("code_refs entry `{value}` does not exist"),
		));
	} else if !target_path.is_file() {
		report.issues.push(issue(
			Some(path.to_path_buf()),
			format!("code_refs entry `{value}` must reference a file"),
		));
	}
}

fn validate_okf_related_refs(
	fields: &Mapping,
	path: &Path,
	bundle_root: &Path,
	report: &mut OkfCheckReport,
) {
	let Some(values) = okf_frontmatter_string_list(fields, "related", path, report) else {
		return;
	};

	for value in values {
		validate_okf_related_ref_value(path, bundle_root, &value, report);
	}
}

fn validate_okf_related_ref_value(
	path: &Path,
	bundle_root: &Path,
	value: &str,
	report: &mut OkfCheckReport,
) {
	let target = strip_fragment(value);

	if target.is_empty() {
		report.issues.push(issue(
			Some(path.to_path_buf()),
			format!("related entry `{value}` must include a bundle file path"),
		));

		return;
	}

	let target_value_path = Path::new(target);

	if target_value_path.is_absolute() {
		report.issues.push(issue(
			Some(path.to_path_buf()),
			format!("related entry `{value}` must be a bundle-relative file path"),
		));

		return;
	}

	let parent = path.parent().unwrap_or_else(|| Path::new(""));
	let target_path = normalize_path(&bundle_root.join(parent).join(target_value_path));

	if !target_path.starts_with(bundle_root) {
		report.issues.push(issue(
			Some(path.to_path_buf()),
			format!("related entry `{value}` must stay under the OKF bundle"),
		));

		return;
	}
	if !target_path.exists() {
		report.issues.push(issue(
			Some(path.to_path_buf()),
			format!("related entry `{value}` does not exist"),
		));
	} else if !target_path.is_file() || !is_markdown(&target_path) {
		report.issues.push(issue(
			Some(path.to_path_buf()),
			format!("related entry `{value}` must reference a Markdown file"),
		));
	}
}

fn validate_source_refs(fields: &Mapping, path: &Path, report: &mut DocsCheckReport) {
	let Some(values) = frontmatter_string_list(fields, "source_refs", path, report) else {
		return;
	};

	for value in values {
		if !is_http_url(&value) {
			report.issues.push(issue(
				Some(path.to_path_buf()),
				format!("source_refs entry `{value}` must be an http(s) URL"),
			));
		}
	}
}

fn validate_code_refs(
	fields: &Mapping,
	path: &Path,
	docs_root: &Path,
	report: &mut DocsCheckReport,
) {
	let Some(values) = frontmatter_string_list(fields, "code_refs", path, report) else {
		return;
	};
	let repo_root = docs_root.parent().unwrap_or(docs_root);

	for value in values {
		let value_path = Path::new(&value);

		if value.contains('#') || value.contains('?') {
			report.issues.push(issue(
				Some(path.to_path_buf()),
				format!("code_refs entry `{value}` must be a file path without fragments"),
			));

			continue;
		}
		if !is_normalized_relative_path(value_path) {
			report.issues.push(issue(
				Some(path.to_path_buf()),
				format!(
					"code_refs entry `{value}` must be a normalized repository-relative file path"
				),
			));

			continue;
		}

		let target_path = normalize_path(&repo_root.join(value_path));

		if !target_path.exists() {
			report.issues.push(issue(
				Some(path.to_path_buf()),
				format!("code_refs entry `{value}` does not exist"),
			));
		} else if !target_path.is_file() {
			report.issues.push(issue(
				Some(path.to_path_buf()),
				format!("code_refs entry `{value}` must reference a file"),
			));
		}
	}
}

fn validate_related_refs(
	fields: &Mapping,
	path: &Path,
	docs_root: &Path,
	report: &mut DocsCheckReport,
) {
	let Some(values) = frontmatter_string_list(fields, "related", path, report) else {
		return;
	};

	for value in values {
		let target = strip_fragment(&value);

		if target.is_empty() {
			report.issues.push(issue(
				Some(path.to_path_buf()),
				format!("related entry `{value}` must include a docs file path"),
			));

			continue;
		}

		let target_value_path = Path::new(target);

		if target_value_path.is_absolute() {
			report.issues.push(issue(
				Some(path.to_path_buf()),
				format!("related entry `{value}` must be a docs-relative file path"),
			));

			continue;
		}

		let relative_target = target.strip_prefix("docs/").unwrap_or(target);
		let target_path = if target.starts_with("docs/") {
			normalize_path(&docs_root.join(relative_target))
		} else {
			let parent = path.parent().unwrap_or_else(|| Path::new(""));

			normalize_path(&docs_root.join(parent).join(relative_target))
		};

		if !target_path.starts_with(docs_root) {
			report.issues.push(issue(
				Some(path.to_path_buf()),
				format!("related entry `{value}` must stay under docs/"),
			));

			continue;
		}
		if !target_path.exists() {
			report.issues.push(issue(
				Some(path.to_path_buf()),
				format!("related entry `{value}` does not exist"),
			));
		} else if !target_path.is_file() || !is_markdown(&target_path) {
			report.issues.push(issue(
				Some(path.to_path_buf()),
				format!("related entry `{value}` must reference a Markdown file"),
			));
		}
	}
}

fn validate_promotes_to(fields: &Mapping, path: &Path, report: &mut DocsCheckReport) {
	let Some(values) = frontmatter_string_list(fields, "promotes_to", path, report) else {
		return;
	};

	for value in values {
		if !ALLOWED_PROMOTION_TARGETS.contains(&value.as_str()) {
			report.issues.push(issue(
				Some(path.to_path_buf()),
				format!("promotes_to entry `{value}` is not an authoritative promotion lane"),
			));
		}
	}
}

fn strip_fragment(value: &str) -> &str {
	value.split_once('#').map_or(value, |(path, _)| path)
}

fn is_http_url(value: &str) -> bool {
	let Ok(url) = Url::parse(value) else {
		return false;
	};

	matches!(url.scheme(), "http" | "https")
		&& url.host_str().is_some_and(|host| !host.trim().is_empty())
}

fn is_normalized_relative_path(path: &Path) -> bool {
	!path.is_absolute()
		&& path.components().all(|component| matches!(component, Component::Normal(_)))
}

fn is_valid_iso_date(value: &str) -> bool {
	let mut parts = value.split('-');
	let Some(year) = parts.next().and_then(|year| year.parse::<i32>().ok()) else {
		return false;
	};
	let Some(month) = parts.next().and_then(|month| month.parse::<u8>().ok()) else {
		return false;
	};
	let Some(day) = parts.next().and_then(|day| day.parse::<u8>().ok()) else {
		return false;
	};

	if parts.next().is_some() {
		return false;
	}

	let Ok(month) = Month::try_from(month) else {
		return false;
	};

	Date::from_calendar_date(year, month, day).is_ok()
}

fn validate_type_specific_headings(
	fields: &Mapping,
	body: &str,
	path: &Path,
	report: &mut DocsCheckReport,
) {
	let Some(concept_type) = frontmatter_string(fields, "type") else {
		return;
	};
	let required_headings = match concept_type {
		"Research Contract" => RESEARCH_CONTRACT_HEADINGS,
		"Drift Audit" => DRIFT_AUDIT_HEADINGS,
		_ => return,
	};
	let headings = markdown_heading_texts(body);

	for required_heading in required_headings {
		if !headings.contains(*required_heading) {
			report.issues.push(issue(
				Some(path.to_path_buf()),
				format!("`{concept_type}` concept must include heading `{required_heading}`"),
			));
		}
	}
}

fn markdown_heading_texts(body: &str) -> BTreeSet<String> {
	body.lines()
		.filter_map(|line| {
			let line = line.trim_start();
			let heading = line.strip_prefix('#')?;
			let heading = heading.trim_start_matches('#').trim();

			if heading.is_empty() {
				return None;
			}

			Some(heading.trim_end_matches('#').trim().to_owned())
		})
		.collect()
}

fn concept_summary(file: &DocsFile) -> Option<OkfConceptSummary> {
	let content = file.content.as_deref()?;
	let (frontmatter, _) = split_yaml_frontmatter(content)?;
	let Value::Mapping(fields) = serde_yaml::from_str::<Value>(frontmatter).ok()? else {
		return None;
	};
	let concept_type = frontmatter_string(&fields, "type")?.to_owned();
	let path = path_to_string(&file.relative_path);
	let title = frontmatter_string(&fields, "title")
		.filter(|title| !title.is_empty())
		.map_or_else(|| concept_id(&file.relative_path), str::to_owned);
	let description = frontmatter_string(&fields, "description")
		.filter(|description| !description.is_empty())
		.map(str::to_owned);
	let resource = frontmatter_string(&fields, "resource")
		.filter(|resource| !resource.is_empty())
		.map(str::to_owned);
	let tags = frontmatter_string_list_lossy(&fields, "tags");
	let source_refs = frontmatter_string_list_lossy(&fields, "source_refs");
	let code_refs = frontmatter_string_list_lossy(&fields, "code_refs");
	let related = frontmatter_string_list_lossy(&fields, "related");

	Some(OkfConceptSummary {
		id: concept_id(&file.relative_path),
		path,
		concept_type,
		title,
		description,
		resource,
		tags,
		source_refs,
		code_refs,
		related,
	})
}

fn concept_id(path: &Path) -> String {
	let mut id = path.to_path_buf();

	id.set_extension("");

	path_to_string(&id)
}

fn path_to_string(path: &Path) -> String {
	path.to_string_lossy().replace('\\', "/")
}

fn frontmatter_string_list_lossy(fields: &Mapping, key: &str) -> Vec<String> {
	match frontmatter_value(fields, key) {
		Some(Value::Sequence(items)) => items
			.iter()
			.filter_map(|item| match item {
				Value::String(value) if !value.trim().is_empty() => Some(value.trim().to_owned()),
				_ => None,
			})
			.collect(),
		_ => Vec::new(),
	}
}

fn okf_query_matches(concept: &OkfConceptSummary, query: &OkfQuery) -> bool {
	query
		.concept_type
		.as_deref()
		.is_none_or(|value| concept.concept_type.eq_ignore_ascii_case(value))
		&& query
			.tags
			.iter()
			.all(|tag| concept.tags.iter().any(|candidate| candidate.eq_ignore_ascii_case(tag)))
		&& query.resource.as_deref().is_none_or(|value| {
			concept.resource.as_deref().is_some_and(|resource| contains_ci(resource, value))
		}) && query.source_ref.as_deref().is_none_or(|value| {
		concept.source_refs.iter().any(|source_ref| contains_ci(source_ref, value))
	}) && query
		.code_ref
		.as_deref()
		.is_none_or(|value| concept.code_refs.iter().any(|code_ref| contains_ci(code_ref, value)))
		&& query
			.related
			.as_deref()
			.is_none_or(|value| concept.related.iter().any(|related| contains_ci(related, value)))
		&& query.text.as_deref().is_none_or(|value| concept_text_matches(concept, value))
}

fn concept_text_matches(concept: &OkfConceptSummary, value: &str) -> bool {
	contains_ci(&concept.path, value)
		|| contains_ci(&concept.title, value)
		|| concept.description.as_deref().is_some_and(|description| contains_ci(description, value))
}

fn contains_ci(haystack: &str, needle: &str) -> bool {
	haystack.to_lowercase().contains(&needle.to_lowercase())
}

fn okf_concept_path_set(files: &[DocsFile]) -> BTreeSet<PathBuf> {
	files
		.iter()
		.filter(|file| is_concept_markdown(&file.relative_path))
		.map(|file| file.relative_path.clone())
		.collect()
}

fn collect_markdown_graph_edges(
	file: &DocsFile,
	bundle_root: &Path,
	concept_paths: &BTreeSet<PathBuf>,
	source: &str,
	edges: &mut Vec<OkfGraphEdge>,
	broken_links: &mut Vec<OkfBrokenLink>,
) -> Result<()> {
	let Some(content) = file.content.as_deref() else {
		return Ok(());
	};
	let link_pattern = Regex::new(r"!?\[[^\]]*\]\(([^)\s]+)(?:\s+[^)]*)?\)")?;

	for captures in link_pattern.captures_iter(content) {
		let Some(target_match) = captures.get(1) else {
			continue;
		};
		let target = target_match.as_str();

		if should_skip_link_target(target) {
			continue;
		}

		push_graph_target(
			file,
			bundle_root,
			concept_paths,
			source,
			target,
			"markdown",
			edges,
			broken_links,
		);
	}

	Ok(())
}

fn collect_related_graph_edges(
	file: &DocsFile,
	bundle_root: &Path,
	concept_paths: &BTreeSet<PathBuf>,
	source: &str,
	edges: &mut Vec<OkfGraphEdge>,
	broken_links: &mut Vec<OkfBrokenLink>,
) {
	let Some(content) = file.content.as_deref() else {
		return;
	};
	let Some((frontmatter, _)) = split_yaml_frontmatter(content) else {
		return;
	};
	let Ok(Value::Mapping(fields)) = serde_yaml::from_str::<Value>(frontmatter) else {
		return;
	};

	for target in frontmatter_string_list_lossy(&fields, "related") {
		push_graph_target(
			file,
			bundle_root,
			concept_paths,
			source,
			&target,
			"related",
			edges,
			broken_links,
		);
	}
}

#[allow(clippy::too_many_arguments)]
fn push_graph_target(
	file: &DocsFile,
	bundle_root: &Path,
	concept_paths: &BTreeSet<PathBuf>,
	source: &str,
	target: &str,
	kind: &str,
	edges: &mut Vec<OkfGraphEdge>,
	broken_links: &mut Vec<OkfBrokenLink>,
) {
	let Some(target_path) = resolve_link_target(&file.path, bundle_root, target) else {
		return;
	};
	let Ok(relative_target) = target_path.strip_prefix(bundle_root) else {
		return;
	};
	let relative_target = relative_target.to_path_buf();

	if concept_paths.contains(&relative_target) {
		edges.push(OkfGraphEdge {
			source: source.to_owned(),
			target: concept_id(&relative_target),
			kind: kind.to_owned(),
		});
	} else if !target_path.exists() {
		broken_links.push(broken_link(source, target, kind));
	}
}

fn broken_link(source: &str, target: &str, kind: &str) -> OkfBrokenLink {
	OkfBrokenLink { source: source.to_owned(), target: target.to_owned(), kind: kind.to_owned() }
}

fn okf_orphan_concepts(concepts: &[OkfConceptSummary], edges: &[OkfGraphEdge]) -> Vec<String> {
	let connected: BTreeSet<&str> =
		edges.iter().flat_map(|edge| [edge.source.as_str(), edge.target.as_str()]).collect();

	concepts
		.iter()
		.filter(|concept| !connected.contains(concept.id.as_str()))
		.map(|concept| concept.id.clone())
		.collect()
}

fn route_tokens(intent: &str) -> Vec<String> {
	intent
		.split(|character: char| !character.is_alphanumeric())
		.map(str::trim)
		.filter(|token| token.chars().count() >= 3)
		.map(str::to_lowercase)
		.collect()
}

fn route_score(file: &DocsFile, concept: &OkfConceptSummary, tokens: &[String]) -> usize {
	let strong_text = format!(
		"{} {} {} {}",
		concept.path,
		concept.title,
		concept.description.as_deref().unwrap_or_default(),
		concept.tags.join(" ")
	)
	.to_lowercase();
	let body = file.content.as_deref().unwrap_or_default().to_lowercase();

	tokens
		.iter()
		.map(|token| {
			usize::from(strong_text.contains(token)) * 3 + usize::from(body.contains(token))
		})
		.sum()
}

fn should_skip_link_target(target: &str) -> bool {
	target.starts_with('#')
		|| target.starts_with("http://")
		|| target.starts_with("https://")
		|| target.starts_with("mailto:")
		|| target.starts_with("tel:")
}

fn resolve_link_target(source_path: &Path, docs_root: &Path, target: &str) -> Option<PathBuf> {
	let path_without_anchor = target.split('#').next().unwrap_or_default();
	let path_without_query = path_without_anchor.split('?').next().unwrap_or_default();

	if path_without_query.is_empty() {
		return None;
	}

	let raw_path = if let Some(root_relative) = path_without_query.strip_prefix('/') {
		docs_root.join(root_relative)
	} else {
		source_path.parent()?.join(path_without_query)
	};

	Some(normalize_path(&raw_path))
}

fn normalize_path(path: &Path) -> PathBuf {
	let mut normalized = PathBuf::new();

	for component in path.components() {
		match component {
			Component::ParentDir => {
				normalized.pop();
			},
			Component::CurDir => {},
			other => normalized.push(other.as_os_str()),
		}
	}

	normalized
}

fn issue(path: Option<PathBuf>, message: String) -> DocsCheckIssue {
	DocsCheckIssue { path, message }
}

#[cfg(test)]
mod tests {
	use std::fs;

	use tempfile::TempDir;

	use crate::docs_okf::{self, DocsCheckScope, OkfCheckProfile, OkfQuery};

	#[test]
	fn docs_check_rejects_json_artifacts_outside_research() {
		let temp_dir = TempDir::new().expect("tempdir");
		let docs = temp_dir.path().join("docs");

		write_minimal_okf_bundle(&docs);
		write(&docs.join("research.json"), "{}\n");

		let report = docs_okf::run_docs_check(&docs, DocsCheckScope::All).expect("check");

		assert!(report.has_issues());
		assert!(report.issues.iter().any(|issue| {
			issue.message.contains("JSON artifacts are allowed only under docs/research")
		}));
	}

	#[test]
	fn docs_check_accepts_research_json_artifacts() {
		let temp_dir = TempDir::new().expect("tempdir");
		let docs = temp_dir.path().join("docs");

		write_minimal_okf_bundle(&docs);
		write(&docs.join("research/sample-report.json"), research_report_json("Sample Research"));

		let report = docs_okf::run_docs_check(&docs, DocsCheckScope::All).expect("check");

		assert!(!report.has_issues(), "{report:#?}");
	}

	#[test]
	fn docs_check_rejects_markdown_inside_research() {
		let temp_dir = TempDir::new().expect("tempdir");
		let docs = temp_dir.path().join("docs");

		write_minimal_okf_bundle(&docs);
		write(&docs.join("research/sample.md"), "# Research\n");

		let report = docs_okf::run_docs_check(&docs, DocsCheckScope::All).expect("check");

		assert!(report.has_issues());
		assert!(report.issues.iter().any(|issue| {
			issue.message.contains("docs/research/ accepts only JSON research artifacts")
		}));
	}

	#[test]
	fn docs_check_rejects_mis_capitalized_okf_acronym() {
		let temp_dir = TempDir::new().expect("tempdir");
		let docs = temp_dir.path().join("docs");

		write_minimal_okf_bundle(&docs);
		write(
			&docs.join("policy.md"),
			"---\ntype: Policy\ntitle: Okf policy\ndescription: Test concept.\nstatus: active\nauthority: non_authoritative\nowner: docs\nlast_verified: 2026-06-16\n---\n\n# Purpose\nOkf should be uppercase.\n",
		);

		let report = docs_okf::run_docs_check(&docs, DocsCheckScope::All).expect("check");

		assert!(report.has_issues());
		assert!(report.issues.iter().any(|issue| issue.message.contains("use `OKF`")));
	}

	#[test]
	fn docs_check_passes_minimal_okf_bundle() {
		let temp_dir = TempDir::new().expect("tempdir");
		let docs = temp_dir.path().join("docs");

		write_minimal_okf_bundle(&docs);

		let report = docs_okf::run_docs_check(&docs, DocsCheckScope::All).expect("check");

		assert!(!report.has_issues(), "{report:#?}");
	}

	#[test]
	fn docs_check_rejects_non_markdown_artifacts() {
		let temp_dir = TempDir::new().expect("tempdir");
		let docs = temp_dir.path().join("docs");

		write_minimal_okf_bundle(&docs);
		write(&docs.join("stray.txt"), "not OKF\n");

		let report = docs_okf::run_docs_check(&docs, DocsCheckScope::All).expect("check");

		assert!(report.has_issues());
		assert!(
			report
				.issues
				.iter()
				.any(|issue| issue.message.contains("Markdown concepts plus JSON research"))
		);
	}

	#[test]
	fn docs_check_rejects_invalid_frontmatter_values() {
		let temp_dir = TempDir::new().expect("tempdir");
		let docs = temp_dir.path().join("docs");

		write_minimal_okf_bundle(&docs);
		write(
			&docs.join("policy.md"),
			"---\ntype: Policy\ntitle: Docs policy\ndescription: Test concept.\nstatus: nonsense\nauthority: non_authoritative\nowner: docs\nlast_verified: yesterday\n---\n\n# Purpose\nTest.\n",
		);

		let report = docs_okf::run_docs_check(&docs, DocsCheckScope::All).expect("check");

		assert!(report.has_issues());
		assert!(report.issues.iter().any(|issue| issue.message.contains("unsupported value")));
		assert!(report.issues.iter().any(|issue| issue.message.contains("must be an ISO date")));
	}

	#[test]
	fn docs_check_rejects_invalid_structured_frontmatter_refs() {
		let temp_dir = TempDir::new().expect("tempdir");
		let docs = temp_dir.path().join("docs");

		write_minimal_okf_bundle(&docs);
		write(
			&docs.join("policy.md"),
			"---\ntype: Policy\ntitle: Docs policy\ndescription: Test concept.\nstatus: active\nauthority: non_authoritative\nowner: docs\nlast_verified: 2026-06-16\nsource_refs: ['https://', not-a-url]\ncode_refs: [missing.rs, docs/]\nrelated: [missing.md, '#heading']\npromotes_to: [docs/research]\ndrift_watch: not-a-list\n---\n\n# Purpose\nTest.\n",
		);

		let report = docs_okf::run_docs_check(&docs, DocsCheckScope::All).expect("check");

		assert!(report.has_issues());
		assert!(report.issues.iter().any(|issue| issue.message.contains("http(s) URL")));
		assert!(report.issues.iter().any(|issue| issue.message.contains("code_refs entry")));
		assert!(report.issues.iter().any(|issue| issue.message.contains("related entry")));
		assert!(report.issues.iter().any(|issue| issue.message.contains("promotes_to entry")));
		assert!(report.issues.iter().any(|issue| issue.message.contains("drift_watch")));
	}

	#[test]
	fn okf_core_check_allows_unknown_types_and_missing_decodex_fields() {
		let temp_dir = TempDir::new().expect("tempdir");
		let bundle = temp_dir.path().join("bundle");

		fs::create_dir_all(&bundle).expect("bundle");

		write(&bundle.join("index.md"), "# Bundle\n");
		write(
			&bundle.join("metric.md"),
			"---\ntype: Business Metric\n---\n\nWeekly active users.\n",
		);

		let report = docs_okf::run_okf_check(&bundle, OkfCheckProfile::Core).expect("core check");

		assert!(!report.has_issues(), "{report:#?}");
	}

	#[test]
	fn okf_graph_skips_links_outside_the_bundle() {
		let temp_dir = TempDir::new().expect("tempdir");
		let bundle = temp_dir.path().join("bundle");

		fs::create_dir_all(&bundle).expect("bundle");

		write(&temp_dir.path().join("README.md"), "# External repo doc\n");
		write(&bundle.join("index.md"), "# Bundle\n");
		write(
			&bundle.join("alpha.md"),
			"---\ntype: Concept\ntitle: Alpha\ndescription: Alpha concept.\n---\n\nSee [Beta](beta.md) and [repo readme](../README.md).\n",
		);
		write(
			&bundle.join("beta.md"),
			"---\ntype: Concept\ntitle: Beta\ndescription: Beta concept.\n---\n\nBeta.\n",
		);

		let graph = docs_okf::build_okf_graph(&bundle).expect("graph");

		assert_eq!(graph.broken_links, Vec::new());
		assert_eq!(graph.edges.len(), 1);
		assert_eq!(graph.edges[0].target, "beta");
	}

	#[test]
	fn okf_query_matches_structured_frontmatter_refs() {
		let temp_dir = TempDir::new().expect("tempdir");
		let bundle = temp_dir.path().join("docs");

		fs::create_dir_all(&bundle).expect("bundle");

		write(&temp_dir.path().join("src.rs"), "fn main() {}\n");
		write(&bundle.join("index.md"), "# Bundle\n");
		write(
			&bundle.join("alpha.md"),
			"---\ntype: Concept\ntitle: Alpha\ndescription: Alpha concept.\ntags: [runtime]\nsource_refs: [https://example.com/spec]\ncode_refs: [src.rs]\nrelated: [beta.md]\n---\n\nAlpha.\n",
		);
		write(
			&bundle.join("beta.md"),
			"---\ntype: Concept\ntitle: Beta\ndescription: Beta concept.\n---\n\nBeta.\n",
		);

		let query = OkfQuery {
			code_ref: Some(String::from("src.rs")),
			tags: Vec::new(),
			..OkfQuery::default()
		};
		let matches = docs_okf::query_okf_bundle(&bundle, &query).expect("query");

		assert_eq!(matches.len(), 1);
		assert_eq!(matches[0].id, "alpha");
	}

	#[test]
	fn okf_route_prefers_matching_concepts() {
		let temp_dir = TempDir::new().expect("tempdir");
		let bundle = temp_dir.path().join("bundle");

		fs::create_dir_all(&bundle).expect("bundle");

		write(&bundle.join("index.md"), "# Bundle\n");
		write(
			&bundle.join("okf.md"),
			"---\ntype: Spec\ntitle: OKF Knowledge Layer\ndescription: Command design for portable OKF bundles.\ntags: [okf]\n---\n\nOKF command design.\n",
		);
		write(
			&bundle.join("runtime.md"),
			"---\ntype: Spec\ntitle: Runtime\ndescription: Runtime scheduler.\n---\n\nScheduler.\n",
		);

		let matches = docs_okf::route_okf_bundle(&bundle, "okf command design", 2).expect("route");

		assert_eq!(matches.first().map(|matched| matched.concept.id.as_str()), Some("okf"));
	}

	#[test]
	fn okf_init_scaffolds_repo_memory_bundle_that_passes_check() {
		let temp_dir = TempDir::new().expect("tempdir");
		let bundle = temp_dir.path().join("knowledge");
		let init_report =
			docs_okf::init_okf_bundle(&bundle, OkfCheckProfile::RepoMemory).expect("init");
		let check_report =
			docs_okf::run_okf_check(&bundle, OkfCheckProfile::RepoMemory).expect("check");
		let route_matches = docs_okf::route_okf_bundle(&bundle, "repository knowledge", 3)
			.expect("route initialized bundle");

		assert_eq!(init_report.profile(), OkfCheckProfile::RepoMemory);
		assert_eq!(init_report.created.len(), 3);
		assert!(init_report.unchanged.is_empty());
		assert!(!check_report.has_issues(), "{check_report:#?}");
		assert_eq!(
			route_matches.first().map(|matched| matched.concept.id.as_str()),
			Some("overview")
		);
	}

	#[test]
	fn okf_init_is_idempotent_for_unchanged_scaffold_files() {
		let temp_dir = TempDir::new().expect("tempdir");
		let bundle = temp_dir.path().join("knowledge");

		docs_okf::init_okf_bundle(&bundle, OkfCheckProfile::Wiki).expect("first init");

		let init_report =
			docs_okf::init_okf_bundle(&bundle, OkfCheckProfile::Wiki).expect("second init");

		assert!(init_report.created.is_empty());
		assert_eq!(init_report.unchanged.len(), 3);
	}

	#[test]
	fn okf_init_refuses_to_overwrite_existing_content() {
		let temp_dir = TempDir::new().expect("tempdir");
		let bundle = temp_dir.path().join("knowledge");

		fs::create_dir_all(&bundle).expect("bundle");

		write(&bundle.join("index.md"), "# Existing Index\n");

		let error = docs_okf::init_okf_bundle(&bundle, OkfCheckProfile::Core)
			.expect_err("init should refuse divergent scaffold files");

		assert!(error.to_string().contains("already exists with different content"));
	}

	#[test]
	fn okf_init_preflights_divergence_before_writing_scaffold_files() {
		let temp_dir = TempDir::new().expect("tempdir");
		let bundle = temp_dir.path().join("knowledge");

		fs::create_dir_all(&bundle).expect("bundle");

		write(&bundle.join("overview.md"), "# Existing Overview\n");

		let error = docs_okf::init_okf_bundle(&bundle, OkfCheckProfile::RepoMemory)
			.expect_err("init should refuse before writing partial scaffold files");

		assert!(error.to_string().contains("already exists with different content"));
		assert!(!bundle.join("index.md").exists());
		assert!(!bundle.join("log.md").exists());
		assert_eq!(
			fs::read_to_string(bundle.join("overview.md")).expect("overview"),
			"# Existing Overview\n"
		);
	}

	#[test]
	fn okf_init_rejects_decodex_profile() {
		let temp_dir = TempDir::new().expect("tempdir");
		let bundle = temp_dir.path().join("docs");
		let error = docs_okf::init_okf_bundle(&bundle, OkfCheckProfile::Decodex)
			.expect_err("portable init should not scaffold decodex profile");

		assert!(error.to_string().contains("portable profiles only"));
	}

	fn write_minimal_okf_bundle(docs: &std::path::Path) {
		for lane in ["decisions", "evidence", "reference", "research", "runbook", "spec"] {
			fs::create_dir_all(docs.join(lane)).expect("dirs");
		}

		write(&docs.join("index.md"), "# Docs\n\n* [Policy](policy.md)\n");
		write(&docs.join("log.md"), "# Log\n");
		write(&docs.join("policy.md"), concept("Policy", "Docs policy"));
		write(&docs.join("decisions/index.md"), "# Decisions\n");
		write(&docs.join("evidence/index.md"), "# Evidence\n\n* [Docs drift](docs-drift.md)\n");
		write(&docs.join("evidence/docs-drift.md"), drift_concept("Docs drift"));
		write(&docs.join("reference/index.md"), "# Reference\n");
		write(&docs.join("research/index.json"), research_index_json());
		write(&docs.join("runbook/index.md"), "# Runbooks\n");
		write(&docs.join("spec/index.md"), "# Specs\n");
	}

	fn research_index_json() -> &'static str {
		"{\n  \"schema\": \"decodex.research_index/1\",\n  \"reports\": []\n}\n"
	}

	fn research_report_json(title: &str) -> String {
		format!(
			"{{\n  \"schema\": \"decodex.research_report/1\",\n  \"title\": \"{title}\",\n  \"purpose\": \"Test research report.\",\n  \"scope\": {{}},\n  \"status_summary\": [],\n  \"evidence_ledger\": []\n}}\n"
		)
	}

	fn concept(concept_type: &str, title: &str) -> String {
		format!(
			"---\ntype: {concept_type}\ntitle: {title}\ndescription: Test concept.\nstatus: active\nauthority: non_authoritative\nowner: docs\nlast_verified: 2026-06-16\n---\n\n# Purpose\nTest.\n"
		)
	}

	fn drift_concept(title: &str) -> String {
		format!(
			"---\ntype: Drift Audit\ntitle: {title}\ndescription: Test drift audit.\nstatus: active\nauthority: evidence\nowner: docs\nlast_verified: 2026-06-16\n---\n\n# {title}\n\n## Watched Claims\nTest.\n\n## Evidence Anchors\nTest.\n\n## Reverse Checks\nTest.\n\n## Verdict\npass\n\n## Required Updates\nNone.\n\n## Citations\nNone.\n"
		)
	}

	fn write(path: &std::path::Path, content: impl AsRef<str>) {
		fs::write(path, content.as_ref()).expect("write");
	}
}
