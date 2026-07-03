use crate::docs_okf::{self, DocsFile, Mapping, serde_yaml};

pub(in crate::docs_okf) fn concept_type(file: &DocsFile) -> Option<String> {
	let content = file.content.as_deref()?;
	let (frontmatter, _) = split_yaml_frontmatter(content)?;
	let serde_yaml::Value::Mapping(fields) =
		serde_yaml::from_str::<serde_yaml::Value>(frontmatter).ok()?
	else {
		return None;
	};

	frontmatter_string(&fields, "type").map(str::to_owned)
}

pub(in crate::docs_okf) fn split_yaml_frontmatter(content: &str) -> Option<(&str, &str)> {
	let (body_start, closing_delimiter) = if let Some(body_start) = content.strip_prefix("---\n") {
		(body_start, "\n---\n")
	} else {
		(content.strip_prefix("---\r\n")?, "\r\n---\r\n")
	};
	let closing = body_start.find(closing_delimiter)?;
	let frontmatter = &body_start[..closing];
	let body = &body_start[(closing + closing_delimiter.len())..];

	Some((frontmatter, body))
}

pub(in crate::docs_okf) fn frontmatter_value<'a>(
	fields: &'a Mapping,
	key: &str,
) -> Option<&'a serde_yaml::Value> {
	fields.get(serde_yaml::Value::String(key.to_owned()))
}

pub(in crate::docs_okf) fn frontmatter_string<'a>(
	fields: &'a Mapping,
	key: &str,
) -> Option<&'a str> {
	match docs_okf::frontmatter_value(fields, key) {
		Some(serde_yaml::Value::String(value)) => Some(value.trim()),
		_ => None,
	}
}
