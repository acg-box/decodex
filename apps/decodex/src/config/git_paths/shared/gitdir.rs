use std::{
	fs,
	path::{Path, PathBuf},
};

use crate::prelude::Result;

pub(in crate::config::git_paths::shared) fn git_dir_reference_matches_common_dir_best_effort(
	dot_git: &Path,
	common_dir: &Path,
) -> bool {
	git_dir_reference_matches_common_dir(dot_git, common_dir).unwrap_or_default()
}

fn git_dir_reference_matches_common_dir(dot_git: &Path, common_dir: &Path) -> Result<bool> {
	if dot_git.is_dir() {
		return Ok(fs::canonicalize(dot_git)? == common_dir);
	}
	if !dot_git.is_file() {
		return Ok(false);
	}

	let gitdir = parse_gitdir_file(dot_git)?;

	Ok(fs::canonicalize(gitdir)? == common_dir)
}

fn parse_gitdir_file(dot_git: &Path) -> Result<PathBuf> {
	let contents = fs::read_to_string(dot_git)?;
	let prefix = "gitdir:";
	let Some(gitdir) = contents.lines().find_map(|line| line.strip_prefix(prefix)) else {
		crate::prelude::eyre::bail!(
			"Git dir file `{}` is missing a `gitdir:` entry.",
			dot_git.display()
		);
	};
	let gitdir = gitdir.trim();

	if gitdir.is_empty() {
		crate::prelude::eyre::bail!(
			"Git dir file `{}` has an empty `gitdir:` entry.",
			dot_git.display()
		);
	}

	let gitdir = PathBuf::from(gitdir);

	if gitdir.is_absolute() {
		return Ok(gitdir);
	}

	let Some(parent) = dot_git.parent() else {
		crate::prelude::eyre::bail!(
			"Git dir file `{}` must have a parent directory.",
			dot_git.display()
		);
	};

	Ok(parent.join(gitdir))
}
