use crate::docs_okf::{self, DocsFile, OkfCheckReport, check::okf::frontmatter};

pub(in crate::docs_okf::check) fn check_okf_markdown_readability(
	files: &[DocsFile],
	report: &mut OkfCheckReport,
) {
	for file in files {
		if let Some(read_error) = &file.read_error {
			report.issues.push(docs_okf::issue(
				Some(file.relative_path.clone()),
				format!("Markdown file must be UTF-8 readable: {read_error}"),
			));
		}
	}
}

pub(in crate::docs_okf::check) fn check_okf_core_concepts(
	files: &[DocsFile],
	report: &mut OkfCheckReport,
) {
	for file in files.iter().filter(|file| docs_okf::is_concept_markdown(&file.relative_path)) {
		report.concept_count += 1;

		let Some(content) = file.content.as_deref() else {
			continue;
		};
		let Some((frontmatter_text, _)) = docs_okf::split_yaml_frontmatter(content) else {
			report.issues.push(docs_okf::issue(
				Some(file.relative_path.clone()),
				String::from("concept must start with YAML frontmatter delimited by ---"),
			));

			continue;
		};
		let Some(fields) = frontmatter::parse_okf_frontmatter_mapping(
			frontmatter_text,
			&file.relative_path,
			report,
		) else {
			continue;
		};

		frontmatter::read_required_okf_frontmatter_string(
			&fields,
			"type",
			&file.relative_path,
			report,
		);
	}
}
