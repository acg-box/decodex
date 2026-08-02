use std::path::Path;

use sha2::{Digest as _, Sha256};

use crate::{
	BUNDLE_BUILD_RECEIPT_SCHEMA, BUNDLE_SCHEMA, CACHE_MAX_BYTES_PER_COLLECTION,
	RadarBundleBuildReceipt, Value,
	prelude::{Result, eyre},
};

pub(crate) fn install_bundle(path: &Path, bundle: &Value) -> Result<RadarBundleBuildReceipt> {
	install_bundle_with(path, bundle, || {})
}

fn install_bundle_with<AfterWrite>(
	path: &Path,
	bundle: &Value,
	after_write: AfterWrite,
) -> Result<RadarBundleBuildReceipt>
where
	AfterWrite: FnOnce(),
{
	crate::validate_expected_schema(bundle, BUNDLE_SCHEMA, "Bundle")?;
	let expected = pretty_json_bytes(bundle)?;

	validate_bundle_size(expected.len())?;
	let _ = bundle_evidence_from_bytes(&expected)?;

	if !crate::is_radar_cache_path(path) {
		eyre::bail!("bundle installation requires a private Radar cache path");
	}
	let (cache, relative) = crate::private_fs::private_cache_file(path)?;
	let lock = cache.lock()?;

	lock.write_atomic(&relative, &expected)?;
	after_write();
	let installed = lock.read_bounded(&relative, CACHE_MAX_BYTES_PER_COLLECTION)?;

	receipt_from_installed_bytes(&expected, &installed)
}

fn receipt_from_installed_bytes(
	expected: &[u8],
	installed: &[u8],
) -> Result<RadarBundleBuildReceipt> {
	if installed != expected {
		eyre::bail!("installed bundle bytes do not match the deterministic build output");
	}
	let (_, receipt) = bundle_evidence_from_bytes(installed)?;

	Ok(receipt)
}

pub(crate) fn bundle_evidence_from_bytes(bytes: &[u8]) -> Result<(Value, RadarBundleBuildReceipt)> {
	validate_bundle_size(bytes.len())?;
	let bundle: Value = serde_json::from_slice(bytes)
		.map_err(|error| eyre::eyre!("bundle JSON is invalid: {error}"))?;

	crate::validate_expected_schema(&bundle, BUNDLE_SCHEMA, "Bundle")?;
	let object = bundle.as_object().ok_or_else(|| eyre::eyre!("bundle must be an object"))?;
	let analysis_mode = object
		.get("analysis_mode")
		.and_then(Value::as_str)
		.ok_or_else(|| eyre::eyre!("bundle analysis_mode must be a string"))?
		.to_owned();
	let commits = required_array(object.get("commits"), "commits")?;
	let files = required_array(object.get("files"), "files")?;
	let patch_excerpt_count = files.iter().try_fold(0_u32, |count, file| {
		let file =
			file.as_object().ok_or_else(|| eyre::eyre!("bundle files must contain objects"))?;

		match file.get("patch_excerpt") {
			Some(Value::String(value)) if !value.trim().is_empty() => count
				.checked_add(1)
				.ok_or_else(|| eyre::eyre!("bundle patch excerpt count exceeds u32")),
			Some(Value::String(_) | Value::Null) | None => Ok(count),
			Some(_) => {
				eyre::bail!("bundle file patch_excerpt must be a string or null when present")
			},
		}
	})?;
	let commit_count = bounded_count(commits.len(), "commit")?;
	let file_count = bounded_count(files.len(), "file")?;
	let docs_ref_count = string_array_count(object.get("docs_refs"), "docs_refs")?;
	let examples_ref_count = string_array_count(object.get("examples_refs"), "examples_refs")?;

	validate_reference_count(docs_ref_count, file_count, "docs_ref")?;
	validate_reference_count(examples_ref_count, file_count, "examples_ref")?;

	let receipt = RadarBundleBuildReceipt {
		schema: BUNDLE_BUILD_RECEIPT_SCHEMA.to_owned(),
		status: "installed".to_owned(),
		bundle_sha256: sha256_hex(bytes),
		bundle_bytes: u64::try_from(bytes.len())
			.map_err(|_| eyre::eyre!("installed bundle byte count exceeds u64"))?,
		analysis_mode,
		commit_count,
		file_count,
		patch_excerpt_count,
		docs_ref_count,
		examples_ref_count,
	};

	Ok((bundle, receipt))
}

fn required_array<'a>(value: Option<&'a Value>, label: &str) -> Result<&'a Vec<Value>> {
	value.and_then(Value::as_array).ok_or_else(|| eyre::eyre!("bundle {label} must be a list"))
}

fn string_array_count(value: Option<&Value>, label: &str) -> Result<u32> {
	let values = required_array(value, label)?;

	if values.iter().any(|value| value.as_str().is_none()) {
		eyre::bail!("bundle {label} must contain only strings");
	}

	bounded_count(values.len(), label)
}

fn bounded_count(count: usize, label: &str) -> Result<u32> {
	u32::try_from(count).map_err(|_| eyre::eyre!("bundle {label} count exceeds u32"))
}

fn validate_reference_count(count: u32, file_count: u32, label: &str) -> Result<()> {
	if count > file_count {
		eyre::bail!("bundle {label} count cannot exceed file count");
	}

	Ok(())
}

fn validate_bundle_size(size: usize) -> Result<()> {
	if u64::try_from(size).unwrap_or(u64::MAX) > CACHE_MAX_BYTES_PER_COLLECTION {
		eyre::bail!("bundle exceeds the {}-byte evidence limit", CACHE_MAX_BYTES_PER_COLLECTION);
	}

	Ok(())
}

fn pretty_json_bytes(value: &Value) -> Result<Vec<u8>> {
	let mut bytes = serde_json::to_vec_pretty(value)?;

	bytes.push(b'\n');
	Ok(bytes)
}

fn sha256_hex(payload: &[u8]) -> String {
	Sha256::digest(payload).iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
pub(crate) fn install_bundle_after_write(
	path: &Path,
	bundle: &Value,
	after_write: impl FnOnce(),
) -> Result<RadarBundleBuildReceipt> {
	install_bundle_with(path, bundle, after_write)
}
