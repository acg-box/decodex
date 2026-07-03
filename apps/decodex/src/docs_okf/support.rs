//! Shared file, Markdown, frontmatter, and path helpers.

mod files;
mod frontmatter;
mod issue;
mod links;
mod paths;

pub(super) use self::{
	files::{collect_files, read_okf_files},
	frontmatter::{concept_type, frontmatter_string, frontmatter_value, split_yaml_frontmatter},
	issue::issue,
	links::{resolve_link_target, should_skip_link_target},
	paths::{
		docs_dirs_with_content, file_path_set, is_concept_markdown, is_http_url, is_markdown,
		is_normalized_relative_path, is_valid_iso_date, normalize_path, strip_fragment,
	},
};
