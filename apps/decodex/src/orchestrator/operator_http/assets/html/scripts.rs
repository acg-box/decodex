mod accounts;
mod activity;
mod boot;
mod formatting;
mod lanes;
mod overview;
mod render_primitives;
mod stream;

pub(super) fn append_script_parts(html: &mut String) {
	for scripts in [
		boot::SCRIPT_PARTS,
		formatting::SCRIPT_PARTS,
		render_primitives::SCRIPT_PARTS,
		accounts::SCRIPT_PARTS,
		activity::SCRIPT_PARTS,
		overview::SCRIPT_PARTS,
		lanes::SCRIPT_PARTS,
		stream::SCRIPT_PARTS,
	] {
		for script in scripts {
			html.push_str(script);
		}
	}
}
