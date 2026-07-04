pub(crate) const SENSITIVE_PHRASES: &[&str] = &[
	"account=",
	"account_fingerprint",
	"account fingerprint",
	"api key",
	"auth token",
	"codex.github-identity",
	"codex.linear-workspace",
	"credential",
	"github identity",
	"github-identity",
	"linear workspace",
	"linear-workspace",
	"process_start_identity",
	"routed identity",
	"selected account",
	"token=",
];
pub(crate) const HOST_PATH_PREFIXES: &[&str] = &[
	"/home/",
	"/private/",
	"/root/",
	"/tmp/",
	"/users/",
	"/var/folders/",
	"/volumes/",
	"file:///",
];
pub(crate) const CREDENTIAL_MARKERS: &[&str] = &[
	"API_KEY",
	"AUTH_JSON",
	"CREDENTIAL",
	"GITHUB_PAT",
	"LINEAR_API_KEY",
	"PASSWD",
	"PASSWORD",
	"SECRET",
	"TOKEN",
];
pub(crate) const PRIVATE_CONFIG_FILES: &[&str] = &["auth.json", "accounts.jsonl"];
pub(crate) const PRIVATE_ENV_VAR_TOKENS: &[&str] = &["CODEX_HOME", "CODEX_SQLITE_HOME"];
