use std::{
	fs,
	io::ErrorKind,
	path::{Path, PathBuf},
};

use color_eyre::eyre::WrapErr;

use crate::{
	config,
	manual::{MANUAL_LAND_CLOSEOUT_RECEIPT_GIT_PATH, ManualLandCloseoutReceiptRecord},
	prelude::{Result, eyre},
};

pub(super) fn manual_land_closeout_receipt_path(checkout_root: &Path) -> Result<PathBuf> {
	let Some(git_dir) = config::git_dir_for_checkout(checkout_root)? else {
		eyre::bail!(
			"Current checkout `{}` does not expose a Git administrative directory.",
			checkout_root.display()
		);
	};

	Ok(git_dir.join(MANUAL_LAND_CLOSEOUT_RECEIPT_GIT_PATH))
}

pub(in crate::manual) fn manual_land_closeout_receipt_matches(
	checkout_root: &Path,
	pr_url: &str,
	merge_commit: &str,
	branch_name: &str,
	landed_change_record: &str,
) -> Result<bool> {
	let Some(receipt) = read_manual_land_closeout_receipt(checkout_root)? else {
		return Ok(false);
	};

	Ok(receipt.pr_url.as_deref() == Some(pr_url)
		&& receipt.merge_commit.as_deref() == Some(merge_commit)
		&& receipt.branch_name.as_deref() == Some(branch_name)
		&& receipt.landed_change.as_deref() == Some(landed_change_record))
}

pub(in crate::manual) fn read_manual_land_closeout_receipt(
	checkout_root: &Path,
) -> Result<Option<ManualLandCloseoutReceiptRecord>> {
	let receipt_path = manual_land_closeout_receipt_path(checkout_root)?;
	let receipt_body = match fs::read_to_string(&receipt_path) {
		Ok(receipt_body) => receipt_body,
		Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
		Err(error) => {
			return Err(error).wrap_err_with(|| {
				format!("Failed to read manual land closeout receipt `{}`.", receipt_path.display())
			});
		},
	};
	let mut receipt = ManualLandCloseoutReceiptRecord::default();

	for line in receipt_body.lines() {
		let Some((key, value)) = line.split_once('=') else {
			continue;
		};

		match key {
			"pr_url" => receipt.pr_url = Some(value.to_owned()),
			"merge_commit" => receipt.merge_commit = Some(value.to_owned()),
			"branch_name" => receipt.branch_name = Some(value.to_owned()),
			"landed_change" => receipt.landed_change = Some(value.to_owned()),
			_ => {},
		}
	}

	Ok(Some(receipt))
}

pub(in crate::manual) fn write_manual_land_closeout_receipt(
	checkout_root: &Path,
	pr_url: &str,
	merge_commit: &str,
	branch_name: &str,
	landed_change_record: &str,
) -> Result<()> {
	let receipt_path = manual_land_closeout_receipt_path(checkout_root)?;
	let Some(receipt_dir) = receipt_path.parent() else {
		eyre::bail!(
			"Manual land closeout receipt path `{}` has no parent directory.",
			receipt_path.display()
		);
	};

	fs::create_dir_all(receipt_dir).wrap_err_with(|| {
		format!(
			"Failed to create manual land closeout receipt directory `{}`.",
			receipt_dir.display()
		)
	})?;
	fs::write(
		&receipt_path,
		format!(
			"pr_url={pr_url}\nmerge_commit={merge_commit}\nbranch_name={branch_name}\nlanded_change={landed_change_record}\n"
		),
	)
	.wrap_err_with(|| {
		format!("Failed to write manual land closeout receipt `{}`.", receipt_path.display())
	})?;

	Ok(())
}
