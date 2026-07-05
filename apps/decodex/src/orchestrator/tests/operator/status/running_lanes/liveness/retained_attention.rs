#[cfg(any(target_os = "linux", target_os = "macos"))]
mod operator_status_snapshot_does_not_shadow_post_review_lane_with_retained_attention_run;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod operator_status_snapshot_keeps_unleased_live_process_visible_but_not_running;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod operator_status_snapshot_post_review_lane_owns_orphaned_live_thread_worktree;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod operator_status_snapshot_projects_unleased_app_server_current_lane_as_retained_attention;
