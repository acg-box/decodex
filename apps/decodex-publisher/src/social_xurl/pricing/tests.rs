use std::{
	fs,
	os::unix::fs::PermissionsExt as _,
	path::{Path, PathBuf},
};

use serde_json::json;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use super::{
	DIAGNOSTIC_SCHEMA, FAILURE_RECEIPT_NAME, FAILURE_RECEIPT_SCHEMA, OFFICIAL_PRICING_SOURCE,
	PARSER_CONTRACT, PARSER_VERSION, RECEIPT_SCHEMA, XPricingFailureReceipt, XPricingRates,
	XPricingReceipt, canonical_json_sha256, failure_integrity_sha256, integrity_sha256,
	refresh_at_path_with, report_at_path, require_current_at_path,
};

const CURRENT_FIXTURE: &str = include_str!("fixtures/current.md");

fn at(value: &str) -> OffsetDateTime {
	OffsetDateTime::parse(value, &Rfc3339).expect("test timestamp")
}

fn write_receipt(
	root: &Path,
	fetched_at: &str,
	rates: XPricingRates,
) -> (PathBuf, serde_json::Value) {
	let path = root.join("private/x-pricing-receipt.json");
	fs::create_dir_all(path.parent().expect("receipt parent")).expect("private directory");
	let mut receipt = XPricingReceipt {
		schema: RECEIPT_SCHEMA.into(),
		parser_version: PARSER_VERSION.into(),
		source_url: OFFICIAL_PRICING_SOURCE.into(),
		fetched_at: fetched_at.into(),
		raw_sha256: "a".repeat(64),
		rates_microusd: rates,
		integrity_sha256: String::new(),
	};
	receipt.integrity_sha256 = integrity_sha256(&receipt);
	let value = json!({
		"schema": receipt.schema,
		"parser_version": receipt.parser_version,
		"source_url": receipt.source_url,
		"fetched_at": receipt.fetched_at,
		"raw_sha256": receipt.raw_sha256,
		"rates_microusd": {
			"post_create": receipt.rates_microusd.post_create,
			"post_create_with_url": receipt.rates_microusd.post_create_with_url,
			"post_read": receipt.rates_microusd.post_read,
			"user_read": receipt.rates_microusd.user_read,
		},
		"integrity_sha256": receipt.integrity_sha256,
	});
	fs::write(&path, format!("{}\n", serde_json::to_string_pretty(&value).expect("receipt JSON")))
		.expect("write receipt");
	fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).expect("private receipt");
	(path, value)
}

fn current_rates() -> XPricingRates {
	XPricingRates {
		post_create: 15_000,
		post_create_with_url: 200_000,
		post_read: 5_000,
		user_read: 10_000,
	}
}

fn write_failure(root: &Path, fetched_at: &str) -> (PathBuf, serde_json::Value) {
	let path = root.join("private").join(FAILURE_RECEIPT_NAME);
	fs::create_dir_all(path.parent().expect("failure receipt parent")).expect("private directory");
	let raw_sha256 = "b".repeat(64);
	let error_code = "x_pricing_operation_table_header_invalid";
	let diagnostic = json!({
		"schema": DIAGNOSTIC_SCHEMA,
		"parser_contract": PARSER_CONTRACT,
		"error_code": error_code,
		"raw_sha256": raw_sha256,
		"source_bytes": 2048,
		"source_lines": 40,
		"code_fence_count": 0,
		"target_section_count": 1,
		"target_section_lines": 25,
		"target_section_sha256": "c".repeat(64),
		"tables": [{
			"nearest_h2": "## Credit consumption details",
			"nearest_h3": "### Write operations",
			"header_cells": ["Action", "Price per 1,000 requests"],
			"header_sha256": "d".repeat(64),
			"row_count": 1,
			"rows_sha256": "e".repeat(64),
			"sample_rows": [{
				"cells": ["**Post: Create**", "\\$0.015 per request"],
				"row_sha256": "f".repeat(64),
			}],
			"truncated": false,
		}],
	});
	let diagnostic_sha256 = canonical_json_sha256(&diagnostic).expect("diagnostic digest");
	assert_eq!(
		diagnostic_sha256,
		"b97ba1dba25b06285bd7d7e6d6a6858662fa3dc329ff659096310edeeb603fbd"
	);
	let mut receipt = XPricingFailureReceipt {
		schema: FAILURE_RECEIPT_SCHEMA.into(),
		parser_version: PARSER_VERSION.into(),
		source_url: OFFICIAL_PRICING_SOURCE.into(),
		fetched_at: fetched_at.into(),
		raw_sha256,
		error_code: error_code.into(),
		diagnostic,
		diagnostic_sha256,
		integrity_sha256: String::new(),
	};
	receipt.integrity_sha256 = failure_integrity_sha256(&receipt);
	let value = json!({
		"schema": receipt.schema,
		"parser_version": receipt.parser_version,
		"source_url": receipt.source_url,
		"fetched_at": receipt.fetched_at,
		"raw_sha256": receipt.raw_sha256,
		"error_code": receipt.error_code,
		"diagnostic": receipt.diagnostic,
		"diagnostic_sha256": receipt.diagnostic_sha256,
		"integrity_sha256": receipt.integrity_sha256,
	});
	fs::write(&path, format!("{}\n", serde_json::to_string_pretty(&value).expect("failure JSON")))
		.expect("write failure receipt");
	fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).expect("private failure receipt");
	(path, value)
}

#[test]
fn current_receipt_renews_the_policy_without_a_code_expiry() {
	let temp = tempfile::tempdir().expect("temporary directory");
	let (path, _) = write_receipt(temp.path(), "2035-01-01T00:00:00Z", current_rates());
	let report = report_at_path(&path, at("2035-01-02T12:00:00Z")).expect("current receipt");
	assert_eq!(report.status, "current");
	assert_eq!(report.official_source, OFFICIAL_PRICING_SOURCE);
	assert_eq!(report.reviewed_at, "2035-01-01T00:00:00Z");
	assert_eq!(report.expires_at, "2035-01-02T12:00:00Z");
	assert_eq!(report.post_read_cost_ceiling_microusd, 5_000);
	assert_eq!(report.user_read_cost_microusd, 10_000);
	assert_eq!(report.url_free_content_create_cost_microusd, 15_000);
	assert_eq!(report.monthly_reservation_cap_microusd, 1_250_000);
	require_current_at_path(&path, at("2035-01-02T12:00:00Z"))
		.expect("36-hour boundary remains current");
}

#[test]
fn missing_stale_future_and_tampered_receipts_fail_closed() {
	let temp = tempfile::tempdir().expect("temporary directory");
	let missing = temp.path().join("missing.json");
	assert!(require_current_at_path(&missing, at("2026-07-27T00:00:00Z")).is_err());

	let (path, original) = write_receipt(temp.path(), "2026-07-27T00:00:00Z", current_rates());
	let stale = require_current_at_path(&path, at("2026-07-28T12:00:01Z"))
		.expect_err("stale receipt")
		.to_string();
	assert!(stale.contains("not current: stale"), "{stale}");
	let future = require_current_at_path(&path, at("2026-07-26T23:59:59Z"))
		.expect_err("future receipt")
		.to_string();
	assert!(future.contains("not current: future"), "{future}");

	let mut tampered = original;
	tampered["fetched_at"] = json!("2036-01-01T00:00:00Z");
	fs::write(
		&path,
		format!("{}\n", serde_json::to_string_pretty(&tampered).expect("tampered JSON")),
	)
	.expect("tamper receipt");
	let error = require_current_at_path(&path, at("2036-01-01T01:00:00Z"))
		.expect_err("integrity mismatch")
		.to_string();
	assert!(error.contains("contract is invalid"), "{error}");
}

#[test]
fn changed_official_rate_and_unsafe_mode_stop_paid_calls() {
	let temp = tempfile::tempdir().expect("temporary directory");
	let changed = XPricingRates { post_create_with_url: 250_000, ..current_rates() };
	let (path, _) = write_receipt(temp.path(), "2026-07-27T00:00:00Z", changed);
	let report = report_at_path(&path, at("2026-07-27T01:00:00Z")).expect("drift report");
	assert_eq!(report.status, "contract_drift");
	let error = require_current_at_path(&path, at("2026-07-27T01:00:00Z"))
		.expect_err("rate drift")
		.to_string();
	assert!(error.contains("not current: contract_drift"), "{error}");

	fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).expect("unsafe mode");
	let error = report_at_path(&path, at("2026-07-27T01:00:00Z"))
		.expect_err("unsafe receipt mode")
		.to_string();
	assert!(error.contains("receipt is unavailable"), "{error}");
}

#[test]
fn newest_parse_failure_immediately_stops_paid_calls() {
	let temp = tempfile::tempdir().expect("temporary directory");
	let (path, _) = write_receipt(temp.path(), "2026-07-27T00:00:00Z", current_rates());
	let _ = write_failure(temp.path(), "2026-07-27T00:00:00Z");

	let report = report_at_path(&path, at("2026-07-27T01:00:00Z")).expect("failure report");
	assert_eq!(report.status, "parse_failed");
	let error = require_current_at_path(&path, at("2026-07-27T01:00:00Z"))
		.expect_err("same-time parser failure must win")
		.to_string();
	assert!(error.contains("not current: parse_failed"), "{error}");

	let failure_only = tempfile::tempdir().expect("temporary directory");
	let _ = write_failure(failure_only.path(), "2026-07-27T00:00:00Z");
	let missing_success = failure_only.path().join("private/x-pricing-receipt.json");
	let error = require_current_at_path(&missing_success, at("2026-07-27T01:00:00Z"))
		.expect_err("failure without prior success")
		.to_string();
	assert!(error.contains("not current: parse_failed"), "{error}");
}

#[test]
fn older_failure_is_ignored_but_malformed_or_unsafe_markers_fail_closed() {
	let temp = tempfile::tempdir().expect("temporary directory");
	let (path, _) = write_receipt(temp.path(), "2026-07-27T00:00:00Z", current_rates());
	let (failure_path, mut failure) = write_failure(temp.path(), "2026-07-26T23:59:59Z");
	require_current_at_path(&path, at("2026-07-27T01:00:00Z"))
		.expect("newer success supersedes an older valid failure");

	failure["diagnostic"]["tables"][0]["header_cells"][0] = json!("Changed");
	fs::write(
		&failure_path,
		format!("{}\n", serde_json::to_string_pretty(&failure).expect("tampered failure JSON")),
	)
	.expect("tamper failure receipt");
	let error = require_current_at_path(&path, at("2026-07-27T01:00:00Z"))
		.expect_err("malformed failure marker")
		.to_string();
	assert!(error.contains("failure receipt contract is invalid"), "{error}");

	let _ = write_failure(temp.path(), "2026-07-26T23:59:59Z");
	fs::set_permissions(&failure_path, fs::Permissions::from_mode(0o644))
		.expect("unsafe failure mode");
	let error = require_current_at_path(&path, at("2026-07-27T01:00:00Z"))
		.expect_err("unsafe failure marker")
		.to_string();
	assert!(error.contains("failure receipt is unavailable"), "{error}");
}

#[test]
fn current_official_fixture_parses_and_refreshes_a_private_receipt_without_x_calls() {
	let rates = super::parser::parse(CURRENT_FIXTURE.as_bytes()).expect("current fixture");
	assert_eq!(rates, current_rates());

	let temp = tempfile::tempdir().expect("temporary directory");
	let path = temp.path().join("private/x-pricing-receipt.json");
	let report = refresh_at_path_with(&path, at("2026-08-02T12:00:00Z"), || {
		Ok(CURRENT_FIXTURE.as_bytes().to_vec())
	})
	.expect("pricing refresh");
	assert_eq!(report.status, "current");
	assert_eq!(report.receipt_status, "current");
	assert_eq!(report.ordinary_https_get_count, 1);
	assert_eq!(report.x_api_call_count, 0);
	assert_eq!(report.x_api_cost_microusd, 0);
	assert_eq!(report.rates_microusd.expect("parsed rates").post_create, 15_000);
	let metadata = fs::symlink_metadata(&path).expect("receipt metadata");
	assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
	require_current_at_path(&path, at("2026-08-03T23:59:59Z")).expect("renewed receipt is current");
}

#[test]
fn malformed_and_duplicate_pricing_tables_fail_closed_with_a_newer_marker() {
	let malformed = CURRENT_FIXTURE.replace("| Resource | Unit cost |", "| Resource | Price |");
	let malformed_error = super::parser::parse(malformed.as_bytes()).expect_err("malformed header");
	assert_eq!(malformed_error.code(), "x_pricing_operation_table_header_invalid");
	let duplicate = CURRENT_FIXTURE.replace(
		"| **Posts: Read** | \\$0.005 per resource |",
		"| **Posts: Read** | \\$0.005 per resource |\n| **Posts: Read** | \\$0.005 per resource |",
	);
	let duplicate_error = super::parser::parse(duplicate.as_bytes()).expect_err("duplicate label");
	assert_eq!(duplicate_error.code(), "x_pricing_row_duplicate");

	let temp = tempfile::tempdir().expect("temporary directory");
	let path = temp.path().join("private/x-pricing-receipt.json");
	refresh_at_path_with(&path, at("2026-08-02T12:00:00Z"), || {
		Ok(CURRENT_FIXTURE.as_bytes().to_vec())
	})
	.expect("initial success");
	let report =
		refresh_at_path_with(&path, at("2026-08-02T12:01:00Z"), || Ok(malformed.into_bytes()))
			.expect("bounded parse failure report");
	assert_eq!(report.status, "parse_failed");
	assert_eq!(report.error_code.as_deref(), Some("x_pricing_operation_table_header_invalid"));
	assert_eq!(report.x_api_call_count, 0);
	let failure_path = path.parent().expect("pricing parent").join(FAILURE_RECEIPT_NAME);
	let failure_metadata = fs::symlink_metadata(&failure_path).expect("failure marker");
	assert_eq!(failure_metadata.permissions().mode() & 0o777, 0o600);
	assert!(failure_metadata.len() <= 16 * 1024);
	let blocked = require_current_at_path(&path, at("2026-08-02T12:01:01Z"))
		.expect_err("newer parse marker blocks publishing")
		.to_string();
	assert!(blocked.contains("not current: parse_failed"), "{blocked}");
	let success = crate::load_json(&path).expect("preserved success receipt");
	assert_eq!(success["fetched_at"], "2026-08-02T12:00:00Z");
}

#[test]
fn successful_renewal_replaces_the_receipt_and_removes_an_older_failure() {
	let temp = tempfile::tempdir().expect("temporary directory");
	let path = temp.path().join("private/x-pricing-receipt.json");
	refresh_at_path_with(&path, at("2026-08-02T12:00:00Z"), || {
		Ok(CURRENT_FIXTURE.as_bytes().to_vec())
	})
	.expect("initial success");
	let malformed = CURRENT_FIXTURE.replace("### Write operations", "### Writes");
	refresh_at_path_with(&path, at("2026-08-02T12:01:00Z"), || Ok(malformed.into_bytes()))
		.expect("parse failure");
	let failure_path = path.parent().expect("pricing parent").join(FAILURE_RECEIPT_NAME);
	assert!(failure_path.exists());

	let renewed = refresh_at_path_with(&path, at("2026-08-02T12:02:00Z"), || {
		Ok(CURRENT_FIXTURE.as_bytes().to_vec())
	})
	.expect("renewed success");
	assert_eq!(renewed.status, "current");
	assert_eq!(renewed.fetched_at.as_deref(), Some("2026-08-02T12:02:00Z"));
	assert!(!failure_path.exists(), "successful refresh removes the older marker");
	let success = crate::load_json(&path).expect("renewed success receipt");
	assert_eq!(success["fetched_at"], "2026-08-02T12:02:00Z");
}

#[test]
fn network_failure_preserves_only_a_real_cached_receipt() {
	let temp = tempfile::tempdir().expect("temporary directory");
	let path = temp.path().join("private/x-pricing-receipt.json");
	refresh_at_path_with(&path, at("2026-08-02T12:00:00Z"), || {
		Ok(CURRENT_FIXTURE.as_bytes().to_vec())
	})
	.expect("initial success");
	let before = fs::read(&path).expect("cached receipt bytes");
	let deferred = refresh_at_path_with(&path, at("2026-08-02T13:00:00Z"), || {
		Err(super::fetch::PricingFetchFailure::network_for_test())
	})
	.expect("deferred network failure");
	assert_eq!(deferred.status, "network_deferred");
	assert_eq!(deferred.receipt_status, "current");
	assert_eq!(deferred.fetched_at.as_deref(), Some("2026-08-02T12:00:00Z"));
	assert_eq!(fs::read(&path).expect("preserved receipt bytes"), before);

	let blocked = refresh_at_path_with(&path, at("2026-08-04T00:00:01Z"), || {
		Err(super::fetch::PricingFetchFailure::network_for_test())
	})
	.expect("stale network failure report");
	assert_eq!(blocked.status, "blocked");
	assert_eq!(blocked.receipt_status, "stale");
	assert_eq!(fs::read(&path).expect("still preserved receipt bytes"), before);

	let missing_root = tempfile::tempdir().expect("missing receipt directory");
	let missing_path = missing_root.path().join("private/x-pricing-receipt.json");
	let missing = refresh_at_path_with(&missing_path, at("2026-08-02T12:00:00Z"), || {
		Err(super::fetch::PricingFetchFailure::network_for_test())
	})
	.expect("missing cache report");
	assert_eq!(missing.status, "blocked");
	assert_eq!(missing.receipt_status, "missing");
	assert!(missing.rates_microusd.is_none());
	assert!(!missing_path.exists());
}

#[test]
fn changed_official_rates_are_recorded_as_contract_drift() {
	let temp = tempfile::tempdir().expect("temporary directory");
	let path = temp.path().join("private/x-pricing-receipt.json");
	let changed = CURRENT_FIXTURE.replace("\\$0.015 per request", "\\$0.016 per request");
	let report =
		refresh_at_path_with(&path, at("2026-08-02T12:00:00Z"), || Ok(changed.into_bytes()))
			.expect("contract drift report");
	assert_eq!(report.status, "contract_drift");
	assert_eq!(report.receipt_status, "contract_drift");
	assert_eq!(report.rates_microusd.expect("changed rates").post_create, 16_000);
	let blocked = require_current_at_path(&path, at("2026-08-02T12:01:00Z"))
		.expect_err("changed rate blocks paid calls")
		.to_string();
	assert!(blocked.contains("not current: contract_drift"), "{blocked}");
}
