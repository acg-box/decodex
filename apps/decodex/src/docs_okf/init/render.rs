use crate::docs_okf::OkfInitReport;

/// Render a stable human-readable OKF init report.
pub(crate) fn render_okf_init_report(report: &OkfInitReport) -> String {
	let mut output = String::new();

	output.push_str(&format!(
		"okf init: profile={} root={} created={} unchanged={}\n",
		report.profile().as_str(),
		report.bundle_root.display(),
		report.created.len(),
		report.unchanged.len()
	));

	for path in &report.created {
		output.push_str(&format!("- created {}\n", path.display()));
	}
	for path in &report.unchanged {
		output.push_str(&format!("- unchanged {}\n", path.display()));
	}

	output.push_str(&format!(
		"next: decodex okf check {} --profile {}\n",
		report.bundle_root.display(),
		report.profile.as_str()
	));

	output
}
