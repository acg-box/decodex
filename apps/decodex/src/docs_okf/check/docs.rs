use crate::docs_okf::{
	self, ALLOWED_AUTHORITIES, ALLOWED_CONCEPT_TYPES, ALLOWED_PROMOTION_TARGETS, ALLOWED_STATUSES,
	BTreeSet, DRIFT_AUDIT_HEADINGS, DocsCheckReport, DocsFile, Mapping, Path, PathBuf,
	REQUIRED_CONCEPT_KEYS, REQUIRED_DOCS_FILES, RESEARCH_CONTRACT_HEADINGS, Regex, Result,
	serde_yaml::{self, Value},
};

pub(super) fn check_required_docs_layout(files: &[DocsFile], report: &mut DocsCheckReport) {
	let paths = docs_okf::file_path_set(files);

	for required in REQUIRED_DOCS_FILES {
		if !paths.contains(Path::new(required)) {
			report
				.issues
				.push(docs_okf::issue(None, format!("required docs file `{required}` is missing")));
		}
	}

	let dirs = docs_okf::docs_dirs_with_content(files);

	for dir in dirs {
		let index_path = dir.join("index.md");

		if !paths.contains(&index_path) {
			report.issues.push(docs_okf::issue(
				Some(index_path),
				String::from("directory must have an OKF progressive-disclosure index.md"),
			));
		}
	}
}

pub(super) fn check_markdown_readability(files: &[DocsFile], report: &mut DocsCheckReport) {
	for file in files {
		if let Some(read_error) = &file.read_error {
			report.issues.push(docs_okf::issue(
				Some(file.relative_path.clone()),
				format!("Markdown file must be UTF-8 readable: {read_error}"),
			));
		}
	}
}

pub(super) fn check_markdown_only(files: &[DocsFile], report: &mut DocsCheckReport) {
	for file in files {
		if !docs_okf::is_markdown(&file.relative_path) {
			let message = if file.path.extension().is_some_and(|extension| extension == "json") {
				"docs/ must be OKF Markdown-only; JSON artifacts are not allowed"
			} else {
				"docs/ must be OKF Markdown-only; only .md files are allowed"
			};

			report
				.issues
				.push(docs_okf::issue(Some(file.relative_path.clone()), String::from(message)));
		}
	}
}

pub(super) fn check_acronym_capitalization(files: &[DocsFile], report: &mut DocsCheckReport) {
	for file in files.iter().filter(|file| docs_okf::is_markdown(&file.relative_path)) {
		let Some(content) = file.content.as_deref() else {
			continue;
		};

		if content.contains("Okf") {
			report.issues.push(docs_okf::issue(
				Some(file.relative_path.clone()),
				String::from(
					"use `OKF` in prose; lowercase `okf` is reserved for paths, slugs, tags, and URLs",
				),
			));
		}
	}
}

pub(super) fn check_concept_contracts(files: &[DocsFile], report: &mut DocsCheckReport) {
	for file in files.iter().filter(|file| docs_okf::is_concept_markdown(&file.relative_path)) {
		report.concept_count += 1;

		let Some(content) = file.content.as_deref() else {
			continue;
		};
		let Some((frontmatter, body)) = docs_okf::split_yaml_frontmatter(content) else {
			report.issues.push(docs_okf::issue(
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

pub(super) fn check_links(files: &[DocsFile], report: &mut DocsCheckReport) -> Result<()> {
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
				docs_okf::resolve_link_target(&file.path, &report.docs_root, target)
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

pub(super) fn check_drift_surface(files: &[DocsFile], report: &mut DocsCheckReport) {
	let has_drift_concept = files.iter().any(|file| {
		docs_okf::is_concept_markdown(&file.relative_path)
			&& docs_okf::concept_type(file)
				.is_some_and(|concept_type| concept_type == "Drift Audit")
	});

	if !has_drift_concept {
		report.issues.push(docs_okf::issue(
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

fn frontmatter_string_list(
	fields: &Mapping,
	key: &str,
	path: &Path,
	report: &mut DocsCheckReport,
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

fn read_required_frontmatter_string(
	fields: &Mapping,
	key: &str,
	path: &Path,
	report: &mut DocsCheckReport,
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

fn validate_frontmatter_enum(
	fields: &Mapping,
	key: &str,
	allowed_values: &[&str],
	path: &Path,
	report: &mut DocsCheckReport,
) {
	let Some(value) = docs_okf::frontmatter_string(fields, key) else {
		return;
	};

	if !value.is_empty() && !allowed_values.contains(&value) {
		report.issues.push(docs_okf::issue(
			Some(path.to_path_buf()),
			format!("frontmatter key `{key}` has unsupported value `{value}`"),
		));
	}
}

fn validate_frontmatter_date(fields: &Mapping, path: &Path, report: &mut DocsCheckReport) {
	let Some(value) = docs_okf::frontmatter_string(fields, "last_verified") else {
		return;
	};

	if !value.is_empty() && !docs_okf::is_valid_iso_date(value) {
		report.issues.push(docs_okf::issue(
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
	let Some(values) = frontmatter_string_list(fields, "code_refs", path, report) else {
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
	let Some(values) = frontmatter_string_list(fields, "related", path, report) else {
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
	let Some(values) = frontmatter_string_list(fields, "promotes_to", path, report) else {
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

fn validate_type_specific_headings(
	fields: &Mapping,
	body: &str,
	path: &Path,
	report: &mut DocsCheckReport,
) {
	let Some(concept_type) = docs_okf::frontmatter_string(fields, "type") else {
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
			report.issues.push(docs_okf::issue(
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
