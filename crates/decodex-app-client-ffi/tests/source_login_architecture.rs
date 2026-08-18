//! Architecture checks for the bounded in-process Codex login source adapter.

// This integration-test target validates source boundaries and therefore uses the package
// dependency graph indirectly.
use base64 as _;
use decodex_app_client_ffi as _;
use decodex_protocol as _;
use getrandom as _;
use httparse as _;
#[cfg(unix)] use libc as _;
use reqwest as _;
use serde as _;
use serde_json as _;
use sha2 as _;
use tempfile as _;
use time as _;
use tokio as _;
use toml_edit as _;
use url as _;
use zeroize as _;

use std::{fs, path::PathBuf};

fn crate_source(file: &str) -> String {
	let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src").join(file);
	fs::read_to_string(path).expect("FFI source must be readable")
}

fn production_source(source: &str) -> &str {
	source.split_once("#[cfg(test)]").map_or(source, |(production, _)| production)
}

#[test]
fn active_account_login_has_no_executable_or_terminal_boundary() {
	let manager_source = crate_source("account_reauthentication.rs");
	let bridge_source = crate_source("lib.rs");
	let manager = production_source(&manager_source);
	let bridge = production_source(&bridge_source);
	for forbidden in [
		"codex_bin",
		"std::process",
		"process::{Child",
		"spawn_login_process",
		"PseudoTerminal",
		"openpty",
		"--device-auth",
		"parse_device_prompt",
		"strip_ansi",
		"ChildStdout",
		"ChildStderr",
	] {
		assert!(
			!manager.contains(forbidden) && !bridge.contains(forbidden),
			"active login source still contains forbidden marker: {forbidden}",
		);
	}
}

#[test]
fn active_account_login_declares_one_source_adapter() {
	let manager_source = crate_source("account_reauthentication.rs");
	let bridge_source = crate_source("lib.rs");
	let manager = production_source(&manager_source);
	let bridge = production_source(&bridge_source);
	assert!(manager.contains("source_login_adapter"));
	assert!(bridge.contains("mod source_login_adapter;"));
}

#[test]
fn source_adapter_is_process_free_nonlogging_and_pins_reviewable_upstream_provenance() {
	let adapter = crate_source("source_login_adapter.rs");
	for required in [
		"9392c3fa5bcda342b5b96a1a04d67b2f781617c2",
		"login/src/pkce.rs: generate_pkce",
		"login/src/server.rs: build_authorize_url, exchange_code_for_tokens",
		"login/src/device_code_auth.rs: request_device_code",
		"login/src/auth/storage.rs: FileAuthStorage::save",
	] {
		assert!(adapter.contains(required), "missing source provenance marker: {required}");
	}
	for forbidden in [
		"std::process",
		"Command::new",
		"codex_bin",
		"openpty",
		"PseudoTerminal",
		"--device-auth",
		"parse_device_prompt",
		"println!",
		"eprintln!",
		"dbg!",
		"tracing::",
	] {
		assert!(
			!adapter.contains(forbidden),
			"source adapter contains forbidden marker: {forbidden}"
		);
	}
	for required in ["MAX_CALLBACK_REQUEST_BYTES", "MAX_CALLBACK_HEADERS", "httparse::Request"] {
		assert!(adapter.contains(required), "missing bounded callback parser marker: {required}");
	}
	let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
	let workspace_root =
		crate_root.parent().and_then(std::path::Path::parent).expect("repository root");
	for manifest in [crate_root.join("Cargo.toml"), workspace_root.join("Cargo.toml")] {
		let contents = fs::read_to_string(&manifest).expect("manifest must be readable");
		assert!(
			!contents.contains("tiny_http"),
			"unbounded callback server dependency remains in {}",
			manifest.display(),
		);
	}
}

#[test]
fn source_adapter_notice_and_license_pin_the_same_upstream_review() {
	let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
	let notice = fs::read_to_string(root.join("THIRD_PARTY_NOTICES.md"))
		.expect("third-party notice must be readable");
	let license = fs::read_to_string(root.join("third_party/openai-codex-LICENSE-APACHE"))
		.expect("upstream license must be readable");
	for required in [
		"rust-v0.148.0-alpha.9",
		"9392c3fa5bcda342b5b96a1a04d67b2f781617c2",
		"codex-rs/login/src/pkce.rs",
		"codex-rs/login/src/server.rs",
		"codex-rs/login/src/device_code_auth.rs",
		"codex-rs/login/src/auth/storage.rs",
	] {
		assert!(notice.contains(required), "missing notice marker: {required}");
	}
	assert!(license.contains("Apache License"));
	assert!(license.contains("Version 2.0, January 2004"));
}

#[test]
fn excluded_legacy_app_has_no_interactive_codex_login_process_surface() {
	let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
	let repo = manifest_dir
		.parent()
		.and_then(std::path::Path::parent)
		.expect("repository root")
		.to_path_buf();
	assert!(!repo.join("apps/decodex/src/accounts/login.rs").exists());
	for relative in [
		"apps/decodex/src/accounts.rs",
		"apps/decodex/src/accounts/types.rs",
		"apps/decodex/src/app_bridge.rs",
		"apps/decodex/src/app_bridge/request.rs",
		"apps/decodex/src/cli/account_commands.rs",
	] {
		let source =
			fs::read_to_string(repo.join(relative)).expect("legacy source must be readable");
		for forbidden in [
			"AccountLoginRequest",
			"AccountLoginCommand",
			"run_account_login",
			"account_login(",
			"codex_bin",
			"--device-auth",
		] {
			assert!(
				!source.contains(forbidden),
				"legacy login marker remains in {relative}: {forbidden}",
			);
		}
	}
}
