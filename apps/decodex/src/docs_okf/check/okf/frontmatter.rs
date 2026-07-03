use crate::docs_okf::{
	self, DocsFile, Mapping, OkfCheckReport, Path,
	serde_yaml::{self, Value},
};

pub(in crate::docs_okf::check::okf) fn okf_frontmatter_fields(
	file: &DocsFile,
	report: &mut OkfCheckReport,
) -> Option<Mapping> {
	let content = file.content.as_deref()?;
	let Some((frontmatter, _)) = docs_okf::split_yaml_frontmatter(content) else {
		report.issues.push(docs_okf::issue(
			Some(file.relative_path.clone()),
			String::from("concept must start with YAML frontmatter delimited by ---"),
		));

		return None;
	};

	parse_okf_frontmatter_mapping(frontmatter, &file.relative_path, report)
}

pub(in crate::docs_okf::check::okf) fn parse_okf_frontmatter_mapping(
	frontmatter: &str,
	path: &Path,
	report: &mut OkfCheckReport,
) -> Option<Mapping> {
	match serde_yaml::from_str::<Value>(frontmatter) {
		Ok(serde_yaml::Value::Mapping(mapping)) => Some(mapping),
		Ok(_) => {
			report.issues.push(docs_okf::issue(
				Some(path.to_path_buf()),
				String::from("frontmatter must be a YAML mapping"),
			));

			None
		},
		Err(error) => {
			report.issues.push(docs_okf::issue(
				Some(path.to_path_buf()),
				format!("frontmatter must parse as YAML: {error}"),
			));

			None
		},
	}
}

pub(in crate::docs_okf::check::okf) fn read_required_okf_frontmatter_string(
	fields: &Mapping,
	key: &str,
	path: &Path,
	report: &mut OkfCheckReport,
) {
	match docs_okf::frontmatter_value(fields, key) {
		Some(serde_yaml::Value::String(value)) if !value.trim().is_empty() => {},
		Some(serde_yaml::Value::String(_)) | None => report.issues.push(docs_okf::issue(
			Some(path.to_path_buf()),
			format!("frontmatter key `{key}` is required and must be non-empty"),
		)),
		Some(_) => report.issues.push(docs_okf::issue(
			Some(path.to_path_buf()),
			format!("frontmatter key `{key}` must be a string"),
		)),
	}
}

pub(in crate::docs_okf::check::okf) fn okf_frontmatter_string_list(
	fields: &Mapping,
	key: &str,
	path: &Path,
	report: &mut OkfCheckReport,
) -> Option<Vec<String>> {
	match docs_okf::frontmatter_value(fields, key) {
		None => None,
		Some(serde_yaml::Value::Sequence(items)) => {
			let mut values = Vec::new();

			for item in items {
				match item {
					serde_yaml::Value::String(value) if !value.trim().is_empty() => {
						values.push(value.trim().to_owned());
					},
					serde_yaml::Value::String(_) => report.issues.push(docs_okf::issue(
						Some(path.to_path_buf()),
						format!("frontmatter list `{key}` must not contain empty strings"),
					)),
					_ => report.issues.push(docs_okf::issue(
						Some(path.to_path_buf()),
						format!("frontmatter list `{key}` must contain only strings"),
					)),
				}
			}

			Some(values)
		},
		Some(_) => {
			report.issues.push(docs_okf::issue(
				Some(path.to_path_buf()),
				format!("frontmatter key `{key}` must be a list of strings"),
			));

			None
		},
	}
}
