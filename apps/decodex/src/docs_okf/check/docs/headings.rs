use crate::docs_okf::{
	self, BTreeSet, DRIFT_AUDIT_HEADINGS, DocsCheckReport, Mapping, Path,
	RESEARCH_CONTRACT_HEADINGS,
};

pub(in crate::docs_okf::check::docs) fn validate_type_specific_headings(
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
