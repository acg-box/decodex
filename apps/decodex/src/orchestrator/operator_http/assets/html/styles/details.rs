const STYLE_PARTS: &[&str] = &[
	include_str!(concat!(
		env!("CARGO_MANIFEST_DIR"),
		"/src/orchestrator/operator_dashboard/styles/details.css"
	)),
	include_str!(concat!(
		env!("CARGO_MANIFEST_DIR"),
		"/src/orchestrator/operator_dashboard/styles/details/fields.css"
	)),
	include_str!(concat!(
		env!("CARGO_MANIFEST_DIR"),
		"/src/orchestrator/operator_dashboard/styles/details/disclosure.css"
	)),
	include_str!(concat!(
		env!("CARGO_MANIFEST_DIR"),
		"/src/orchestrator/operator_dashboard/styles/details/phases.css"
	)),
	include_str!(concat!(
		env!("CARGO_MANIFEST_DIR"),
		"/src/orchestrator/operator_dashboard/styles/details/fold-panels.css"
	)),
	include_str!(concat!(
		env!("CARGO_MANIFEST_DIR"),
		"/src/orchestrator/operator_dashboard/styles/details/empty-state.css"
	)),
];

pub(super) fn append_style_parts(html: &mut String) {
	for style in STYLE_PARTS {
		html.push_str(style);
	}
}
