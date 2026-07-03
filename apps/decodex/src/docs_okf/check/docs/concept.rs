use crate::docs_okf::{
	self, ALLOWED_AUTHORITIES, ALLOWED_CONCEPT_TYPES, ALLOWED_STATUSES, DocsCheckReport, DocsFile,
	REQUIRED_CONCEPT_KEYS,
	check::docs::{frontmatter, headings, references},
};

pub(in crate::docs_okf::check) fn check_concept_contracts(
	files: &[DocsFile],
	report: &mut DocsCheckReport,
) {
	for file in files.iter().filter(|file| docs_okf::is_concept_markdown(&file.relative_path)) {
		report.concept_count += 1;

		let Some(content) = file.content.as_deref() else {
			continue;
		};
		let Some((frontmatter_text, body)) = docs_okf::split_yaml_frontmatter(content) else {
			report.issues.push(docs_okf::issue(
				Some(file.relative_path.clone()),
				String::from("concept must start with YAML frontmatter delimited by ---"),
			));

			continue;
		};
		let Some(fields) =
			frontmatter::parse_frontmatter_mapping(frontmatter_text, &file.relative_path, report)
		else {
			continue;
		};

		for required_key in REQUIRED_CONCEPT_KEYS {
			frontmatter::read_required_frontmatter_string(
				&fields,
				required_key,
				&file.relative_path,
				report,
			);
		}

		frontmatter::validate_frontmatter_enum(
			&fields,
			"type",
			ALLOWED_CONCEPT_TYPES,
			&file.relative_path,
			report,
		);
		frontmatter::validate_frontmatter_enum(
			&fields,
			"status",
			ALLOWED_STATUSES,
			&file.relative_path,
			report,
		);
		frontmatter::validate_frontmatter_enum(
			&fields,
			"authority",
			ALLOWED_AUTHORITIES,
			&file.relative_path,
			report,
		);
		frontmatter::validate_frontmatter_date(&fields, &file.relative_path, report);
		headings::validate_type_specific_headings(&fields, body, &file.relative_path, report);
		references::validate_structured_frontmatter_fields(
			&fields,
			&file.relative_path,
			&report.docs_root.clone(),
			report,
		);
	}
}
