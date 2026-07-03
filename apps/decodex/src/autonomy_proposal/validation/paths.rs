use std::path::{Component, Path};

pub(super) fn surface_allowed(intended_surface: &str, allowed_surfaces: &[String]) -> bool {
	let Some(intended_surface) = normalize_repo_relative_path(intended_surface) else {
		return false;
	};

	allowed_surfaces.iter().any(|surface| {
		normalize_repo_relative_path(surface).is_some_and(|surface| {
			intended_surface == surface
				|| intended_surface
					.strip_prefix(&surface)
					.is_some_and(|suffix| suffix.starts_with('/'))
		})
	})
}

pub(super) fn normalize_repo_relative_path(value: &str) -> Option<String> {
	let path = Path::new(value);

	if path.is_absolute() {
		return None;
	}

	let mut parts = Vec::new();

	for component in path.components() {
		let Component::Normal(part) = component else {
			return None;
		};

		parts.push(part.to_str()?);
	}

	if parts.is_empty() {
		return None;
	}

	Some(parts.join("/"))
}
