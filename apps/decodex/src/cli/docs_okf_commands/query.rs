use std::path::Path;

use clap::Args;

use crate::{
	docs_okf::{self, OkfQuery},
	prelude::Result,
};

#[derive(Debug, Args)]
pub(crate) struct OkfFindFilters {
	/// Match concept type.
	#[arg(long = "type")]
	pub(super) concept_type: Option<String>,
	/// Match exact tag. May be repeated.
	#[arg(long)]
	pub(crate) tag: Vec<String>,
	/// Match resource URI substring.
	#[arg(long)]
	pub(super) resource: Option<String>,
	/// Match source_refs substring.
	#[arg(long)]
	pub(super) source_ref: Option<String>,
	/// Match code_refs substring.
	#[arg(long)]
	pub(super) code_ref: Option<String>,
	/// Match related refs substring.
	#[arg(long)]
	pub(super) related: Option<String>,
	/// Match path, title, or description substring.
	#[arg(long)]
	pub(crate) text: Option<String>,
}

impl From<&OkfFindFilters> for OkfQuery {
	fn from(value: &OkfFindFilters) -> Self {
		Self {
			concept_type: value.concept_type.clone(),
			tags: value.tag.clone(),
			resource: value.resource.clone(),
			source_ref: value.source_ref.clone(),
			code_ref: value.code_ref.clone(),
			related: value.related.clone(),
			text: value.text.clone(),
		}
	}
}

pub(super) fn run_okf_find(root: &Path, filters: &OkfFindFilters) -> Result<()> {
	let query = OkfQuery::from(filters);
	let concepts = docs_okf::query_okf_bundle(root, &query)?;

	println!("okf find: concepts={} root={}", concepts.len(), root.display());

	for concept in concepts {
		println!(
			"{} | {} | {}{}",
			concept.path,
			concept.concept_type,
			concept.title,
			concept
				.description
				.as_deref()
				.map_or(String::new(), |description| format!(" | {description}"))
		);
	}

	Ok(())
}

pub(super) fn run_okf_graph(root: &Path, json: bool) -> Result<()> {
	let graph = docs_okf::build_okf_graph(root)?;

	if json {
		print!("{}", docs_okf::render_okf_graph_json(&graph)?);
	} else {
		print!("{}", docs_okf::render_okf_graph_summary(root, &graph));
	}

	Ok(())
}
