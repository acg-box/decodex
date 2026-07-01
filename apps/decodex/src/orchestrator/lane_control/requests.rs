use std::path::Path;

#[derive(Clone, Copy)]
pub(crate) struct LaneInspectRequest<'a> {
	pub(crate) config_path: Option<&'a Path>,
	pub(crate) issue: &'a str,
	pub(crate) run_id: Option<&'a str>,
	pub(crate) json: bool,
}

#[derive(Clone, Copy)]
pub(crate) struct LaneInterruptRequest<'a> {
	pub(crate) config_path: Option<&'a Path>,
	pub(crate) issue: &'a str,
	pub(crate) run_id: &'a str,
	pub(crate) force: bool,
	pub(crate) reason: Option<&'a str>,
	pub(crate) json: bool,
	pub(crate) source: &'a str,
}
