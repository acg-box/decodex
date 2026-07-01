//! Archive hygiene config and request normalization helpers.

use std::{
	collections::BTreeSet,
	env,
	path::{Path, PathBuf},
};

use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
	prelude::{Result, eyre},
	runtime,
	state::StateStore,
};

pub(super) fn resolve_config_path(
	explicit_path: Option<&Path>,
	state_store: &StateStore,
) -> Result<Option<PathBuf>> {
	if let Some(path) = explicit_path {
		return Ok(Some(path.to_path_buf()));
	}

	runtime::registered_config_path_for_cwd(state_store, &env::current_dir()?)
}

pub(super) fn normalize_repo_labels(repo_labels: &[String]) -> Result<Vec<String>> {
	let mut labels = BTreeSet::new();

	for label in repo_labels {
		if label.trim() != label || label.is_empty() {
			eyre::bail!(
				"`--repo-label` values must be non-empty labels without surrounding whitespace."
			);
		}
		if !label.starts_with("repo:") {
			eyre::bail!("`--repo-label` must name a repo label such as `repo:decodex`.");
		}

		labels.insert(label.clone());
	}

	if labels.is_empty() {
		eyre::bail!("At least one `--repo-label` is required.");
	}

	Ok(labels.into_iter().collect())
}

pub(super) fn updated_before_timestamp(older_than_days: u32) -> Result<String> {
	if older_than_days == 0 {
		eyre::bail!("`--older-than-days` must be greater than zero.");
	}

	Ok((OffsetDateTime::now_utc() - Duration::days(i64::from(older_than_days))).format(&Rfc3339)?)
}
