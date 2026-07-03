use crate::docs_okf::{self, DocsCheckReport, DocsFile};

pub(in crate::docs_okf::check) fn check_markdown_readability(
	files: &[DocsFile],
	report: &mut DocsCheckReport,
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

pub(in crate::docs_okf::check) fn check_acronym_capitalization(
	files: &[DocsFile],
	report: &mut DocsCheckReport,
) {
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
