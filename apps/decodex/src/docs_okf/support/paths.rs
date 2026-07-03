use crate::docs_okf::{BTreeSet, Component, Date, DocsFile, Month, Path, PathBuf, Url};

pub(in crate::docs_okf) fn file_path_set(files: &[DocsFile]) -> BTreeSet<PathBuf> {
	files.iter().map(|file| file.relative_path.clone()).collect()
}

pub(in crate::docs_okf) fn docs_dirs_with_content(files: &[DocsFile]) -> BTreeSet<PathBuf> {
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

pub(in crate::docs_okf) fn is_markdown(path: &Path) -> bool {
	path.extension().is_some_and(|extension| extension == "md")
}

pub(in crate::docs_okf) fn is_concept_markdown(path: &Path) -> bool {
	is_markdown(path) && path.file_name().is_some_and(|name| name != "index.md" && name != "log.md")
}

pub(in crate::docs_okf) fn strip_fragment(value: &str) -> &str {
	value.split_once('#').map_or(value, |(path, _)| path)
}

pub(in crate::docs_okf) fn is_http_url(value: &str) -> bool {
	let Ok(url) = Url::parse(value) else {
		return false;
	};

	matches!(url.scheme(), "http" | "https")
		&& url.host_str().is_some_and(|host| !host.trim().is_empty())
}

pub(in crate::docs_okf) fn is_normalized_relative_path(path: &Path) -> bool {
	!path.is_absolute()
		&& path.components().all(|component| matches!(component, Component::Normal(_)))
}

pub(in crate::docs_okf) fn is_valid_iso_date(value: &str) -> bool {
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

pub(in crate::docs_okf) fn normalize_path(path: &Path) -> PathBuf {
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
