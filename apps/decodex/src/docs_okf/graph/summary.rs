use crate::docs_okf::{
	self, DocsFile, Mapping, OkfConceptSummary, Path,
	serde_yaml::{self, Value},
};

pub(in crate::docs_okf::graph) fn concept_summary(file: &DocsFile) -> Option<OkfConceptSummary> {
	let content = file.content.as_deref()?;
	let (frontmatter, _) = docs_okf::split_yaml_frontmatter(content)?;
	let serde_yaml::Value::Mapping(fields) = serde_yaml::from_str::<Value>(frontmatter).ok()?
	else {
		return None;
	};
	let concept_type = docs_okf::frontmatter_string(&fields, "type")?.to_owned();
	let path = path_to_string(&file.relative_path);
	let title = docs_okf::frontmatter_string(&fields, "title")
		.filter(|title| !title.is_empty())
		.map_or_else(|| concept_id(&file.relative_path), str::to_owned);
	let description = docs_okf::frontmatter_string(&fields, "description")
		.filter(|description| !description.is_empty())
		.map(str::to_owned);
	let resource = docs_okf::frontmatter_string(&fields, "resource")
		.filter(|resource| !resource.is_empty())
		.map(str::to_owned);
	let tags = frontmatter_string_list_lossy(&fields, "tags");
	let source_refs = frontmatter_string_list_lossy(&fields, "source_refs");
	let code_refs = frontmatter_string_list_lossy(&fields, "code_refs");
	let related = frontmatter_string_list_lossy(&fields, "related");

	Some(OkfConceptSummary {
		id: concept_id(&file.relative_path),
		path,
		concept_type,
		title,
		description,
		resource,
		tags,
		source_refs,
		code_refs,
		related,
	})
}

pub(in crate::docs_okf::graph) fn frontmatter_string_list_lossy(
	fields: &Mapping,
	key: &str,
) -> Vec<String> {
	match docs_okf::frontmatter_value(fields, key) {
		Some(serde_yaml::Value::Sequence(items)) => items
			.iter()
			.filter_map(|item| match item {
				serde_yaml::Value::String(value) if !value.trim().is_empty() =>
					Some(value.trim().to_owned()),
				_ => None,
			})
			.collect(),
		_ => Vec::new(),
	}
}

pub(in crate::docs_okf::graph) fn concept_id(path: &Path) -> String {
	let mut id = path.to_path_buf();

	id.set_extension("");

	path_to_string(&id)
}

fn path_to_string(path: &Path) -> String {
	path.to_string_lossy().replace('\\', "/")
}
