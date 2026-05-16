#[cfg(unix)] use std::os::unix::fs::PermissionsExt as _;
use std::{
	env, fs,
	io::ErrorKind,
	path::{Path, PathBuf},
	process::{self, Command},
};

use serde::{Deserialize, Serialize};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
	prelude::{Result, eyre},
	runtime,
};

pub(crate) struct AccountLoginRequest {
	pub(crate) codex_bin: String,
	pub(crate) keep_temp_home: bool,
}

pub(crate) struct AccountImportRequest {
	pub(crate) auth_json_path: PathBuf,
	pub(crate) json: bool,
}

struct AccountStore {
	accounts_path: PathBuf,
	global_config_path: PathBuf,
}
impl AccountStore {
	fn global() -> Result<Self> {
		Ok(Self {
			accounts_path: runtime::accounts_path()?,
			global_config_path: runtime::global_config_path()?,
		})
	}

	#[cfg(test)]
	fn new(accounts_path: PathBuf, global_config_path: PathBuf) -> Self {
		Self { accounts_path, global_config_path }
	}

	fn list(&self) -> Result<AccountListResponse> {
		let records = self.load_records()?;

		self.response_from_records(&records)
	}

	fn select(&self, selector: &str) -> Result<AccountListResponse> {
		let selector = selector.trim();

		if selector.is_empty() {
			eyre::bail!("Codex account selector cannot be empty.");
		}

		let records = self.load_records()?;

		if !records.iter().any(|record| record.matches_account_selector(selector)) {
			eyre::bail!("No Decodex account matches selector `{selector}`.");
		}

		self.write_fixed_account_selector(Some(selector))?;

		self.response_from_records(&records)
	}

	fn clear_selection(&self) -> Result<AccountListResponse> {
		let records = self.load_records()?;

		self.write_fixed_account_selector(None)?;

		self.response_from_records(&records)
	}

	fn logout(&self, selector: &str) -> Result<AccountListResponse> {
		let selector = selector.trim();

		if selector.is_empty() {
			eyre::bail!("Codex account selector cannot be empty.");
		}

		let mut records = self.load_records()?;
		let selector_matched_fixed =
			self.fixed_account_selector()?.as_deref().is_some_and(|fixed| {
				fixed == selector
					|| records.iter().any(|record| {
						record.matches_account_selector(selector)
							&& record.matches_account_selector(fixed)
					})
			});
		let original_len = records.len();

		records.retain(|record| !record.matches_account_selector(selector));

		if records.len() == original_len {
			eyre::bail!("No Decodex account matches selector `{selector}`.");
		}

		self.save_records(&records)?;

		if selector_matched_fixed {
			self.write_fixed_account_selector(None)?;
		}

		self.response_from_records(&records)
	}

	fn import_auth_json(&self, auth_json_path: &Path) -> Result<AccountListResponse> {
		let input = fs::read_to_string(auth_json_path).map_err(|error| {
			eyre::eyre!("Failed to read Codex auth JSON `{}`: {error}", auth_json_path.display())
		})?;
		let auth = serde_json::from_str::<AuthDotJson>(&input).map_err(|error| {
			eyre::eyre!("Codex auth JSON `{}` is invalid: {error}", auth_json_path.display())
		})?;
		let mut record = AccountPoolRecord::from_auth(auth)?;
		let mut records = self.load_records()?;

		if record.last_refresh.is_none() {
			record.last_refresh = Some(now_rfc3339()?);
		}

		let replace_index = records.iter().position(|candidate| {
			record.account_id().is_some() && candidate.account_id() == record.account_id()
				|| record.email().is_some() && candidate.email() == record.email()
		});

		if let Some(index) = replace_index {
			records[index] = record;
		} else {
			records.push(record);
		}

		self.save_records(&records)?;

		self.response_from_records(&records)
	}

	fn load_records(&self) -> Result<Vec<AccountPoolRecord>> {
		let input = match fs::read_to_string(&self.accounts_path) {
			Ok(input) => input,
			Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
			Err(error) => {
				eyre::bail!(
					"Failed to read Decodex accounts `{}`: {error}",
					self.accounts_path.display()
				);
			},
		};

		parse_account_records(&input, &self.accounts_path)
	}

	fn save_records(&self, records: &[AccountPoolRecord]) -> Result<()> {
		let parent = self.accounts_path.parent().ok_or_else(|| {
			eyre::eyre!(
				"Decodex accounts path `{}` must have a parent directory.",
				self.accounts_path.display()
			)
		})?;
		let file_name =
			self.accounts_path.file_name().and_then(|name| name.to_str()).ok_or_else(|| {
				eyre::eyre!("Decodex accounts path must end in a valid file name.")
			})?;
		let temp_path = parent.join(format!(".{file_name}.tmp-{}", process::id()));
		let mut body = String::new();

		for record in records {
			body.push_str(&serde_json::to_string(record)?);
			body.push('\n');
		}

		fs::create_dir_all(parent)?;
		fs::write(&temp_path, body)?;

		secure_account_file(&temp_path)?;

		fs::rename(temp_path, &self.accounts_path)?;

		secure_account_file(&self.accounts_path)?;

		Ok(())
	}

	fn response_from_records(&self, records: &[AccountPoolRecord]) -> Result<AccountListResponse> {
		let selector = self.fixed_account_selector()?;
		let control = AccountControlSummary {
			mode: if selector.is_some() { String::from("fixed") } else { String::from("balanced") },
			account_selector: selector.clone(),
		};
		let accounts =
			records.iter().map(|record| record.summary(selector.as_deref())).collect::<Vec<_>>();

		Ok(AccountListResponse {
			accounts_path: self.accounts_path.display().to_string(),
			global_config_path: self.global_config_path.display().to_string(),
			control,
			accounts,
		})
	}

	fn fixed_account_selector(&self) -> Result<Option<String>> {
		let input = match fs::read_to_string(&self.global_config_path) {
			Ok(input) => input,
			Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
			Err(error) => {
				eyre::bail!(
					"Failed to read Decodex global config `{}`: {error}",
					self.global_config_path.display()
				);
			},
		};
		let document = toml::from_str::<toml::Table>(&input)?;
		let selector = document
			.get("codex")
			.and_then(toml::Value::as_table)
			.and_then(|codex| codex.get("accounts"))
			.and_then(toml::Value::as_table)
			.and_then(|accounts| accounts.get("fixed_account"))
			.and_then(toml::Value::as_str)
			.map(str::trim)
			.filter(|value| !value.is_empty())
			.map(str::to_owned);

		Ok(selector)
	}

	fn write_fixed_account_selector(&self, selector: Option<&str>) -> Result<()> {
		let input = match fs::read_to_string(&self.global_config_path) {
			Ok(input) => input,
			Err(error) if error.kind() == ErrorKind::NotFound => String::new(),
			Err(error) => {
				eyre::bail!(
					"Failed to read Decodex global config `{}`: {error}",
					self.global_config_path.display()
				);
			},
		};
		let mut document = if input.trim().is_empty() {
			toml::Table::new()
		} else {
			toml::from_str::<toml::Table>(&input)?
		};

		match selector.map(str::trim).filter(|value| !value.is_empty()) {
			Some(selector) => {
				let accounts =
					ensure_toml_table(ensure_toml_table(&mut document, "codex")?, "accounts")?;

				accounts.insert(String::from("fixed_account"), selector.to_owned().into());
			},
			None => {
				if let Some(codex) = document.get_mut("codex").and_then(toml::Value::as_table_mut)
					&& let Some(accounts) =
						codex.get_mut("accounts").and_then(toml::Value::as_table_mut)
				{
					accounts.remove("fixed_account");
				}
			},
		}

		let parent = self.global_config_path.parent().ok_or_else(|| {
			eyre::eyre!(
				"Decodex global config `{}` must have a parent directory.",
				self.global_config_path.display()
			)
		})?;
		let file_name =
			self.global_config_path.file_name().and_then(|name| name.to_str()).ok_or_else(
				|| eyre::eyre!("Decodex global config path must end in a valid file name."),
			)?;
		let temp_path = parent.join(format!(".{file_name}.tmp-{}", process::id()));
		let output = toml::to_string_pretty(&document)?;

		fs::create_dir_all(parent)?;
		fs::write(&temp_path, output)?;

		secure_account_file(&temp_path)?;

		fs::rename(temp_path, &self.global_config_path)?;

		secure_account_file(&self.global_config_path)?;

		Ok(())
	}
}

#[derive(Serialize)]
struct AccountListResponse {
	accounts_path: String,
	global_config_path: String,
	control: AccountControlSummary,
	accounts: Vec<AccountSummary>,
}

#[derive(Serialize)]
struct AccountControlSummary {
	mode: String,
	account_selector: Option<String>,
}

#[derive(Serialize)]
struct AccountSummary {
	account_fingerprint: String,
	email: Option<String>,
	selector: String,
	status: String,
	selected: bool,
	disabled: bool,
	refresh_token_present: bool,
	access_token_expires_at_unix_epoch: Option<i64>,
	last_selected_at_unix_epoch: Option<i64>,
	cooldown_until_unix_epoch: Option<i64>,
	note: Option<String>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(untagged)]
enum AccountPoolLine {
	Wrapped {
		#[serde(skip_serializing_if = "Option::is_none")]
		email: Option<String>,
		#[serde(default, skip_serializing_if = "is_false")]
		disabled: bool,
		#[serde(skip_serializing_if = "Option::is_none")]
		cooldown_until_unix_epoch: Option<i64>,
		#[serde(skip_serializing_if = "Option::is_none")]
		cooldown_until: Option<String>,
		#[serde(skip_serializing_if = "Option::is_none")]
		last_selected_at_unix_epoch: Option<i64>,
		auth: AuthDotJson,
	},
	Flat(AccountPoolRecord),
}
impl AccountPoolLine {
	fn into_record(self) -> Result<AccountPoolRecord> {
		match self {
			Self::Flat(record) => Ok(record),
			Self::Wrapped {
				email,
				disabled,
				cooldown_until_unix_epoch,
				cooldown_until,
				last_selected_at_unix_epoch,
				auth,
			} => {
				let mut record = AccountPoolRecord::from_auth(auth)?;

				record.email = first_nonblank_string(email, record.email);
				record.disabled = disabled;
				record.cooldown_until_unix_epoch = cooldown_until_unix_epoch;
				record.cooldown_until = cooldown_until;
				record.last_selected_at_unix_epoch = last_selected_at_unix_epoch;

				Ok(record)
			},
		}
	}
}

#[derive(Clone, Deserialize, Serialize)]
struct AuthDotJson {
	#[serde(skip_serializing_if = "Option::is_none")]
	email: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	auth_mode: Option<String>,
	#[serde(rename = "OPENAI_API_KEY", skip_serializing_if = "Option::is_none")]
	openai_api_key: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	tokens: Option<CodexTokenData>,
	#[serde(skip_serializing_if = "Option::is_none")]
	last_refresh: Option<String>,
}

#[derive(Clone, Deserialize, Serialize)]
struct AccountPoolRecord {
	#[serde(skip_serializing_if = "Option::is_none")]
	email: Option<String>,
	#[serde(default, skip_serializing_if = "is_false")]
	disabled: bool,
	#[serde(skip_serializing_if = "Option::is_none")]
	cooldown_until_unix_epoch: Option<i64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	cooldown_until: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	last_selected_at_unix_epoch: Option<i64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	auth_mode: Option<String>,
	#[serde(rename = "OPENAI_API_KEY", skip_serializing_if = "Option::is_none")]
	openai_api_key: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	tokens: Option<CodexTokenData>,
	#[serde(skip_serializing_if = "Option::is_none")]
	last_refresh: Option<String>,
}
impl AccountPoolRecord {
	fn from_auth(auth: AuthDotJson) -> Result<Self> {
		let record = Self {
			email: first_nonblank_string(
				auth.email,
				auth.tokens.as_ref().and_then(|tokens| {
					nonblank_string(tokens.email.as_deref())
						.or_else(|| jwt_email_claim(tokens.id_token.as_deref()))
				}),
			),
			disabled: false,
			cooldown_until_unix_epoch: None,
			cooldown_until: None,
			last_selected_at_unix_epoch: None,
			auth_mode: auth.auth_mode,
			openai_api_key: auth.openai_api_key,
			tokens: auth.tokens,
			last_refresh: auth.last_refresh,
		};

		record.validate_importable()?;

		Ok(record)
	}

	fn validate_importable(&self) -> Result<()> {
		if self
			.tokens
			.as_ref()
			.and_then(|tokens| nonblank_string(Some(&tokens.access_token)))
			.is_none()
		{
			eyre::bail!("Codex auth JSON is missing `tokens.access_token`.");
		}
		if self
			.tokens
			.as_ref()
			.and_then(|tokens| nonblank_string(Some(&tokens.refresh_token)))
			.is_none()
		{
			eyre::bail!("Codex auth JSON is missing `tokens.refresh_token`.");
		}
		if self.account_id().is_none() {
			eyre::bail!("Codex auth JSON is missing `tokens.account_id`.");
		}

		Ok(())
	}

	fn matches_account_selector(&self, selector: &str) -> bool {
		let selector = selector.trim();

		self.email().as_deref() == Some(selector)
			|| self.account_id() == Some(selector)
			|| self.account_id().map(redact_account_id).as_deref() == Some(selector)
	}

	fn account_id(&self) -> Option<&str> {
		self.tokens
			.as_ref()
			.and_then(|tokens| tokens.account_id.as_deref())
			.filter(|account_id| !account_id.trim().is_empty())
	}

	fn email(&self) -> Option<String> {
		nonblank_string(self.email.as_deref())
			.or_else(|| {
				self.tokens.as_ref().and_then(|tokens| nonblank_string(tokens.email.as_deref()))
			})
			.or_else(|| {
				self.tokens.as_ref().and_then(|tokens| jwt_email_claim(tokens.id_token.as_deref()))
			})
	}

	fn summary(&self, fixed_selector: Option<&str>) -> AccountSummary {
		let now = OffsetDateTime::now_utc().unix_timestamp();
		let account_fingerprint = self
			.account_id()
			.map(redact_account_id)
			.or_else(|| self.email())
			.unwrap_or_else(|| String::from("unknown"));
		let selector = self.email().unwrap_or_else(|| account_fingerprint.clone());
		let selected = fixed_selector.is_some_and(|fixed| self.matches_account_selector(fixed));
		let access_token_expires_at_unix_epoch =
			self.tokens.as_ref().and_then(|tokens| jwt_expiration_unix_epoch(&tokens.access_token));
		let refresh_token_present = self
			.tokens
			.as_ref()
			.and_then(|tokens| nonblank_string(Some(&tokens.refresh_token)))
			.is_some();
		let status = if self.disabled {
			"disabled"
		} else if self.cooldown_until_unix_epoch.is_some_and(|cooldown_until| cooldown_until > now)
		{
			"cooldown"
		} else if access_token_expires_at_unix_epoch.is_some_and(|expires_at| expires_at <= now) {
			"expired"
		} else if self.account_id().is_none() || !refresh_token_present {
			"unusable"
		} else {
			"available"
		};

		AccountSummary {
			account_fingerprint,
			email: self.email(),
			selector,
			status: status.to_owned(),
			selected,
			disabled: self.disabled,
			refresh_token_present,
			access_token_expires_at_unix_epoch,
			last_selected_at_unix_epoch: self.last_selected_at_unix_epoch,
			cooldown_until_unix_epoch: self.cooldown_until_unix_epoch,
			note: Some(String::from("local account pool")),
		}
	}
}

#[derive(Clone, Deserialize, Serialize)]
struct CodexTokenData {
	#[serde(skip_serializing_if = "Option::is_none")]
	email: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	id_token: Option<String>,
	access_token: String,
	refresh_token: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	account_id: Option<String>,
}

pub(crate) fn run_account_list(json: bool) -> Result<()> {
	let store = AccountStore::global()?;

	print_list_response(&store.list()?, json)
}

pub(crate) fn run_account_select(selector: &str, json: bool) -> Result<()> {
	let store = AccountStore::global()?;
	let response = store.select(selector)?;

	print_list_response(&response, json)
}

pub(crate) fn run_account_clear(json: bool) -> Result<()> {
	let store = AccountStore::global()?;
	let response = store.clear_selection()?;

	print_list_response(&response, json)
}

pub(crate) fn run_account_logout(selector: &str, json: bool) -> Result<()> {
	let store = AccountStore::global()?;
	let response = store.logout(selector)?;

	print_list_response(&response, json)
}

pub(crate) fn run_account_import(request: &AccountImportRequest) -> Result<()> {
	let store = AccountStore::global()?;
	let response = store.import_auth_json(&request.auth_json_path)?;

	print_list_response(&response, request.json)
}

pub(crate) fn run_account_login(request: &AccountLoginRequest) -> Result<()> {
	let temp_home = create_login_home()?;
	let status = Command::new(&request.codex_bin)
		.arg("login")
		.arg("--device-auth")
		.env("CODEX_HOME", &temp_home)
		.env("CODEX_SQLITE_HOME", &temp_home)
		.status()
		.map_err(|error| {
			eyre::eyre!("Failed to start `{}` for Codex account login: {error}", request.codex_bin)
		})?;

	if !status.success() {
		cleanup_login_home(&temp_home, request.keep_temp_home);

		eyre::bail!("Codex account login failed with status {status}.");
	}

	let auth_json_path = temp_home.join("auth.json");
	let store = AccountStore::global()?;
	let import_result = store.import_auth_json(&auth_json_path);

	cleanup_login_home(&temp_home, request.keep_temp_home);

	let response = import_result?;

	print_list_response(&response, false)
}

fn parse_account_records(input: &str, path: &Path) -> Result<Vec<AccountPoolRecord>> {
	let mut records = Vec::new();

	for (line_index, line) in input.lines().enumerate() {
		let line_number = line_index + 1;
		let trimmed = line.trim();

		if trimmed.is_empty() || trimmed.starts_with('#') {
			continue;
		}

		let parsed = serde_json::from_str::<AccountPoolLine>(trimmed).map_err(|error| {
			eyre::eyre!(
				"Decodex accounts `{}` line {line_number} is not a valid auth JSONL entry: {error}",
				path.display()
			)
		})?;

		records.push(parsed.into_record()?);
	}

	Ok(records)
}

fn print_list_response(response: &AccountListResponse, json: bool) -> Result<()> {
	if json {
		println!("{}", serde_json::to_string_pretty(response)?);

		return Ok(());
	}

	println!(
		"Codex account pool: {} ({})",
		response.control.mode,
		response.control.account_selector.as_deref().unwrap_or("balanced selection")
	);
	println!("accounts: {}", response.accounts.len());

	for account in &response.accounts {
		let marker = if account.selected { "*" } else { "-" };
		let email = account.email.as_deref().unwrap_or("no email");

		println!("{marker} {email} {} {}", account.account_fingerprint, account.status);
	}

	Ok(())
}

fn create_login_home() -> Result<PathBuf> {
	let root = env::temp_dir().join(format!(
		"decodex-codex-login-{}-{}",
		process::id(),
		OffsetDateTime::now_utc().unix_timestamp()
	));

	fs::create_dir_all(&root)?;

	secure_account_file(&root)?;

	Ok(root)
}

fn cleanup_login_home(path: &Path, keep: bool) {
	if keep {
		eprintln!("temporary Codex login home preserved at {}", path.display());

		return;
	}

	if let Err(error) = fs::remove_dir_all(path) {
		eprintln!(
			"warning: failed to remove temporary Codex login home `{}`: {error}",
			path.display()
		);
	}
}

fn secure_account_file(path: &Path) -> Result<()> {
	#[cfg(unix)]
	{
		let mode = if path.is_dir() { 0o700 } else { 0o600 };
		let mut permissions = fs::metadata(path)?.permissions();

		permissions.set_mode(mode);

		fs::set_permissions(path, permissions)?;
	}

	Ok(())
}

fn ensure_toml_table<'a>(parent: &'a mut toml::Table, key: &str) -> Result<&'a mut toml::Table> {
	if !parent.contains_key(key) {
		parent.insert(String::from(key), toml::Value::Table(toml::Table::new()));
	}

	parent
		.get_mut(key)
		.and_then(toml::Value::as_table_mut)
		.ok_or_else(|| eyre::eyre!("`{key}` in Decodex global config must be a table."))
}

fn first_nonblank_string(left: Option<String>, right: Option<String>) -> Option<String> {
	left.filter(|value| !value.trim().is_empty())
		.or_else(|| right.filter(|value| !value.trim().is_empty()))
}

fn nonblank_string(value: Option<&str>) -> Option<String> {
	value.map(str::trim).filter(|value| !value.is_empty()).map(str::to_owned)
}

fn jwt_email_claim(id_token: Option<&str>) -> Option<String> {
	let payload = id_token?.split('.').nth(1)?;
	let payload_bytes = parse_base64_url(payload)?;
	let claims = serde_json::from_slice::<serde_json::Value>(&payload_bytes).ok()?;

	claims.get("email").and_then(json_scalar_to_string)
}

fn jwt_expiration_unix_epoch(jwt: &str) -> Option<i64> {
	let payload = jwt.split('.').nth(1)?;
	let payload_bytes = parse_base64_url(payload)?;
	let claims = serde_json::from_slice::<serde_json::Value>(&payload_bytes).ok()?;

	claims.get("exp").and_then(number_as_i64)
}

fn parse_base64_url(input: &str) -> Option<Vec<u8>> {
	let mut output = Vec::with_capacity(input.len() * 3 / 4);
	let mut accumulator = 0_u32;
	let mut bits = 0_u32;

	for byte in input.bytes().take_while(|byte| *byte != b'=') {
		accumulator = (accumulator << 6) | u32::from(base64_url_value(byte)?);
		bits += 6;

		if bits >= 8 {
			bits -= 8;

			output.push(((accumulator >> bits) & 0xff) as u8);
		}
	}

	Some(output)
}

const fn base64_url_value(byte: u8) -> Option<u8> {
	match byte {
		b'A'..=b'Z' => Some(byte - b'A'),
		b'a'..=b'z' => Some(byte - b'a' + 26),
		b'0'..=b'9' => Some(byte - b'0' + 52),
		b'-' => Some(62),
		b'_' => Some(63),
		_ => None,
	}
}

fn json_scalar_to_string(value: &serde_json::Value) -> Option<String> {
	match value {
		serde_json::Value::String(text) if !text.is_empty() => Some(text.clone()),
		serde_json::Value::Number(number) => Some(number.to_string()),
		serde_json::Value::Bool(value) => Some(value.to_string()),
		_ => None,
	}
}

fn number_as_i64(value: &serde_json::Value) -> Option<i64> {
	value
		.as_i64()
		.or_else(|| value.as_u64().and_then(|number| i64::try_from(number).ok()))
		.or_else(|| value.as_f64().map(|number| number.round() as i64))
}

fn redact_account_id(account_id: &str) -> String {
	let tail =
		account_id.chars().rev().take(6).collect::<Vec<_>>().into_iter().rev().collect::<String>();

	if tail.is_empty() { String::from("unknown") } else { format!("...{tail}") }
}

fn now_rfc3339() -> Result<String> {
	Ok(OffsetDateTime::now_utc().format(&Rfc3339)?)
}

const fn is_false(value: &bool) -> bool {
	!*value
}

#[cfg(test)]
mod tests {
	use std::fs;

	use tempfile::TempDir;

	use crate::accounts::{AccountPoolRecord, AccountStore, CodexTokenData};

	#[test]
	fn imports_auth_json_without_printing_tokens() {
		let temp_dir = TempDir::new().expect("temp dir should create");
		let auth_path = temp_dir.path().join("auth.json");
		let store = AccountStore::new(
			temp_dir.path().join("accounts.jsonl"),
			temp_dir.path().join("config.toml"),
		);

		fs::write(
			&auth_path,
			r#"{
				"email": "copy@example.com",
				"tokens": {
					"access_token": "header.eyJleHAiOjQxMDI0NDQ4MDB9.sig",
					"refresh_token": "refresh-secret",
					"account_id": "acct_123456"
				}
			}"#,
		)
		.expect("auth json should write");

		let response = store.import_auth_json(&auth_path).expect("auth should import");
		let output = serde_json::to_string(&response).expect("response should serialize");

		assert_eq!(response.accounts.len(), 1);
		assert!(output.contains("copy@example.com"));
		assert!(output.contains("...123456"));
		assert!(!output.contains("refresh-secret"));
	}

	#[test]
	fn logout_removes_matching_account() {
		let temp_dir = TempDir::new().expect("temp dir should create");
		let store = AccountStore::new(
			temp_dir.path().join("accounts.jsonl"),
			temp_dir.path().join("config.toml"),
		);

		store
			.save_records(&[AccountPoolRecord {
				email: Some(String::from("copy@example.com")),
				disabled: false,
				cooldown_until_unix_epoch: None,
				cooldown_until: None,
				last_selected_at_unix_epoch: None,
				auth_mode: None,
				openai_api_key: None,
				tokens: Some(CodexTokenData {
					email: None,
					id_token: None,
					access_token: String::from("token"),
					refresh_token: String::from("refresh"),
					account_id: Some(String::from("acct_123456")),
				}),
				last_refresh: None,
			}])
			.expect("records should save");

		let response = store.logout("copy@example.com").expect("account should logout");

		assert!(response.accounts.is_empty());
		assert_eq!(fs::read_to_string(&store.accounts_path).expect("accounts should read"), "");
	}
}
