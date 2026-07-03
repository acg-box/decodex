use crate::docs_okf::{self, OkfConceptSummary, OkfQuery, Path, Result, graph::summary};

pub(crate) fn query_okf_bundle(root: &Path, query: &OkfQuery) -> Result<Vec<OkfConceptSummary>> {
	let files = docs_okf::read_okf_files(root)?;
	let mut concepts = Vec::new();

	for file in files.iter().filter(|file| docs_okf::is_concept_markdown(&file.relative_path)) {
		let Some(concept) = summary::concept_summary(file) else {
			continue;
		};

		if okf_query_matches(&concept, query) {
			concepts.push(concept);
		}
	}

	concepts.sort_by(|left, right| left.path.cmp(&right.path));

	Ok(concepts)
}

fn okf_query_matches(concept: &OkfConceptSummary, query: &OkfQuery) -> bool {
	query
		.concept_type
		.as_deref()
		.is_none_or(|value| concept.concept_type.eq_ignore_ascii_case(value))
		&& query
			.tags
			.iter()
			.all(|tag| concept.tags.iter().any(|candidate| candidate.eq_ignore_ascii_case(tag)))
		&& query.resource.as_deref().is_none_or(|value| {
			concept.resource.as_deref().is_some_and(|resource| contains_ci(resource, value))
		}) && query.source_ref.as_deref().is_none_or(|value| {
		concept.source_refs.iter().any(|source_ref| contains_ci(source_ref, value))
	}) && query
		.code_ref
		.as_deref()
		.is_none_or(|value| concept.code_refs.iter().any(|code_ref| contains_ci(code_ref, value)))
		&& query
			.related
			.as_deref()
			.is_none_or(|value| concept.related.iter().any(|related| contains_ci(related, value)))
		&& query.text.as_deref().is_none_or(|value| concept_text_matches(concept, value))
}

fn concept_text_matches(concept: &OkfConceptSummary, value: &str) -> bool {
	contains_ci(&concept.path, value)
		|| contains_ci(&concept.title, value)
		|| concept.description.as_deref().is_some_and(|description| contains_ci(description, value))
}

fn contains_ci(haystack: &str, needle: &str) -> bool {
	haystack.to_lowercase().contains(&needle.to_lowercase())
}
