//! Worktree mapping and status inspection for stale-active recovery.

mod inspection;
mod mapping;
mod marker;

pub(super) use self::{
	inspection::inspect_stale_active_worktree, mapping::stale_active_worktree_mapping_for_keys,
};
