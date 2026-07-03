use std::{
	fs,
	io::ErrorKind,
	path::{Path, PathBuf},
};

use color_eyre::eyre::WrapErr;

use crate::{
	config,
	manual::{MANUAL_LAND_CLOSEOUT_MARKER_GIT_PATH, ManualLandCloseoutMarkerRecord},
	prelude::{Result, eyre},
};

pub(super) fn manual_land_closeout_marker_path(checkout_root: &Path) -> Result<PathBuf> {
	let Some(git_dir) = config::git_dir_for_checkout(checkout_root)? else {
		eyre::bail!(
			"Current checkout `{}` does not expose a Git administrative directory.",
			checkout_root.display()
		);
	};

	Ok(git_dir.join(MANUAL_LAND_CLOSEOUT_MARKER_GIT_PATH))
}

pub(in crate::manual) fn manual_land_closeout_matches(
	checkout_root: &Path,
	pr_url: &str,
	merge_commit: &str,
	branch_name: &str,
	landed_change_record: &str,
) -> Result<bool> {
	let Some(marker) = read_manual_land_closeout_marker(checkout_root)? else {
		return Ok(false);
	};

	Ok(marker.pr_url.as_deref() == Some(pr_url)
		&& marker.merge_commit.as_deref() == Some(merge_commit)
		&& marker.branch_name.as_deref() == Some(branch_name)
		&& marker.landed_change.as_deref() == Some(landed_change_record))
}

pub(in crate::manual) fn read_manual_land_closeout_marker(
	checkout_root: &Path,
) -> Result<Option<ManualLandCloseoutMarkerRecord>> {
	let marker_path = manual_land_closeout_marker_path(checkout_root)?;
	let marker_body = match fs::read_to_string(&marker_path) {
		Ok(marker_body) => marker_body,
		Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
		Err(error) => {
			return Err(error).wrap_err_with(|| {
				format!("Failed to read manual land closeout marker `{}`.", marker_path.display())
			});
		},
	};
	let mut marker = ManualLandCloseoutMarkerRecord::default();

	for line in marker_body.lines() {
		let Some((key, value)) = line.split_once('=') else {
			continue;
		};

		match key {
			"pr_url" => marker.pr_url = Some(value.to_owned()),
			"merge_commit" => marker.merge_commit = Some(value.to_owned()),
			"branch_name" => marker.branch_name = Some(value.to_owned()),
			"landed_change" => marker.landed_change = Some(value.to_owned()),
			_ => {},
		}
	}

	Ok(Some(marker))
}

pub(in crate::manual) fn write_manual_land_closeout_marker(
	checkout_root: &Path,
	pr_url: &str,
	merge_commit: &str,
	branch_name: &str,
	landed_change_record: &str,
) -> Result<()> {
	let marker_path = manual_land_closeout_marker_path(checkout_root)?;
	let Some(marker_dir) = marker_path.parent() else {
		eyre::bail!(
			"Manual land closeout marker path `{}` has no parent directory.",
			marker_path.display()
		);
	};

	fs::create_dir_all(marker_dir).wrap_err_with(|| {
		format!(
			"Failed to create manual land closeout marker directory `{}`.",
			marker_dir.display()
		)
	})?;
	fs::write(
		&marker_path,
		format!(
			"pr_url={pr_url}\nmerge_commit={merge_commit}\nbranch_name={branch_name}\nlanded_change={landed_change_record}\n"
		),
	)
	.wrap_err_with(|| {
		format!("Failed to write manual land closeout marker `{}`.", marker_path.display())
	})?;

	Ok(())
}
