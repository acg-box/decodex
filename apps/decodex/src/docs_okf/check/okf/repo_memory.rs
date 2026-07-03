use crate::docs_okf::{self, DocsFile, Mapping, OkfCheckReport, Path, check::okf::frontmatter};

pub(in crate::docs_okf::check) fn check_okf_repo_memory_surface(
	files: &[DocsFile],
	report: &mut OkfCheckReport,
) {
	for file in files.iter().filter(|file| docs_okf::is_concept_markdown(&file.relative_path)) {
		let Some(fields) = frontmatter::okf_frontmatter_fields(file, report) else {
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

fn validate_repo_memory_frontmatter_fields(
	fields: &Mapping,
	path: &Path,
	bundle_root: &Path,
	report: &mut OkfCheckReport,
) {
	for key in ["tags", "drift_watch"] {
		frontmatter::okf_frontmatter_string_list(fields, key, path, report);
	}

	validate_okf_source_refs(fields, path, report);
	validate_okf_code_refs(fields, path, bundle_root, report);
	validate_okf_related_refs(fields, path, bundle_root, report);
}

fn validate_okf_source_refs(fields: &Mapping, path: &Path, report: &mut OkfCheckReport) {
	let Some(values) =
		frontmatter::okf_frontmatter_string_list(fields, "source_refs", path, report)
	else {
		return;
	};

	for value in values {
		if !docs_okf::is_http_url(&value) {
			report.issues.push(docs_okf::issue(
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
	let Some(values) = frontmatter::okf_frontmatter_string_list(fields, "code_refs", path, report)
	else {
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
		report.issues.push(docs_okf::issue(
			Some(path.to_path_buf()),
			format!("code_refs entry `{value}` must be a file path without fragments"),
		));

		return;
	}
	if !docs_okf::is_normalized_relative_path(value_path) {
		report.issues.push(docs_okf::issue(
			Some(path.to_path_buf()),
			format!("code_refs entry `{value}` must be a normalized repository-relative file path"),
		));

		return;
	}

	let target_path = docs_okf::normalize_path(&repo_root.join(value_path));

	if !target_path.exists() {
		report.issues.push(docs_okf::issue(
			Some(path.to_path_buf()),
			format!("code_refs entry `{value}` does not exist"),
		));
	} else if !target_path.is_file() {
		report.issues.push(docs_okf::issue(
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
	let Some(values) = frontmatter::okf_frontmatter_string_list(fields, "related", path, report)
	else {
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
	let target = docs_okf::strip_fragment(value);

	if target.is_empty() {
		report.issues.push(docs_okf::issue(
			Some(path.to_path_buf()),
			format!("related entry `{value}` must include a bundle file path"),
		));

		return;
	}

	let target_value_path = Path::new(target);

	if target_value_path.is_absolute() {
		report.issues.push(docs_okf::issue(
			Some(path.to_path_buf()),
			format!("related entry `{value}` must be a bundle-relative file path"),
		));

		return;
	}

	let parent = path.parent().unwrap_or_else(|| Path::new(""));
	let target_path = docs_okf::normalize_path(&bundle_root.join(parent).join(target_value_path));

	if !target_path.starts_with(bundle_root) {
		report.issues.push(docs_okf::issue(
			Some(path.to_path_buf()),
			format!("related entry `{value}` must stay under the OKF bundle"),
		));

		return;
	}
	if !target_path.exists() {
		report.issues.push(docs_okf::issue(
			Some(path.to_path_buf()),
			format!("related entry `{value}` does not exist"),
		));
	} else if !target_path.is_file() || !docs_okf::is_markdown(&target_path) {
		report.issues.push(docs_okf::issue(
			Some(path.to_path_buf()),
			format!("related entry `{value}` must reference a Markdown file"),
		));
	}
}
