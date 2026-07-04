const STYLE_PARTS: &[&str] = &[
	include_str!(concat!(
		env!("CARGO_MANIFEST_DIR"),
		"/src/orchestrator/operator_dashboard/styles/activity.css"
	)),
	include_str!(concat!(
		env!("CARGO_MANIFEST_DIR"),
		"/src/orchestrator/operator_dashboard/styles/activity/child-layout.css"
	)),
	include_str!(concat!(
		env!("CARGO_MANIFEST_DIR"),
		"/src/orchestrator/operator_dashboard/styles/activity/child-metrics.css"
	)),
	include_str!(concat!(
		env!("CARGO_MANIFEST_DIR"),
		"/src/orchestrator/operator_dashboard/styles/activity/status-tone-grid.css"
	)),
];

pub(super) fn append_style_parts(html: &mut String) {
	for style in STYLE_PARTS {
		html.push_str(style);
	}
}
