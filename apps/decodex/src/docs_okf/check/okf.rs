use crate::docs_okf::{
	self, DocsFile, Mapping, OkfCheckReport, Path, PathBuf, Regex, Result,
	serde_yaml::{self, Value},
};

pub(super) fn check_okf_markdown_readability(files: &[DocsFile], report: &mut OkfCheckReport) {
	for file in files {
		if let Some(read_error) = &file.read_error {
			report.issues.push(docs_okf::issue(
				Some(file.relative_path.clone()),
				format!("Markdown file must be UTF-8 readable: {read_error}"),
			));
		}
	}
}

pub(super) fn check_okf_core_concepts(files: &[DocsFile], report: &mut OkfCheckReport) {
	for file in files.iter().filter(|file| docs_okf::is_concept_markdown(&file.relative_path)) {
		report.concept_count += 1;

		let Some(content) = file.content.as_deref() else {
			continue;
		};
		let Some((frontmatter, _)) = docs_okf::split_yaml_frontmatter(content) else {
			report.issues.push(docs_okf::issue(
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

pub(super) fn check_okf_wiki_surface(
	files: &[DocsFile],
	report: &mut OkfCheckReport,
) -> Result<()> {
	check_okf_indexes(files, report);
	check_okf_wiki_frontmatter(files, report);
	check_okf_links(files, report)?;

	Ok(())
}

pub(super) fn check_okf_repo_memory_surface(files: &[DocsFile], report: &mut OkfCheckReport) {
	for file in files.iter().filter(|file| docs_okf::is_concept_markdown(&file.relative_path)) {
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

fn check_okf_indexes(files: &[DocsFile], report: &mut OkfCheckReport) {
	let paths = docs_okf::file_path_set(files);

	if !paths.contains(Path::new("index.md")) {
		report.issues.push(docs_okf::issue(
			Some(PathBuf::from("index.md")),
			String::from("wiki profile expects a root progressive-disclosure index.md"),
		));
	}

	for dir in docs_okf::docs_dirs_with_content(files) {
		let index_path = dir.join("index.md");

		if !paths.contains(&index_path) {
			report.issues.push(docs_okf::issue(
				Some(index_path),
				String::from("wiki profile expects each populated directory to have index.md"),
			));
		}
	}
}

fn check_okf_wiki_frontmatter(files: &[DocsFile], report: &mut OkfCheckReport) {
	for file in files.iter().filter(|file| docs_okf::is_concept_markdown(&file.relative_path)) {
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

	for file in files.iter().filter(|file| docs_okf::is_markdown(&file.relative_path)) {
		let Some(content) = file.content.as_deref() else {
			continue;
		};

		for captures in link_pattern.captures_iter(content) {
			let Some(target_match) = captures.get(1) else {
				continue;
			};
			let target = target_match.as_str();

			if docs_okf::should_skip_link_target(target) {
				continue;
			}

			report.link_count += 1;

			if let Some(link_path) =
				docs_okf::resolve_link_target(&file.path, &report.bundle_root, target)
				&& !link_path.exists()
			{
				report.issues.push(docs_okf::issue(
					Some(file.relative_path.clone()),
					format!("link target `{target}` does not exist"),
				));
			}
		}
	}

	Ok(())
}

fn okf_frontmatter_fields(file: &DocsFile, report: &mut OkfCheckReport) -> Option<Mapping> {
	let content = file.content.as_deref()?;
	let Some((frontmatter, _)) = docs_okf::split_yaml_frontmatter(content) else {
		report.issues.push(docs_okf::issue(
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
		Ok(serde_yaml::Value::Mapping(mapping)) => Some(mapping),
		Ok(_) => {
			report.issues.push(docs_okf::issue(
				Some(path.to_path_buf()),
				String::from("frontmatter must be a YAML mapping"),
			));

			None
		},
		Err(error) => {
			report.issues.push(docs_okf::issue(
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
	match docs_okf::frontmatter_value(fields, key) {
		Some(serde_yaml::Value::String(value)) if !value.trim().is_empty() => {},
		Some(serde_yaml::Value::String(_)) | None => report.issues.push(docs_okf::issue(
			Some(path.to_path_buf()),
			format!("frontmatter key `{key}` is required and must be non-empty"),
		)),
		Some(_) => report.issues.push(docs_okf::issue(
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
	match docs_okf::frontmatter_value(fields, key) {
		None => None,
		Some(serde_yaml::Value::Sequence(items)) => {
			let mut values = Vec::new();

			for item in items {
				match item {
					serde_yaml::Value::String(value) if !value.trim().is_empty() => {
						values.push(value.trim().to_owned());
					},
					serde_yaml::Value::String(_) => report.issues.push(docs_okf::issue(
						Some(path.to_path_buf()),
						format!("frontmatter list `{key}` must not contain empty strings"),
					)),
					_ => report.issues.push(docs_okf::issue(
						Some(path.to_path_buf()),
						format!("frontmatter list `{key}` must contain only strings"),
					)),
				}
			}

			Some(values)
		},
		Some(_) => {
			report.issues.push(docs_okf::issue(
				Some(path.to_path_buf()),
				format!("frontmatter key `{key}` must be a list of strings"),
			));

			None
		},
	}
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
