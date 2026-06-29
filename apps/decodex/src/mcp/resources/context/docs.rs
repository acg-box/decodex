use std::{
	fs,
	io::ErrorKind,
	path::{Path, PathBuf},
};

use crate::{
	mcp::{
		McpContext, McpError,
		resources::types::{McpResource, ResourceContent, ResourceUri},
	},
	prelude::Result,
};

impl McpContext {
	pub(in crate::mcp::resources) fn docs_resources(&self) -> Result<Vec<McpResource>, McpError> {
		let mut resources = Vec::new();

		push_file_resource(
			&mut resources,
			self.repo_root.join("docs/index.md"),
			"decodex://docs/index",
			"Documentation index",
			"Checked-in Decodex documentation router.",
		);
		push_file_resource(
			&mut resources,
			self.repo_root.join("docs/policy.md"),
			"decodex://docs/policy",
			"Documentation policy",
			"Checked-in Decodex documentation policy.",
		);

		for lane in ["spec", "runbook", "reference", "decisions", "research"] {
			let docs_dir = self.repo_root.join("docs").join(lane);

			for entry in read_sorted_dir(&docs_dir)? {
				let Some(stem) = markdown_stem(&entry) else {
					continue;
				};

				resources.push(McpResource::markdown(
					format!("decodex://docs/{lane}/{stem}"),
					format!("docs/{lane}/{stem}.md"),
					"Checked-in Decodex documentation resource.",
				));
			}
		}
		for entry in read_sorted_dir(&self.repo_root.join("docs/research"))? {
			let Some(stem) = markdown_stem(&entry) else {
				continue;
			};

			resources.push(McpResource::markdown(
				format!("decodex://research/{stem}"),
				format!("docs/research/{stem}.md"),
				"Checked-in Markdown Research Contract concept.",
			));
		}

		Ok(resources)
	}

	pub(super) fn read_docs_resource(
		&self,
		uri: &ResourceUri,
	) -> Result<ResourceContent, McpError> {
		let path = match uri.segments.as_slice() {
			[segment] if segment == "index" => self.repo_root.join("docs/index.md"),
			[segment] if segment == "policy" => self.repo_root.join("docs/policy.md"),
			[lane, topic] if docs_lane_allowed(lane) && safe_resource_stem(topic) =>
				self.repo_root.join("docs").join(lane).join(format!("{topic}.md")),
			_ => return Err(McpError::resource_not_found()),
		};

		read_file_resource(&uri.raw, path, "text/markdown")
	}

	pub(super) fn read_research_resource(
		&self,
		uri: &ResourceUri,
	) -> Result<ResourceContent, McpError> {
		let [concept] = uri.segments.as_slice() else {
			return Err(McpError::resource_not_found());
		};

		if !safe_resource_stem(concept) {
			return Err(McpError::resource_not_found());
		}

		read_file_resource(
			&uri.raw,
			self.repo_root.join("docs/research").join(format!("{concept}.md")),
			"text/markdown",
		)
	}
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

fn read_file_resource(
	uri: &str,
	path: PathBuf,
	mime_type: &str,
) -> Result<ResourceContent, McpError> {
	let text = fs::read_to_string(path).map_err(|error| match error.kind() {
		ErrorKind::NotFound => McpError::resource_not_found(),
		_ => McpError::internal(error),
	})?;

	Ok(ResourceContent { uri: uri.to_owned(), mime_type: mime_type.to_owned(), text })
}

fn docs_lane_allowed(lane: &str) -> bool {
	matches!(lane, "spec" | "runbook" | "reference" | "decisions" | "research")
}

fn safe_resource_stem(value: &str) -> bool {
	!value.is_empty()
		&& value.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}
