//! Release and comparison pair selection.

mod pairs;
mod releases;

pub(super) use self::{
	pairs::select_release_pairs,
	releases::{select_release, select_release_options},
};
