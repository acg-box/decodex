mod scripts;
mod styles;

use std::sync::LazyLock;

const HEAD: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/src/orchestrator/operator_dashboard/head.html"
));
const BODY: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/src/orchestrator/operator_dashboard/body.html"
));
const TAIL: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/src/orchestrator/operator_dashboard/tail.html"
));

pub(in crate::orchestrator::operator_http) static OPERATOR_DASHBOARD_HTML: LazyLock<String> =
	LazyLock::new(|| {
		let mut html = String::new();

		html.push_str(HEAD);
		styles::append_style_parts(&mut html);
		html.push_str(BODY);
		self::scripts::append_script_parts(&mut html);
		html.push_str(TAIL);

		html
	});
