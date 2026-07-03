mod markup;
mod scripts;
mod styles;

use std::sync::LazyLock;

use self::{
	markup::{BODY, HEAD, TAIL},
	scripts::SCRIPT_PARTS,
	styles::STYLE_PARTS,
};

pub(in crate::orchestrator::operator_http) static OPERATOR_DASHBOARD_HTML: LazyLock<String> =
	LazyLock::new(|| {
		let mut html = String::new();

		html.push_str(HEAD);
		for style in STYLE_PARTS {
			html.push_str(style);
		}
		html.push_str(BODY);
		for script in SCRIPT_PARTS {
			html.push_str(script);
		}
		html.push_str(TAIL);

		html
	});
