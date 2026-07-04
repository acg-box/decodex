use crate::{
	orchestrator::status::{env, eyre},
	prelude::Result,
};

pub(crate) fn resolve_configured_env_var(
	field_name: &str,
	env_var: Option<&str>,
) -> Result<String> {
	let env_var = env_var.ok_or_else(|| {
		eyre::eyre!("`{field_name}` must be configured for this GitHub-backed operation.")
	})?;
	let value = env::var(env_var).map_err(|error| {
		eyre::eyre!(
			"Failed to read environment variable `{env_var}` referenced by `{field_name}`: {error}"
		)
	})?;

	if value.trim().is_empty() {
		eyre::bail!(
			"Environment variable `{env_var}` referenced by `{field_name}` must not be blank."
		);
	}

	Ok(value)
}
