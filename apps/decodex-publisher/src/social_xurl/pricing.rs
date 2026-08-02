use std::{
	io::ErrorKind,
	path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest as _, Sha256};
use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};

mod fetch;
mod parser;

use super::model::{
	CREATE_COST_MICROUSD, IDENTITY_READ_COST_MICROUSD, PRICING_POLICY_ID, READ_COST_MICROUSD,
};
use crate::{
	SOCIAL_MONTHLY_BUDGET_MICROUSD, SocialRefreshPricingReport, XPricingPolicyReport,
	XPricingRatesReport,
	prelude::{Result, eyre},
};

const RECEIPT_SCHEMA: &str = "decodex/x-pricing-audit-receipt/1";
const FAILURE_RECEIPT_SCHEMA: &str = "decodex/x-pricing-audit-failure/2";
const DIAGNOSTIC_SCHEMA: &str = "decodex/x-pricing-parser-diagnostic/1";
const PARSER_CONTRACT: &str = "official-credit-consumption-tables/1";
const PARSER_VERSION: &str = "x-pricing-markdown-table/1";
const OFFICIAL_PRICING_SOURCE: &str = "https://docs.x.com/x-api/getting-started/pricing.md";
const DEFAULT_RECEIPT_PATH: &str =
	".agent/automations/decodex/cache/social/x/x-pricing-receipt.json";
const FAILURE_RECEIPT_NAME: &str = "x-pricing-failure.json";
const PRICING_LOCK_NAME: &str = ".x-pricing-refresh.lock";
const MAX_RECEIPT_BYTES: u64 = 16 * 1024;
const MAX_SOURCE_BYTES: u64 = 1024 * 1024;
const MAX_RECEIPT_AGE: Duration = Duration::hours(36);
const URL_CREATE_COST_MICROUSD: u64 = 200_000;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct XPricingRates {
	post_create: u64,
	post_create_with_url: u64,
	post_read: u64,
	user_read: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct XPricingReceipt {
	schema: String,
	parser_version: String,
	source_url: String,
	fetched_at: String,
	raw_sha256: String,
	rates_microusd: XPricingRates,
	integrity_sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct XPricingFailureReceipt {
	schema: String,
	parser_version: String,
	source_url: String,
	fetched_at: String,
	raw_sha256: String,
	error_code: String,
	diagnostic: Value,
	diagnostic_sha256: String,
	integrity_sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct XPricingDiagnostic {
	schema: String,
	parser_contract: String,
	error_code: String,
	raw_sha256: String,
	source_bytes: u64,
	source_lines: u64,
	code_fence_count: u64,
	target_section_count: u64,
	target_section_lines: u64,
	target_section_sha256: Option<String>,
	tables: Vec<XPricingDiagnosticTable>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct XPricingDiagnosticTable {
	nearest_h2: String,
	nearest_h3: String,
	header_cells: Vec<String>,
	header_sha256: String,
	row_count: u64,
	rows_sha256: String,
	sample_rows: Vec<XPricingDiagnosticRow>,
	truncated: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct XPricingDiagnosticRow {
	cells: Vec<String>,
	row_sha256: String,
}

struct VerifiedPricingReceipt {
	receipt: XPricingReceipt,
	fetched_at: OffsetDateTime,
	expires_at: OffsetDateTime,
}

struct VerifiedPricingFailure {
	fetched_at: OffsetDateTime,
}

struct StoredPricingReceipt {
	payload: Value,
	verified: VerifiedPricingReceipt,
}

struct StoredPricingFailure {
	payload: Value,
	verified: VerifiedPricingFailure,
}

pub(super) fn require_current_at(now: OffsetDateTime) -> Result<()> {
	let path = default_receipt_path()?;
	require_current_at_path(&path, now)
}

pub(super) fn report_at(now: OffsetDateTime) -> Result<XPricingPolicyReport> {
	let path = default_receipt_path()?;
	report_at_path(&path, now)
}

pub(super) fn refresh_at(now: OffsetDateTime) -> Result<SocialRefreshPricingReport> {
	let path = default_receipt_path()?;
	refresh_at_path_with(&path, now, fetch::fetch_official)
}

fn refresh_at_path_with<F>(
	path: &Path,
	now: OffsetDateTime,
	fetcher: F,
) -> Result<SocialRefreshPricingReport>
where
	F: FnOnce() -> std::result::Result<Vec<u8>, fetch::PricingFetchFailure>,
{
	let parent =
		path.parent().ok_or_else(|| eyre::eyre!("X pricing audit receipt path is invalid"))?;
	crate::ensure_private_directory(parent)?;
	let lock = crate::open_or_create_private_lock(&parent.join(PRICING_LOCK_NAME))?;
	lock.lock()?;

	let fetched_at = format_refresh_time(now);
	let recorded_at = parse_time(&fetched_at)?;
	let previous = load_optional_receipt(path)?;
	let failure_path = failure_receipt_path(path)?;
	let previous_failure = load_optional_failure_receipt(&failure_path)?;
	require_refresh_time(recorded_at, previous.as_ref(), previous_failure.as_ref())?;

	let raw = match fetcher() {
		Ok(raw) => raw,
		Err(failure) => {
			let receipt_status =
				stored_status(previous.as_ref(), previous_failure.as_ref(), recorded_at);
			let status = if receipt_status == "current" { "network_deferred" } else { "blocked" };
			return Ok(refresh_report(
				status,
				receipt_status,
				previous.as_ref().map(|stored| &stored.verified),
				Some(failure.code()),
				failure.ordinary_https_get_count(),
			));
		},
	};

	let raw_sha256 = digest(&raw);
	let rates = match parser::parse(&raw) {
		Ok(rates) => rates,
		Err(failure) => {
			let diagnostic = parser::diagnostic(&raw, failure.code());
			let diagnostic = serde_json::to_value(diagnostic)?;
			let mut receipt = XPricingFailureReceipt {
				schema: FAILURE_RECEIPT_SCHEMA.into(),
				parser_version: PARSER_VERSION.into(),
				source_url: OFFICIAL_PRICING_SOURCE.into(),
				fetched_at: fetched_at.clone(),
				raw_sha256,
				error_code: failure.code().into(),
				diagnostic_sha256: canonical_json_sha256(&diagnostic)?,
				diagnostic,
				integrity_sha256: String::new(),
			};
			receipt.integrity_sha256 = failure_integrity_sha256(&receipt);
			validate_failure_receipt(&receipt)?;
			let payload = serde_json::to_value(&receipt)?;
			write_private_json(
				&failure_path,
				previous_failure.as_ref().map(|stored| &stored.payload),
				&payload,
			)?;
			let stored = load_stored_failure_receipt(&failure_path)?;
			if stored.payload != payload || stored.verified.fetched_at != recorded_at {
				return Err(eyre::eyre!("X pricing failure receipt readback did not match"));
			}

			return Ok(SocialRefreshPricingReport {
				status: "parse_failed".into(),
				official_source: OFFICIAL_PRICING_SOURCE.into(),
				fetched_at: Some(fetched_at),
				receipt_status: "parse_failed".into(),
				rates_microusd: None,
				error_code: Some(failure.code().into()),
				ordinary_https_get_count: 1,
				x_api_call_count: 0,
				x_api_cost_microusd: 0,
			});
		},
	};

	let mut receipt = XPricingReceipt {
		schema: RECEIPT_SCHEMA.into(),
		parser_version: PARSER_VERSION.into(),
		source_url: OFFICIAL_PRICING_SOURCE.into(),
		fetched_at,
		raw_sha256,
		rates_microusd: rates,
		integrity_sha256: String::new(),
	};
	receipt.integrity_sha256 = integrity_sha256(&receipt);
	validate_receipt(&receipt)?;
	let payload = serde_json::to_value(&receipt)?;
	write_private_json(path, previous.as_ref().map(|stored| &stored.payload), &payload)?;
	let stored = load_stored_receipt(path)?;
	if stored.payload != payload || stored.verified.receipt != receipt {
		return Err(eyre::eyre!("X pricing success receipt readback did not match"));
	}
	remove_failure_receipt(&failure_path, previous_failure.as_ref())?;
	let receipt_status = status_at(&stored.verified, None, recorded_at);

	Ok(refresh_report(receipt_status, receipt_status, Some(&stored.verified), None, 1))
}

fn default_receipt_path() -> Result<PathBuf> {
	Ok(crate::repo_root()?.join(DEFAULT_RECEIPT_PATH))
}

fn format_refresh_time(value: OffsetDateTime) -> String {
	format!(
		"{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
		value.year(),
		u8::from(value.month()),
		value.day(),
		value.hour(),
		value.minute(),
		value.second()
	)
}

fn require_refresh_time(
	now: OffsetDateTime,
	previous: Option<&StoredPricingReceipt>,
	previous_failure: Option<&StoredPricingFailure>,
) -> Result<()> {
	if previous.is_some_and(|stored| stored.verified.fetched_at >= now)
		|| previous_failure.is_some_and(|stored| stored.verified.fetched_at >= now)
	{
		return Err(eyre::eyre!("X pricing refresh timestamp must advance stored evidence"));
	}
	Ok(())
}

fn refresh_report(
	status: &str,
	receipt_status: &str,
	receipt: Option<&VerifiedPricingReceipt>,
	error_code: Option<&str>,
	ordinary_https_get_count: u64,
) -> SocialRefreshPricingReport {
	SocialRefreshPricingReport {
		status: status.into(),
		official_source: OFFICIAL_PRICING_SOURCE.into(),
		fetched_at: receipt.map(|verified| verified.receipt.fetched_at.clone()),
		receipt_status: receipt_status.into(),
		rates_microusd: receipt.map(|verified| rates_report(&verified.receipt.rates_microusd)),
		error_code: error_code.map(str::to_owned),
		ordinary_https_get_count,
		x_api_call_count: 0,
		x_api_cost_microusd: 0,
	}
}

fn rates_report(rates: &XPricingRates) -> XPricingRatesReport {
	XPricingRatesReport {
		post_create: rates.post_create,
		post_create_with_url: rates.post_create_with_url,
		post_read: rates.post_read,
		user_read: rates.user_read,
	}
}

fn stored_status(
	receipt: Option<&StoredPricingReceipt>,
	failure: Option<&StoredPricingFailure>,
	now: OffsetDateTime,
) -> &'static str {
	match receipt {
		Some(receipt) =>
			status_at(&receipt.verified, failure.map(|failure| &failure.verified), now),
		None if failure.is_some() => "parse_failed",
		None => "missing",
	}
}

fn write_private_json(path: &Path, previous: Option<&Value>, payload: &Value) -> Result<()> {
	let encoded = serde_json::to_vec_pretty(payload)?;
	if encoded.len().saturating_add(1) > MAX_RECEIPT_BYTES as usize {
		return Err(eyre::eyre!("X pricing receipt exceeds its bounded size"));
	}
	if let Some(previous) = previous {
		crate::replace_existing_json(path, previous, payload)
	} else {
		crate::write_new_json(path, payload)
	}
}

fn remove_failure_receipt(path: &Path, previous: Option<&StoredPricingFailure>) -> Result<()> {
	let Some(previous) = previous else { return Ok(()) };
	let pinned = crate::filesystem::PinnedPrivateJsonFile::open(path, MAX_RECEIPT_BYTES)?;
	if pinned.payload != previous.payload {
		return Err(eyre::eyre!("X pricing failure receipt changed before cleanup"));
	}
	let receipt: XPricingFailureReceipt = serde_json::from_value(pinned.payload.clone())
		.map_err(|_| eyre::eyre!("X pricing failure receipt contract is invalid"))?;
	validate_failure_receipt(&receipt)?;
	pinned.unlink()
}

fn require_current_at_path(path: &Path, now: OffsetDateTime) -> Result<()> {
	let failure = load_failure_receipt(&failure_receipt_path(path)?)?;
	let verified = match load_receipt(path) {
		Ok(receipt) => receipt,
		Err(_) if failure.is_some() => {
			return Err(eyre::eyre!("X pricing policy is not current: parse_failed"));
		},
		Err(error) => return Err(error),
	};
	let status = status_at(&verified, failure.as_ref(), now);
	if status != "current" {
		return Err(eyre::eyre!("X pricing policy is not current: {status}"));
	}

	Ok(())
}

fn report_at_path(path: &Path, now: OffsetDateTime) -> Result<XPricingPolicyReport> {
	let failure = load_failure_receipt(&failure_receipt_path(path)?)?;
	let verified = match load_receipt(path) {
		Ok(receipt) => receipt,
		Err(_) if failure.is_some() => {
			return Err(eyre::eyre!("X pricing policy is not current: parse_failed"));
		},
		Err(error) => return Err(error),
	};
	let status = status_at(&verified, failure.as_ref(), now);
	Ok(XPricingPolicyReport {
		policy_id: PRICING_POLICY_ID.into(),
		official_source: OFFICIAL_PRICING_SOURCE.into(),
		reviewed_at: verified.receipt.fetched_at.clone(),
		effective_at: verified.receipt.fetched_at.clone(),
		expires_at: verified
			.expires_at
			.format(&Rfc3339)
			.map_err(|_| eyre::eyre!("X pricing receipt expiry is invalid"))?,
		status: status.into(),
		user_read_cost_microusd: verified.receipt.rates_microusd.user_read,
		url_free_content_create_cost_microusd: verified.receipt.rates_microusd.post_create,
		post_read_cost_ceiling_microusd: verified.receipt.rates_microusd.post_read,
		monthly_reservation_cap_microusd: SOCIAL_MONTHLY_BUDGET_MICROUSD,
	})
}

fn failure_receipt_path(success_path: &Path) -> Result<PathBuf> {
	let parent = success_path
		.parent()
		.ok_or_else(|| eyre::eyre!("X pricing audit receipt path is invalid"))?;
	Ok(parent.join(FAILURE_RECEIPT_NAME))
}

fn load_receipt(path: &Path) -> Result<VerifiedPricingReceipt> {
	Ok(load_stored_receipt(path)?.verified)
}

fn load_optional_receipt(path: &Path) -> Result<Option<StoredPricingReceipt>> {
	match path.symlink_metadata() {
		Ok(_) => load_stored_receipt(path).map(Some),
		Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
		Err(error) => Err(eyre::eyre!("X pricing audit receipt is unavailable: {error}")),
	}
}

fn load_stored_receipt(path: &Path) -> Result<StoredPricingReceipt> {
	let (payload, _receipt_sha256) = crate::load_json_with_sha256_bounded(path, MAX_RECEIPT_BYTES)
		.map_err(|error| eyre::eyre!("X pricing audit receipt is unavailable: {error}"))?;
	let receipt: XPricingReceipt = serde_json::from_value(payload)
		.map_err(|_| eyre::eyre!("X pricing audit receipt contract is invalid"))?;
	validate_receipt(&receipt)?;
	let fetched_at = parse_time(&receipt.fetched_at)?;
	let expires_at = fetched_at
		.checked_add(MAX_RECEIPT_AGE)
		.ok_or_else(|| eyre::eyre!("X pricing receipt expiry is invalid"))?;

	let payload = serde_json::to_value(&receipt)?;
	Ok(StoredPricingReceipt {
		payload,
		verified: VerifiedPricingReceipt { receipt, fetched_at, expires_at },
	})
}

fn load_failure_receipt(path: &Path) -> Result<Option<VerifiedPricingFailure>> {
	Ok(load_optional_failure_receipt(path)?.map(|stored| stored.verified))
}

fn load_optional_failure_receipt(path: &Path) -> Result<Option<StoredPricingFailure>> {
	match path.symlink_metadata() {
		Ok(_) => {},
		Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
		Err(error) => {
			return Err(eyre::eyre!("X pricing failure receipt is unavailable: {error}"));
		},
	}
	load_stored_failure_receipt(path).map(Some)
}

fn load_stored_failure_receipt(path: &Path) -> Result<StoredPricingFailure> {
	let (payload, _receipt_sha256) = crate::load_json_with_sha256_bounded(path, MAX_RECEIPT_BYTES)
		.map_err(|error| eyre::eyre!("X pricing failure receipt is unavailable: {error}"))?;
	let receipt: XPricingFailureReceipt = serde_json::from_value(payload)
		.map_err(|_| eyre::eyre!("X pricing failure receipt contract is invalid"))?;
	validate_failure_receipt(&receipt)?;
	let fetched_at = parse_time(&receipt.fetched_at)?;
	let payload = serde_json::to_value(&receipt)?;
	Ok(StoredPricingFailure { payload, verified: VerifiedPricingFailure { fetched_at } })
}

fn validate_receipt(receipt: &XPricingReceipt) -> Result<()> {
	if receipt.schema != RECEIPT_SCHEMA
		|| receipt.parser_version != PARSER_VERSION
		|| receipt.source_url != OFFICIAL_PRICING_SOURCE
		|| !lowercase_digest(&receipt.raw_sha256)
		|| !lowercase_digest(&receipt.integrity_sha256)
		|| receipt.integrity_sha256 != integrity_sha256(receipt)
		|| receipt.rates_microusd.post_create == 0
		|| receipt.rates_microusd.post_create_with_url == 0
		|| receipt.rates_microusd.post_read == 0
		|| receipt.rates_microusd.user_read == 0
		|| receipt.rates_microusd.post_create > 10_000_000
		|| receipt.rates_microusd.post_create_with_url > 10_000_000
		|| receipt.rates_microusd.post_read > 10_000_000
		|| receipt.rates_microusd.user_read > 10_000_000
	{
		return Err(eyre::eyre!("X pricing audit receipt contract is invalid"));
	}
	parse_time(&receipt.fetched_at)?;

	Ok(())
}

fn validate_failure_receipt(receipt: &XPricingFailureReceipt) -> Result<()> {
	let diagnostic: XPricingDiagnostic = serde_json::from_value(receipt.diagnostic.clone())
		.map_err(|_| eyre::eyre!("X pricing failure receipt contract is invalid"))?;
	if receipt.schema != FAILURE_RECEIPT_SCHEMA
		|| receipt.parser_version != PARSER_VERSION
		|| receipt.source_url != OFFICIAL_PRICING_SOURCE
		|| !lowercase_digest(&receipt.raw_sha256)
		|| !failure_code(&receipt.error_code)
		|| !lowercase_digest(&receipt.diagnostic_sha256)
		|| receipt.diagnostic_sha256 != canonical_json_sha256(&receipt.diagnostic)?
		|| !lowercase_digest(&receipt.integrity_sha256)
		|| receipt.integrity_sha256 != failure_integrity_sha256(receipt)
		|| diagnostic.schema != DIAGNOSTIC_SCHEMA
		|| diagnostic.parser_contract != PARSER_CONTRACT
		|| diagnostic.error_code != receipt.error_code
		|| diagnostic.raw_sha256 != receipt.raw_sha256
		|| !valid_diagnostic(&diagnostic)
	{
		return Err(eyre::eyre!("X pricing failure receipt contract is invalid"));
	}
	parse_time(&receipt.fetched_at)?;
	Ok(())
}

fn valid_diagnostic(diagnostic: &XPricingDiagnostic) -> bool {
	if diagnostic.source_bytes > MAX_SOURCE_BYTES
		|| diagnostic.source_lines > MAX_SOURCE_BYTES + 1
		|| diagnostic.code_fence_count > diagnostic.source_lines
		|| diagnostic.target_section_count > diagnostic.source_lines
		|| diagnostic.target_section_lines > diagnostic.source_lines
		|| (diagnostic.target_section_count == 0
			&& (diagnostic.target_section_lines != 0 || diagnostic.target_section_sha256.is_some()))
		|| (diagnostic.target_section_lines > 0 && diagnostic.target_section_sha256.is_none())
		|| diagnostic.target_section_sha256.as_ref().is_some_and(|digest| !lowercase_digest(digest))
		|| diagnostic.tables.len() > 4
	{
		return false;
	}
	for table in &diagnostic.tables {
		if !bounded_diagnostic_text(&table.nearest_h2)
			|| !bounded_diagnostic_text(&table.nearest_h3)
			|| table.header_cells.len() > 4
			|| table.header_cells.iter().any(|cell| !bounded_diagnostic_text(cell))
			|| !lowercase_digest(&table.header_sha256)
			|| table.row_count > diagnostic.source_lines
			|| !lowercase_digest(&table.rows_sha256)
			|| table.sample_rows.len() > 8
			|| table.sample_rows.len() as u64 > table.row_count
			|| table.truncated != ((table.sample_rows.len() as u64) < table.row_count)
		{
			return false;
		}
		for row in &table.sample_rows {
			if row.cells.len() > 2
				|| row.cells.iter().any(|cell| !bounded_diagnostic_text(cell))
				|| !lowercase_digest(&row.row_sha256)
			{
				return false;
			}
		}
	}
	true
}

fn bounded_diagnostic_text(value: &str) -> bool {
	value.len() <= 64 && value.bytes().all(|byte| matches!(byte, b' '..=b'~'))
}

fn failure_code(value: &str) -> bool {
	(1..=128).contains(&value.len())
		&& value.starts_with("x_pricing_")
		&& value.bytes().all(|byte| byte.is_ascii_lowercase() || byte == b'_')
}

fn status_at(
	receipt: &VerifiedPricingReceipt,
	failure: Option<&VerifiedPricingFailure>,
	now: OffsetDateTime,
) -> &'static str {
	if failure.is_some_and(|failure| failure.fetched_at >= receipt.fetched_at) {
		return "parse_failed";
	}
	if now < receipt.fetched_at {
		return "future";
	}
	if now > receipt.expires_at {
		return "stale";
	}
	if receipt.receipt.rates_microusd.post_read != READ_COST_MICROUSD
		|| receipt.receipt.rates_microusd.user_read != IDENTITY_READ_COST_MICROUSD
		|| receipt.receipt.rates_microusd.post_create != CREATE_COST_MICROUSD
		|| receipt.receipt.rates_microusd.post_create_with_url != URL_CREATE_COST_MICROUSD
		|| SOCIAL_MONTHLY_BUDGET_MICROUSD != 1_250_000
	{
		return "contract_drift";
	}

	"current"
}

fn integrity_sha256(receipt: &XPricingReceipt) -> String {
	let rates = &receipt.rates_microusd;
	let material = format!(
		"schema={}\nparser_version={}\nsource_url={}\nfetched_at={}\nraw_sha256={}\npost_create={}\npost_create_with_url={}\npost_read={}\nuser_read={}",
		receipt.schema,
		receipt.parser_version,
		receipt.source_url,
		receipt.fetched_at,
		receipt.raw_sha256,
		rates.post_create,
		rates.post_create_with_url,
		rates.post_read,
		rates.user_read,
	);
	Sha256::digest(material.as_bytes()).iter().map(|byte| format!("{byte:02x}")).collect()
}

fn failure_integrity_sha256(receipt: &XPricingFailureReceipt) -> String {
	let material = format!(
		"schema={}\nparser_version={}\nsource_url={}\nfetched_at={}\nraw_sha256={}\nerror_code={}\ndiagnostic_sha256={}",
		receipt.schema,
		receipt.parser_version,
		receipt.source_url,
		receipt.fetched_at,
		receipt.raw_sha256,
		receipt.error_code,
		receipt.diagnostic_sha256,
	);
	Sha256::digest(material.as_bytes()).iter().map(|byte| format!("{byte:02x}")).collect()
}

fn canonical_json_sha256(value: &Value) -> Result<String> {
	let mut canonical = String::new();
	write_canonical_json(value, &mut canonical)?;
	Ok(Sha256::digest(canonical.as_bytes()).iter().map(|byte| format!("{byte:02x}")).collect())
}

fn write_canonical_json(value: &Value, output: &mut String) -> Result<()> {
	match value {
		Value::Null => output.push_str("null"),
		Value::Bool(value) => output.push_str(if *value { "true" } else { "false" }),
		Value::Number(value) => output.push_str(&value.to_string()),
		Value::String(value) => output.push_str(
			&serde_json::to_string(value)
				.map_err(|_| eyre::eyre!("X pricing diagnostic string is invalid"))?,
		),
		Value::Array(values) => {
			output.push('[');
			for (index, value) in values.iter().enumerate() {
				if index > 0 {
					output.push(',');
				}
				write_canonical_json(value, output)?;
			}
			output.push(']');
		},
		Value::Object(values) => {
			output.push('{');
			let mut keys: Vec<_> = values.keys().collect();
			keys.sort_unstable();
			for (index, key) in keys.into_iter().enumerate() {
				if index > 0 {
					output.push(',');
				}
				output.push_str(
					&serde_json::to_string(key)
						.map_err(|_| eyre::eyre!("X pricing diagnostic key is invalid"))?,
				);
				output.push(':');
				write_canonical_json(&values[key], output)?;
			}
			output.push('}');
		},
	}
	Ok(())
}

fn parse_time(value: &str) -> Result<OffsetDateTime> {
	if value.len() != 20 || !value.ends_with('Z') {
		return Err(eyre::eyre!("X pricing receipt fetched_at is invalid"));
	}
	OffsetDateTime::parse(value, &Rfc3339)
		.map_err(|_| eyre::eyre!("X pricing receipt fetched_at is invalid"))
}

fn lowercase_digest(value: &str) -> bool {
	value.len() == 64
		&& value.bytes().all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn digest(bytes: &[u8]) -> String {
	Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)] mod tests;
