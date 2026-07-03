use crate::docs_okf::{self, DocsCheckReport, DocsFile, Path, REQUIRED_DOCS_FILES};

pub(in crate::docs_okf::check) fn check_required_docs_layout(
	files: &[DocsFile],
	report: &mut DocsCheckReport,
) {
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

pub(in crate::docs_okf::check) fn check_markdown_only(
	files: &[DocsFile],
	report: &mut DocsCheckReport,
) {
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
