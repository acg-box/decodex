//! Shared file, Markdown, frontmatter, and path helpers.

use crate::docs_okf::{
	BTreeSet, Component, Date, DocsCheckIssue, DocsFile, Mapping, Month, Path, PathBuf, Result,
	Url, fs, serde_yaml,
};

pub(super) fn collect_files(root: &Path, dir: &Path, files: &mut Vec<DocsFile>) -> Result<()> {
	for entry in fs::read_dir(dir)? {
		let entry = entry?;
		let path = entry.path();
		let file_type = entry.file_type()?;

		if file_type.is_dir() {
			collect_files(root, &path, files)?;
		} else if file_type.is_file() {
			let relative_path = path.strip_prefix(root)?.to_path_buf();
			let (content, read_error) =
				if path.extension().is_some_and(|extension| extension == "md") {
					match fs::read_to_string(&path) {
						Ok(content) => (Some(content), None),
						Err(error) => (None, Some(error.to_string())),
					}
				} else {
					(None, None)
				};

			files.push(DocsFile { path, relative_path, content, read_error });
		}
	}

	Ok(())
}

pub(super) fn read_okf_files(root: &Path) -> Result<Vec<DocsFile>> {
	if !root.is_dir() {
		color_eyre::eyre::bail!("OKF bundle root `{}` does not exist.", root.display());
	}

	let mut files = Vec::new();

	collect_files(root, root, &mut files)?;

	Ok(files)
}

pub(super) fn file_path_set(files: &[DocsFile]) -> BTreeSet<PathBuf> {
	files.iter().map(|file| file.relative_path.clone()).collect()
}

pub(super) fn docs_dirs_with_content(files: &[DocsFile]) -> BTreeSet<PathBuf> {
	let mut dirs = BTreeSet::new();

	for file in files {
		let Some(parent) = file.relative_path.parent() else {
			continue;
		};

		if parent.as_os_str().is_empty() {
			continue;
		}

		dirs.insert(parent.to_path_buf());
	}

	dirs
}

pub(super) fn is_markdown(path: &Path) -> bool {
	path.extension().is_some_and(|extension| extension == "md")
}

pub(super) fn is_concept_markdown(path: &Path) -> bool {
	is_markdown(path) && path.file_name().is_some_and(|name| name != "index.md" && name != "log.md")
}

pub(super) fn concept_type(file: &DocsFile) -> Option<String> {
	let content = file.content.as_deref()?;
	let (frontmatter, _) = split_yaml_frontmatter(content)?;
	let serde_yaml::Value::Mapping(fields) =
		serde_yaml::from_str::<serde_yaml::Value>(frontmatter).ok()?
	else {
		return None;
	};

	frontmatter_string(&fields, "type").map(str::to_owned)
}

pub(super) fn split_yaml_frontmatter(content: &str) -> Option<(&str, &str)> {
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

pub(super) fn frontmatter_value<'a>(
	fields: &'a Mapping,
	key: &str,
) -> Option<&'a serde_yaml::Value> {
	fields.get(serde_yaml::Value::String(key.to_owned()))
}

pub(super) fn frontmatter_string<'a>(fields: &'a Mapping, key: &str) -> Option<&'a str> {
	match frontmatter_value(fields, key) {
		Some(serde_yaml::Value::String(value)) => Some(value.trim()),
		_ => None,
	}
}

pub(super) fn strip_fragment(value: &str) -> &str {
	value.split_once('#').map_or(value, |(path, _)| path)
}

pub(super) fn is_http_url(value: &str) -> bool {
	let Ok(url) = Url::parse(value) else {
		return false;
	};

	matches!(url.scheme(), "http" | "https")
		&& url.host_str().is_some_and(|host| !host.trim().is_empty())
}

pub(super) fn is_normalized_relative_path(path: &Path) -> bool {
	!path.is_absolute()
		&& path.components().all(|component| matches!(component, Component::Normal(_)))
}

pub(super) fn is_valid_iso_date(value: &str) -> bool {
	let mut parts = value.split('-');
	let Some(year) = parts.next().and_then(|year| year.parse::<i32>().ok()) else {
		return false;
	};
	let Some(month) = parts.next().and_then(|month| month.parse::<u8>().ok()) else {
		return false;
	};
	let Some(day) = parts.next().and_then(|day| day.parse::<u8>().ok()) else {
		return false;
	};

	if parts.next().is_some() {
		return false;
	}

	let Ok(month) = Month::try_from(month) else {
		return false;
	};

	Date::from_calendar_date(year, month, day).is_ok()
}

pub(super) fn should_skip_link_target(target: &str) -> bool {
	target.starts_with('#')
		|| target.starts_with("http://")
		|| target.starts_with("https://")
		|| target.starts_with("mailto:")
		|| target.starts_with("tel:")
}

pub(super) fn resolve_link_target(
	source_path: &Path,
	docs_root: &Path,
	target: &str,
) -> Option<PathBuf> {
	let path_without_anchor = target.split('#').next().unwrap_or_default();
	let path_without_query = path_without_anchor.split('?').next().unwrap_or_default();

	if path_without_query.is_empty() {
		return None;
	}

	let raw_path = if let Some(root_relative) = path_without_query.strip_prefix('/') {
		docs_root.join(root_relative)
	} else {
		source_path.parent()?.join(path_without_query)
	};

	Some(normalize_path(&raw_path))
}

pub(super) fn normalize_path(path: &Path) -> PathBuf {
	let mut normalized = PathBuf::new();

	for component in path.components() {
		match component {
			Component::ParentDir => {
				normalized.pop();
			},
			Component::CurDir => {},
			other => normalized.push(other.as_os_str()),
		}
	}

	normalized
}

pub(super) fn issue(path: Option<PathBuf>, message: String) -> DocsCheckIssue {
	DocsCheckIssue { path, message }
}
