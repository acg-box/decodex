use crate::docs_okf::{
	self, DocsCheckReport, Mapping, Path,
	serde_yaml::{self, Value},
};

pub(in crate::docs_okf::check::docs) fn parse_frontmatter_mapping(
	frontmatter: &str,
	path: &Path,
	report: &mut DocsCheckReport,
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

pub(in crate::docs_okf::check::docs) fn frontmatter_string_list(
	fields: &Mapping,
	key: &str,
	path: &Path,
	report: &mut DocsCheckReport,
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

pub(in crate::docs_okf::check::docs) fn read_required_frontmatter_string(
	fields: &Mapping,
	key: &str,
	path: &Path,
	report: &mut DocsCheckReport,
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

pub(in crate::docs_okf::check::docs) fn validate_frontmatter_enum(
	fields: &Mapping,
	key: &str,
	allowed_values: &[&str],
	path: &Path,
	report: &mut DocsCheckReport,
) {
	let Some(value) = docs_okf::frontmatter_string(fields, key) else {
		return;
	};

	if !value.is_empty() && !allowed_values.contains(&value) {
		report.issues.push(docs_okf::issue(
			Some(path.to_path_buf()),
			format!("frontmatter key `{key}` has unsupported value `{value}`"),
		));
	}
}

pub(in crate::docs_okf::check::docs) fn validate_frontmatter_date(
	fields: &Mapping,
	path: &Path,
	report: &mut DocsCheckReport,
) {
	let Some(value) = docs_okf::frontmatter_string(fields, "last_verified") else {
		return;
	};

	if !value.is_empty() && !docs_okf::is_valid_iso_date(value) {
		report.issues.push(docs_okf::issue(
			Some(path.to_path_buf()),
			format!("frontmatter key `last_verified` must be an ISO date, not `{value}`"),
		));
	}
}
