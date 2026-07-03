mod concept;
mod drift;
mod frontmatter;
mod headings;
mod layout;
mod links;
mod readability;
mod references;

pub(super) use self::{
	concept::check_concept_contracts,
	drift::check_drift_surface,
	layout::{check_markdown_only, check_required_docs_layout},
	links::check_links,
	readability::{check_acronym_capitalization, check_markdown_readability},
};
