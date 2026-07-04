mod environment;
mod signing;
mod source;

pub(crate) use self::{
	environment::{GitCredentialEnvironment, clear_injected_git_config},
	signing::GitSigningConfig,
	source::GitCredentialSource,
};

pub(in crate::git_credentials) const GITHUB_HTTPS_URL_BASE: &str = "https://github.com/";
pub(in crate::git_credentials) const GITHUB_SSH_URL_PREFIXES: &[&str] = &[
	"git@github.com:",
	"git@github.com-x:",
	"git@github.com-y:",
	"ssh://git@github.com/",
	"ssh://git@github.com-x/",
	"ssh://git@github.com-y/",
];
pub(in crate::git_credentials) const GIT_CONFIG_ENV_REMOVE_FLOOR: usize = 64;
pub(in crate::git_credentials) const GITHUB_CREDENTIAL_HELPER: &str = concat!(
	"!f() { ",
	"test \"$1\" = get || exit 0; ",
	"protocol=; host=; ",
	"while IFS= read -r line; do ",
	"test -n \"$line\" || break; ",
	"case \"$line\" in ",
	"protocol=*) protocol=${line#protocol=} ;; ",
	"host=*) host=${line#host=} ;; ",
	"esac; ",
	"done; ",
	"test \"$protocol\" = https || exit 0; ",
	"test \"$host\" = github.com || exit 0; ",
	"test -n \"$GH_TOKEN\" || exit 0; ",
	"printf '%s\\n' username=x-access-token password=\"$GH_TOKEN\"; ",
	"}; f",
);

#[cfg(test)]
mod tests {
	use std::{
		ffi::OsStr,
		io::Write as _,
		process::{Command, Stdio},
	};

	use crate::git_credentials::GitCredentialEnvironment;

	#[test]
	fn apply_to_scrubs_inherited_git_config_injection() {
		let mut command = Command::new("git");

		command
			.env("GIT_CONFIG_PARAMETERS", "commit.gpgsign=true")
			.env("GIT_CONFIG_COUNT", "1")
			.env("GIT_CONFIG_KEY_0", "commit.gpgsign")
			.env("GIT_CONFIG_VALUE_0", "true");

		GitCredentialEnvironment::default().apply_to(&mut command);

		assert_env_removed(&command, "GIT_CONFIG_PARAMETERS");
		assert_env_removed(&command, "GIT_CONFIG_COUNT");
		assert_env_removed(&command, "GIT_CONFIG_KEY_0");
		assert_env_removed(&command, "GIT_CONFIG_VALUE_0");
	}

	#[test]
	fn inline_github_credentials_supply_only_github_https_credentials() {
		let github = credential_fill_stdout("github.com")
			.expect("github credential helper should return credentials");

		assert!(github.contains("username=x-access-token"));
		assert!(github.contains("password=secret-token-value"));

		let foreign = credential_fill_stdout("github.com.evil")
			.expect_err("foreign host should not receive credentials");

		assert!(!foreign.contains("secret-token-value"));
	}

	fn credential_fill_stdout(host: &str) -> std::result::Result<String, String> {
		let mut command = Command::new("git");

		GitCredentialEnvironment::with_github_credentials(
			String::from("GITHUB_PAT_Y"),
			String::from("secret-token-value"),
		)
		.apply_to(&mut command);

		let mut child = command
			.args(["credential", "fill"])
			.stdin(Stdio::piped())
			.stdout(Stdio::piped())
			.stderr(Stdio::piped())
			.spawn()
			.expect("git credential fill should spawn");

		child
			.stdin
			.as_mut()
			.expect("stdin should be piped")
			.write_all(format!("protocol=https\nhost={host}\n\n").as_bytes())
			.expect("credential input should write");

		let output = child.wait_with_output().expect("credential fill should exit");
		let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
		let stderr = String::from_utf8_lossy(&output.stderr).into_owned();

		if output.status.success() { Ok(stdout) } else { Err(format!("{stdout}{stderr}")) }
	}

	fn assert_env_removed(command: &Command, name: &str) {
		let target = OsStr::new(name);

		assert!(
			command.get_envs().any(|(key, value)| key == target && value.is_none()),
			"`{name}` should be explicitly removed from child environment"
		);
	}
}
