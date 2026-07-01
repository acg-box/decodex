//! OKF and Decodex docs check orchestration.

#[allow(clippy::wildcard_imports)] use super::*;

mod docs;
mod okf;

use self::{
	docs::{
		check_acronym_capitalization, check_concept_contracts, check_drift_surface, check_links,
		check_markdown_only, check_markdown_readability, check_required_docs_layout,
	},
	okf::{
		check_okf_core_concepts, check_okf_markdown_readability, check_okf_repo_memory_surface,
		check_okf_wiki_surface,
	},
};

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

fn decodex_docs_report_as_okf(report: DocsCheckReport) -> OkfCheckReport {
	OkfCheckReport {
		profile: OkfCheckProfile::Decodex,
		bundle_root: report.docs_root,
		concept_count: report.concept_count,
		link_count: report.link_count,
		issues: report.issues,
	}
}
