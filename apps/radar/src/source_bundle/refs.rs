use crate::Value;

pub(super) fn collect_docs_refs(files: &[Value]) -> Vec<String> {
	files
		.iter()
		.filter_map(file_name)
		.filter(|filename| filename.starts_with("docs/") || filename.ends_with("README.md"))
		.map(str::to_owned)
		.collect()
}

pub(super) fn collect_examples_refs(files: &[Value]) -> Vec<String> {
	files
		.iter()
		.filter_map(file_name)
		.filter(|filename| {
			filename.to_lowercase().contains("example") || filename.contains("examples/")
		})
		.map(str::to_owned)
		.collect()
}

fn file_name(file: &Value) -> Option<&str> {
	file.as_object()?.get("filename")?.as_str()
}
