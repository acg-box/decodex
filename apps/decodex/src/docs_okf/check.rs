//! OKF and Decodex docs check orchestration.

#[allow(clippy::wildcard_imports)]
use super::*;

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

fn parse_frontmatter_mapping(
	frontmatter: &str,
	path: &Path,
	report: &mut DocsCheckReport,
) -> Option<Mapping> {
	match serde_yaml::from_str::<serde_yaml::Value>(frontmatter) {
		Ok(serde_yaml::Value::Mapping(mapping)) => Some(mapping),
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
	match serde_yaml::from_str::<serde_yaml::Value>(frontmatter) {
		Ok(serde_yaml::Value::Mapping(mapping)) => Some(mapping),
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
		Some(serde_yaml::Value::String(value)) if !value.trim().is_empty() => {},
		Some(serde_yaml::Value::String(_)) | None => report.issues.push(issue(
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
		Some(serde_yaml::Value::Sequence(items)) => {
			let mut values = Vec::new();

			for item in items {
				match item {
					serde_yaml::Value::String(value) if !value.trim().is_empty() => {
						values.push(value.trim().to_owned());
					},
					serde_yaml::Value::String(_) => report.issues.push(issue(
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

fn frontmatter_value<'a>(fields: &'a Mapping, key: &str) -> Option<&'a serde_yaml::Value> {
	fields.get(serde_yaml::Value::String(key.to_owned()))
}

fn frontmatter_string<'a>(fields: &'a Mapping, key: &str) -> Option<&'a str> {
	match frontmatter_value(fields, key) {
		Some(serde_yaml::Value::String(value)) => Some(value.trim()),
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
		Some(serde_yaml::Value::Sequence(items)) => {
			let mut values = Vec::new();

			for item in items {
				match item {
					serde_yaml::Value::String(value) if !value.trim().is_empty() => {
						values.push(value.trim().to_owned());
					},
					serde_yaml::Value::String(_) => report.issues.push(issue(
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
		Some(serde_yaml::Value::String(value)) if !value.trim().is_empty() => {},
		Some(serde_yaml::Value::String(_)) | None => report.issues.push(issue(
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
