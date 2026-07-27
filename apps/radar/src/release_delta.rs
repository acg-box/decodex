//! Release delta artifact generation and release-window backfill orchestration.

mod backfill;
mod build;
mod comparison;
mod options;
mod selection;

pub(crate) use self::{backfill::backfill_release_range, build::refresh_release_delta};

use self::{
	comparison::{build_release_comparison, load_signal_entries},
	options::{
		compact_release, compact_releases, filter_release_options, release_delta_report,
		release_sort_key, release_tag, required_release_tag, stable_version_key,
	},
	selection::{select_release, select_release_options, select_release_pairs},
};
use crate::{
	BTreeMap, BTreeSet, GitHubApi, HashSet, Path, RELEASE_DELTA_SCHEMA,
	RadarRefreshReleaseDeltaReport, RadarRefreshReleaseDeltaRequest, RefreshKind,
	RefreshWriteReport, Value, absolute_repo_path, extract_commit_sha_from_url,
	extract_pr_number_from_url, eyre, github_token, inspect_json_refresh, iter, load_json,
	optional_value_string, pretty_json, refresh_json, repo_root, required_value_i64,
	required_value_string, serde_json, sorted_json_files, string_array, string_array_from_value,
	utc_now_iso, validate_artifact_errors, validate_signal_file,
};

#[derive(Clone, Debug)]
pub(super) struct ReleasePair {
	stable: Value,
	preview: Value,
}
