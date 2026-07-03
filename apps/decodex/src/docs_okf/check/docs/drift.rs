use crate::docs_okf::{self, DocsCheckReport, DocsFile, PathBuf};

pub(in crate::docs_okf::check) fn check_drift_surface(
	files: &[DocsFile],
	report: &mut DocsCheckReport,
) {
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
