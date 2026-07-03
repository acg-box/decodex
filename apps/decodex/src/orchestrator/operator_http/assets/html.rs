mod markup;
mod scripts;
mod styles;

use std::sync::LazyLock;

use self::{
	markup::{BODY, HEAD, TAIL},
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
		self::scripts::append_script_parts(&mut html);
		html.push_str(TAIL);

		html
	});
