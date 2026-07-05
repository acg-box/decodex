mod canonical_repo_root_for_checkout_prefers_shared_repo_root_for_linked_worktree;
mod external_project_config_requires_explicit_repo_root;
mod loads_service_config_from_external_project_file_with_explicit_repo_root;
mod loads_service_config_from_project_directory;
mod loads_service_config_from_project_file_with_explicit_repo_root;
mod loads_service_config_with_relative_worktree_override;
mod rejects_project_config_with_nonstandard_file_name;

use crate::config::{self};

#[test]
#[cfg(unix)]
fn git_path_output_preserves_non_utf8_bytes() {
	let path = config::path_buf_from_git_line_output(b"/tmp/\xFFlane\n")
		.expect("git path output should parse")
		.expect("git path output should not be empty");

	assert_eq!(std::os::unix::ffi::OsStrExt::as_bytes(path.as_os_str()), b"/tmp/\xFFlane");
}
