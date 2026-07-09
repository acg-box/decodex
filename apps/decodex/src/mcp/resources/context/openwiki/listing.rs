use std::{
	fs,
	io::ErrorKind,
	path::{Path, PathBuf},
};

use crate::{
	mcp::{McpError, resources::types::McpResource},
	prelude::Result,
};

pub(super) fn openwiki_resources(repo_root: &Path) -> Result<Vec<McpResource>, McpError> {
	let mut resources = Vec::new();

	push_file_resource(
		&mut resources,
		repo_root.join("openwiki/quickstart.md"),
		"decodex://openwiki/quickstart",
		"OpenWiki quickstart",
		"Checked-in Decodex OpenWiki router.",
	);

	for section in ["architecture", "workflows", "specs", "operations", "integrations"] {
		let section_dir = repo_root.join("openwiki").join(section);

		for entry in read_sorted_dir(&section_dir)? {
			let Some(stem) = markdown_stem(&entry) else {
				continue;
			};

			resources.push(McpResource::markdown(
				format!("decodex://openwiki/{section}/{stem}"),
				format!("openwiki/{section}/{stem}.md"),
				"Checked-in Decodex OpenWiki resource.",
			));
		}
	}

	Ok(resources)
}

fn push_file_resource(
	resources: &mut Vec<McpResource>,
	path: PathBuf,
	uri: &str,
	name: &str,
	description: &str,
) {
	if path.is_file() {
		resources.push(McpResource::markdown(uri, name, description));
	}
}

fn read_sorted_dir(path: &Path) -> Result<Vec<PathBuf>, McpError> {
	let entries = match fs::read_dir(path) {
		Ok(entries) => entries,
		Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
		Err(error) => return Err(McpError::internal(error)),
	};
	let mut paths = entries
		.map(|entry| entry.map(|entry| entry.path()).map_err(McpError::internal))
		.collect::<Result<Vec<_>, _>>()?;

	paths.sort();

	Ok(paths)
}

fn markdown_stem(path: &Path) -> Option<String> {
	if path.extension().and_then(|extension| extension.to_str()) != Some("md") {
		return None;
	}

	path.file_stem().and_then(|stem| stem.to_str()).map(str::to_owned)
}
