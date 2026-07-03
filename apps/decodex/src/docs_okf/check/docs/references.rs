use crate::docs_okf::{
	self, ALLOWED_PROMOTION_TARGETS, DocsCheckReport, Mapping, Path, check::docs::frontmatter,
};

pub(in crate::docs_okf::check::docs) fn validate_structured_frontmatter_fields(
	fields: &Mapping,
	path: &Path,
	docs_root: &Path,
	report: &mut DocsCheckReport,
) {
	for key in ["tags", "drift_watch"] {
		frontmatter::frontmatter_string_list(fields, key, path, report);
	}

	validate_source_refs(fields, path, report);
	validate_code_refs(fields, path, docs_root, report);
	validate_related_refs(fields, path, docs_root, report);
	validate_promotes_to(fields, path, report);
}

fn validate_source_refs(fields: &Mapping, path: &Path, report: &mut DocsCheckReport) {
	let Some(values) = frontmatter::frontmatter_string_list(fields, "source_refs", path, report)
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

fn validate_code_refs(
	fields: &Mapping,
	path: &Path,
	docs_root: &Path,
	report: &mut DocsCheckReport,
) {
	let Some(values) = frontmatter::frontmatter_string_list(fields, "code_refs", path, report)
	else {
		return;
	};
	let repo_root = docs_root.parent().unwrap_or(docs_root);

	for value in values {
		let value_path = Path::new(&value);

		if value.contains('#') || value.contains('?') {
			report.issues.push(docs_okf::issue(
				Some(path.to_path_buf()),
				format!("code_refs entry `{value}` must be a file path without fragments"),
			));

			continue;
		}
		if !docs_okf::is_normalized_relative_path(value_path) {
			report.issues.push(docs_okf::issue(
				Some(path.to_path_buf()),
				format!(
					"code_refs entry `{value}` must be a normalized repository-relative file path"
				),
			));

			continue;
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
}

fn validate_related_refs(
	fields: &Mapping,
	path: &Path,
	docs_root: &Path,
	report: &mut DocsCheckReport,
) {
	let Some(values) = frontmatter::frontmatter_string_list(fields, "related", path, report) else {
		return;
	};

	for value in values {
		let target = docs_okf::strip_fragment(&value);

		if target.is_empty() {
			report.issues.push(docs_okf::issue(
				Some(path.to_path_buf()),
				format!("related entry `{value}` must include a docs file path"),
			));

			continue;
		}

		let target_value_path = Path::new(target);

		if target_value_path.is_absolute() {
			report.issues.push(docs_okf::issue(
				Some(path.to_path_buf()),
				format!("related entry `{value}` must be a docs-relative file path"),
			));

			continue;
		}

		let relative_target = target.strip_prefix("docs/").unwrap_or(target);
		let target_path = if target.starts_with("docs/") {
			docs_okf::normalize_path(&docs_root.join(relative_target))
		} else {
			let parent = path.parent().unwrap_or_else(|| Path::new(""));

			docs_okf::normalize_path(&docs_root.join(parent).join(relative_target))
		};

		if !target_path.starts_with(docs_root) {
			report.issues.push(docs_okf::issue(
				Some(path.to_path_buf()),
				format!("related entry `{value}` must stay under docs/"),
			));

			continue;
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
}

fn validate_promotes_to(fields: &Mapping, path: &Path, report: &mut DocsCheckReport) {
	let Some(values) = frontmatter::frontmatter_string_list(fields, "promotes_to", path, report)
	else {
		return;
	};

	for value in values {
		if !ALLOWED_PROMOTION_TARGETS.contains(&value.as_str()) {
			report.issues.push(docs_okf::issue(
				Some(path.to_path_buf()),
				format!("promotes_to entry `{value}` is not an authoritative promotion lane"),
			));
		}
	}
}
