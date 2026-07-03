use crate::{BUNDLE_SCHEMA, Value, eyre, prelude::Result};

pub(super) fn validate_bundle_value(bundle: &Value) -> Result<()> {
	let validation = crate::validate_artifact(bundle);

	if validation.errors.is_empty() && validation.schema.as_deref() == Some(BUNDLE_SCHEMA) {
		Ok(())
	} else {
		let mut errors = validation.errors;

		if validation.schema.as_deref() != Some(BUNDLE_SCHEMA) {
			errors.insert(0, format!("schema must be {BUNDLE_SCHEMA}"));
		}

		eyre::bail!("Bundle validation failed:\n- {}", errors.join("\n- "))
	}
}
