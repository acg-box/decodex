pub(super) const HEAD: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/src/orchestrator/operator_dashboard/head.html"
));
pub(super) const BODY: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/src/orchestrator/operator_dashboard/body.html"
));
pub(super) const TAIL: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/src/orchestrator/operator_dashboard/tail.html"
));
