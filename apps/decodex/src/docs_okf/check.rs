//! OKF and Decodex docs check orchestration.

mod docs;
mod okf;

use crate::docs_okf::{
	self, DocsCheckReport, DocsCheckScope, OkfCheckProfile, OkfCheckReport, Path, Result,
};

pub(crate) fn run_docs_check(root: &Path, scope: DocsCheckScope) -> Result<DocsCheckReport> {
	let docs_root = root.to_path_buf();

	if !docs_root.is_dir() {
		color_eyre::eyre::bail!("docs root `{}` does not exist.", docs_root.display());
	}

	let mut files = Vec::new();

	docs_okf::collect_files(&docs_root, &docs_root, &mut files)?;

	let mut report =
		DocsCheckReport { scope, docs_root, concept_count: 0, link_count: 0, issues: Vec::new() };

	if matches!(scope, DocsCheckScope::All | DocsCheckScope::Index | DocsCheckScope::Drift) {
		self::docs::check_required_docs_layout(&files, &mut report);
	}

	self::docs::check_markdown_readability(&files, &mut report);

	if matches!(scope, DocsCheckScope::All | DocsCheckScope::Index) {
		self::docs::check_markdown_only(&files, &mut report);
		self::docs::check_acronym_capitalization(&files, &mut report);
		self::docs::check_concept_contracts(&files, &mut report);
	}
	if matches!(scope, DocsCheckScope::All | DocsCheckScope::Links) {
		self::docs::check_links(&files, &mut report)?;
	}
	if matches!(scope, DocsCheckScope::All | DocsCheckScope::Drift) {
		self::docs::check_drift_surface(&files, &mut report);
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

	docs_okf::collect_files(&bundle_root, &bundle_root, &mut files)?;

	let mut report = OkfCheckReport {
		profile,
		bundle_root,
		concept_count: 0,
		link_count: 0,
		issues: Vec::new(),
	};

	self::okf::check_okf_markdown_readability(&files, &mut report);
	self::okf::check_okf_core_concepts(&files, &mut report);

	if matches!(profile, OkfCheckProfile::Wiki | OkfCheckProfile::RepoMemory) {
		self::okf::check_okf_wiki_surface(&files, &mut report)?;
	}
	if profile == OkfCheckProfile::RepoMemory {
		self::okf::check_okf_repo_memory_surface(&files, &mut report);
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

fn decodex_docs_report_as_okf(report: DocsCheckReport) -> OkfCheckReport {
	OkfCheckReport {
		profile: OkfCheckProfile::Decodex,
		bundle_root: report.docs_root,
		concept_count: report.concept_count,
		link_count: report.link_count,
		issues: report.issues,
	}
}
