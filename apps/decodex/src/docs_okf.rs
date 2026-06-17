//! OKF-style documentation validation for the repository docs bundle.

use std::{
	collections::BTreeSet,
	fs,
	path::{Component, Path, PathBuf},
};

use regex::Regex;
use reqwest::Url;
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
	"research/index.md",
	"runbook/index.md",
	"spec/index.md",
];
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

/// Validate a docs directory as a Markdown-only OKF bundle.
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
		check_markdown_only(&files, &mut report);
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

fn check_required_docs_layout(files: &[DocsFile], report: &mut DocsCheckReport) {
	let paths = file_path_set(files);

	for required in REQUIRED_DOCS_FILES {
		if !paths.contains(Path::new(required)) {
			report.issues.push(issue(None, format!("required docs file `{required}` is missing")));
		}
	}

	let dirs = docs_dirs_with_content(files);

	for dir in dirs {
		let index_path = dir.join("index.md");

		if !paths.contains(&index_path) {
			report.issues.push(issue(
				Some(index_path),
				String::from("directory must have an OKF progressive-disclosure index.md"),
			));
		}
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

fn check_markdown_only(files: &[DocsFile], report: &mut DocsCheckReport) {
	for file in files {
		if !is_markdown(&file.relative_path) {
			let message = if file.path.extension().is_some_and(|extension| extension == "json") {
				"docs/ must be OKF Markdown-only; JSON artifacts are not allowed"
			} else {
				"docs/ must be OKF Markdown-only; only .md files are allowed"
			};

			report.issues.push(issue(Some(file.relative_path.clone()), String::from(message)));
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

	use crate::docs_okf::{self, DocsCheckScope};

	#[test]
	fn docs_check_rejects_json_artifacts() {
		let temp_dir = TempDir::new().expect("tempdir");
		let docs = temp_dir.path().join("docs");

		write_minimal_okf_bundle(&docs);
		write(&docs.join("research.json"), "{}\n");

		let report = docs_okf::run_docs_check(&docs, DocsCheckScope::All).expect("check");

		assert!(report.has_issues());
		assert!(report.issues.iter().any(|issue| issue.message.contains("JSON artifacts")));
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
		assert!(report.issues.iter().any(|issue| issue.message.contains("only .md files")));
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
		write(&docs.join("research/index.md"), "# Research\n");
		write(&docs.join("runbook/index.md"), "# Runbooks\n");
		write(&docs.join("spec/index.md"), "# Specs\n");
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
