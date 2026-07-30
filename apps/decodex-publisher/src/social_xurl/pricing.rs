use std::{
	io::ErrorKind,
	path::{Path, PathBuf},
};

use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest as _, Sha256};
use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};

use super::model::{
	CREATE_COST_MICROUSD, IDENTITY_READ_COST_MICROUSD, PRICING_POLICY_ID, READ_COST_MICROUSD,
};
use crate::{
	SOCIAL_MONTHLY_BUDGET_MICROUSD, XPricingPolicyReport,
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
const MAX_RECEIPT_BYTES: u64 = 16 * 1024;
const MAX_SOURCE_BYTES: u64 = 1024 * 1024;
const MAX_RECEIPT_AGE: Duration = Duration::hours(36);
const URL_CREATE_COST_MICROUSD: u64 = 200_000;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct XPricingRates {
	post_create: u64,
	post_create_with_url: u64,
	post_read: u64,
	user_read: u64,
}

#[derive(Clone, Debug, Deserialize)]
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

#[derive(Clone, Debug, Deserialize)]
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

#[derive(Clone, Debug, Deserialize)]
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

#[derive(Clone, Debug, Deserialize)]
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

#[derive(Clone, Debug, Deserialize)]
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

pub(super) fn require_current_at(now: OffsetDateTime) -> Result<()> {
	let path = default_receipt_path()?;
	require_current_at_path(&path, now)
}

pub(super) fn report_at(now: OffsetDateTime) -> Result<XPricingPolicyReport> {
	let path = default_receipt_path()?;
	report_at_path(&path, now)
}

fn default_receipt_path() -> Result<PathBuf> {
	Ok(crate::repo_root()?.join(DEFAULT_RECEIPT_PATH))
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
	let (payload, _receipt_sha256) = crate::load_json_with_sha256_bounded(path, MAX_RECEIPT_BYTES)
		.map_err(|error| eyre::eyre!("X pricing audit receipt is unavailable: {error}"))?;
	let receipt: XPricingReceipt = serde_json::from_value(payload)
		.map_err(|_| eyre::eyre!("X pricing audit receipt contract is invalid"))?;
	validate_receipt(&receipt)?;
	let fetched_at = parse_time(&receipt.fetched_at)?;
	let expires_at = fetched_at
		.checked_add(MAX_RECEIPT_AGE)
		.ok_or_else(|| eyre::eyre!("X pricing receipt expiry is invalid"))?;

	Ok(VerifiedPricingReceipt { receipt, fetched_at, expires_at })
}

fn load_failure_receipt(path: &Path) -> Result<Option<VerifiedPricingFailure>> {
	match path.symlink_metadata() {
		Ok(_) => {},
		Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
		Err(error) => {
			return Err(eyre::eyre!("X pricing failure receipt is unavailable: {error}"));
		},
	}
	let (payload, _receipt_sha256) = crate::load_json_with_sha256_bounded(path, MAX_RECEIPT_BYTES)
		.map_err(|error| eyre::eyre!("X pricing failure receipt is unavailable: {error}"))?;
	let receipt: XPricingFailureReceipt = serde_json::from_value(payload)
		.map_err(|_| eyre::eyre!("X pricing failure receipt contract is invalid"))?;
	validate_failure_receipt(&receipt)?;
	let fetched_at = parse_time(&receipt.fetched_at)?;
	Ok(Some(VerifiedPricingFailure { fetched_at }))
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

#[cfg(test)] mod tests;
