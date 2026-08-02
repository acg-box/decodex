use crate::{DEFAULT_VALIDATION_PATHS, Path, PathBuf, eyre, fs};

pub(crate) fn validation_paths(paths: &[PathBuf]) -> Vec<PathBuf> {
	if paths.is_empty() {
		DEFAULT_VALIDATION_PATHS.iter().map(PathBuf::from).collect()
	} else {
		paths.to_vec()
	}
}

pub(crate) fn collect_json_files(
	paths: &[PathBuf],
	allow_missing_roots: bool,
) -> crate::prelude::Result<Vec<PathBuf>> {
	let mut files = Vec::new();

	for path in paths {
		if allow_missing_roots && crate::is_radar_cache_path(path) {
			files.extend(crate::collect_private_json_files_if_present(path)?);

			continue;
		}
		if allow_missing_roots && !path.exists() {
			continue;
		}

		collect_json_path(path, &mut files)?;
	}

	files.sort();

	Ok(files)
}

fn collect_json_path(path: &Path, files: &mut Vec<PathBuf>) -> crate::prelude::Result<()> {
	if crate::is_radar_cache_path(path) {
		match crate::private_fs::private_entry_kind(path)? {
			Some(crate::private_fs::PrivateEntryKind::File) => {
				if path.extension().is_some_and(|extension| extension == "json") {
					files.push(path.to_path_buf());
				}
			},
			Some(crate::private_fs::PrivateEntryKind::Directory) => {
				files.extend(crate::collect_private_json_files(path)?);
			},
			None => {
				return Err(eyre::eyre!(
					"Radar validation path does not exist: {}",
					path.display()
				));
			},
		}

		return Ok(());
	}

	if path.is_dir() {
		let mut children = fs::read_dir(path)?
			.map(|entry| entry.map(|entry| entry.path()))
			.collect::<std::result::Result<Vec<_>, _>>()?;

		children.sort();

		for child in children {
			collect_json_path(&child, files)?;
		}
	} else if path.is_file() {
		if path.extension().is_some_and(|extension| extension == "json") {
			files.push(path.to_path_buf());
		}
	} else {
		return Err(eyre::eyre!("Radar validation path does not exist: {}", path.display()));
	}

	Ok(())
}
