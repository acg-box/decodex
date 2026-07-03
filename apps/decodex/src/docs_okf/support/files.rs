use crate::docs_okf::{DocsFile, Path, Result, fs};

pub(in crate::docs_okf) fn collect_files(
	root: &Path,
	dir: &Path,
	files: &mut Vec<DocsFile>,
) -> Result<()> {
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

pub(in crate::docs_okf) fn read_okf_files(root: &Path) -> Result<Vec<DocsFile>> {
	if !root.is_dir() {
		color_eyre::eyre::bail!("OKF bundle root `{}` does not exist.", root.display());
	}

	let mut files = Vec::new();

	collect_files(root, root, &mut files)?;

	Ok(files)
}
