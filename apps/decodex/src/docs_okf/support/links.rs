use crate::docs_okf::{self, Path, PathBuf};

pub(in crate::docs_okf) fn should_skip_link_target(target: &str) -> bool {
	target.starts_with('#')
		|| target.starts_with("http://")
		|| target.starts_with("https://")
		|| target.starts_with("mailto:")
		|| target.starts_with("tel:")
}

pub(in crate::docs_okf) fn resolve_link_target(
	source_path: &Path,
	docs_root: &Path,
	target: &str,
) -> Option<PathBuf> {
	let path_without_anchor = target.split('#').next().unwrap_or_default();
	let path_without_query = path_without_anchor.split('?').next().unwrap_or_default();

	if path_without_query.is_empty() {
		return None;
	}

	let raw_path = if let Some(root_relative) = path_without_query.strip_prefix('/') {
		docs_root.join(root_relative)
	} else {
		source_path.parent()?.join(path_without_query)
	};

	Some(docs_okf::normalize_path(&raw_path))
}
