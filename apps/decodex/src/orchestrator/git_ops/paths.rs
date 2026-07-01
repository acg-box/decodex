use super::*;

pub(crate) fn relative_worktree_path(project: &ServiceConfig, worktree: &WorktreeSpec) -> String {
	relative_worktree_path_for_path(project, &worktree.path)
}

pub(crate) fn relative_worktree_path_for_path(
	project: &ServiceConfig,
	worktree_path: &Path,
) -> String {
	if let Ok(relative_path) = worktree_path.strip_prefix(project.repo_root()) {
		if relative_path.as_os_str().is_empty() {
			return String::from(".");
		}

		return relative_path.display().to_string();
	}
	if let Some(root_name) = project.worktree_root().file_name()
		&& let Ok(relative_path) = worktree_path.strip_prefix(project.worktree_root())
	{
		return Path::new(root_name).join(relative_path).display().to_string();
	}

	worktree_path.file_name().map_or_else(
		|| worktree_path.display().to_string(),
		|path| path.to_string_lossy().into_owned(),
	)
}
