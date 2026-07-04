const STYLE_PARTS: &[&str] = &[
	include_str!(concat!(
		env!("CARGO_MANIFEST_DIR"),
		"/src/orchestrator/operator_dashboard/styles/responsive.css"
	)),
	include_str!(concat!(
		env!("CARGO_MANIFEST_DIR"),
		"/src/orchestrator/operator_dashboard/styles/responsive/wide.css"
	)),
	include_str!(concat!(
		env!("CARGO_MANIFEST_DIR"),
		"/src/orchestrator/operator_dashboard/styles/responsive/medium.css"
	)),
	include_str!(concat!(
		env!("CARGO_MANIFEST_DIR"),
		"/src/orchestrator/operator_dashboard/styles/responsive/mobile.css"
	)),
	include_str!(concat!(
		env!("CARGO_MANIFEST_DIR"),
		"/src/orchestrator/operator_dashboard/styles/responsive/motion.css"
	)),
];

pub(super) fn append_style_parts(html: &mut String) {
	for style in STYLE_PARTS {
		html.push_str(style);
	}
}
