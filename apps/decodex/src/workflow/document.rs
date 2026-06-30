use std::{fs, path::Path};

use super::{
	WorkflowFrontmatter,
	validation::{FRONTMATTER_DELIMITER, split_frontmatter},
};

/// Parsed downstream workflow document.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkflowDocument {
	frontmatter: WorkflowFrontmatter,
	body: String,
}
impl WorkflowDocument {
	/// Parse a workflow document from Markdown text.
	pub fn parse_markdown(input: &str) -> crate::prelude::Result<Self> {
		let (frontmatter_input, body) = split_frontmatter(input)?;
		let frontmatter = toml::from_str::<WorkflowFrontmatter>(&frontmatter_input)?;

		frontmatter.validate()?;

		Ok(Self { frontmatter, body })
	}

	/// Load a workflow document from the repository root.
	pub fn from_path(path: impl AsRef<Path>) -> crate::prelude::Result<Self> {
		let input = fs::read_to_string(path)?;

		Self::parse_markdown(&input)
	}

	/// Machine-readable frontmatter for orchestration behavior.
	pub fn frontmatter(&self) -> &WorkflowFrontmatter {
		&self.frontmatter
	}

	/// Human-readable Markdown policy body.
	pub fn body(&self) -> &str {
		&self.body
	}

	/// Render the workflow back to Markdown for process-to-process handoff.
	pub fn to_markdown(&self) -> crate::prelude::Result<String> {
		let frontmatter = toml::to_string(&self.frontmatter)?;
		let mut markdown = format!("{FRONTMATTER_DELIMITER}\n{frontmatter}{FRONTMATTER_DELIMITER}");

		if !self.body.is_empty() {
			markdown.push_str("\n\n");
			markdown.push_str(&self.body);
		}

		Ok(markdown)
	}
}
