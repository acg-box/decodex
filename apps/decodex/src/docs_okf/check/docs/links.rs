use crate::docs_okf::{self, DocsCheckReport, DocsFile, Regex, Result};

pub(in crate::docs_okf::check) fn check_links(
	files: &[DocsFile],
	report: &mut DocsCheckReport,
) -> Result<()> {
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
