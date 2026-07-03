use crate::docs_okf::{
	self, DocsFile, OkfCheckReport, Path, PathBuf, Regex, Result, check::okf::frontmatter,
};

pub(in crate::docs_okf::check) fn check_okf_wiki_surface(
	files: &[DocsFile],
	report: &mut OkfCheckReport,
) -> Result<()> {
	check_okf_indexes(files, report);
	check_okf_wiki_frontmatter(files, report);
	check_okf_links(files, report)?;

	Ok(())
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
		let Some(fields) = frontmatter::okf_frontmatter_fields(file, report) else {
			continue;
		};

		frontmatter::read_required_okf_frontmatter_string(
			&fields,
			"title",
			&file.relative_path,
			report,
		);
		frontmatter::read_required_okf_frontmatter_string(
			&fields,
			"description",
			&file.relative_path,
			report,
		);
		frontmatter::okf_frontmatter_string_list(&fields, "tags", &file.relative_path, report);
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
