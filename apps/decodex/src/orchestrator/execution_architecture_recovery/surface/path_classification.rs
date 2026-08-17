use crate::orchestrator::execution_architecture_recovery::AuthorityBoundarySurface;

pub(super) fn architecture_recovery_surfaces_for_path(
	relative_path: &str,
) -> Vec<AuthorityBoundarySurface> {
	let normalized = relative_path.replace('\\', "/");
	let lower = normalized.to_ascii_lowercase();
	let mut surfaces = Vec::new();

	if architecture_recovery_path_is_test(&lower) {
		surfaces.push(AuthorityBoundarySurface::Tests);

		return surfaces;
	}
	if architecture_recovery_path_is_config(&lower) {
		surfaces.push(AuthorityBoundarySurface::Config);

		return surfaces;
	}
	if !architecture_recovery_path_is_owned(&lower) {
		return surfaces;
	}
	if architecture_recovery_path_is_public_api(&lower) {
		surfaces.push(AuthorityBoundarySurface::PublicApi);
	}
	if architecture_recovery_path_is_security(&lower) {
		surfaces.push(AuthorityBoundarySurface::Security);
	}
	if architecture_recovery_path_is_privacy(&lower) {
		surfaces.push(AuthorityBoundarySurface::Privacy);
	}
	if architecture_recovery_path_is_data(&lower) {
		surfaces.push(AuthorityBoundarySurface::Data);
	}
	if architecture_recovery_path_is_billing(&lower) {
		surfaces.push(AuthorityBoundarySurface::Billing);
	}
	if architecture_recovery_path_is_validation(&lower) {
		surfaces.push(AuthorityBoundarySurface::Validation);
	}
	if architecture_recovery_path_is_review_policy(&lower) {
		surfaces.push(AuthorityBoundarySurface::ReviewPolicy);
	}
	if surfaces.is_empty() && architecture_recovery_path_is_runtime(&lower) {
		surfaces.push(AuthorityBoundarySurface::Runtime);
	}

	surfaces
}

pub(super) fn architecture_recovery_surface_summary(
	surface: AuthorityBoundarySurface,
) -> &'static str {
	match surface {
		AuthorityBoundarySurface::ImplementationStrategy =>
			"Replace the non-converging guardrail repair strategy with a materially different architecture recovery strategy.",
		AuthorityBoundarySurface::Runtime =>
			"Runtime implementation files changed during recovery.",
		AuthorityBoundarySurface::Tests => "Test files changed during recovery.",
		AuthorityBoundarySurface::PublicApi =>
			"Public API or command surface files changed during recovery.",
		AuthorityBoundarySurface::Config => "Configuration files changed during recovery.",
		AuthorityBoundarySurface::Security =>
			"Security-sensitive implementation files changed during recovery.",
		AuthorityBoundarySurface::Data =>
			"Data or state persistence files changed during recovery.",
		AuthorityBoundarySurface::Billing => "Billing or usage files changed during recovery.",
		AuthorityBoundarySurface::Privacy => "Privacy-sensitive files changed during recovery.",
		AuthorityBoundarySurface::Validation =>
			"Validation or repository-gate files changed during recovery.",
		AuthorityBoundarySurface::ReviewPolicy =>
			"Review policy or landing policy files changed during recovery.",
		AuthorityBoundarySurface::Objective =>
			"Objective-changing recovery requires an explicit human decision.",
		AuthorityBoundarySurface::NonGoal =>
			"Non-goal-changing recovery requires an explicit human decision.",
		AuthorityBoundarySurface::ExternalDependency =>
			"External dependency recovery requires accepted authority.",
		AuthorityBoundarySurface::RetainedOwnership =>
			"Retained ownership evidence changed during recovery.",
		AuthorityBoundarySurface::AuthorityEvidence =>
			"Authority evidence changed or is insufficient during recovery.",
	}
}

fn architecture_recovery_path_is_test(path: &str) -> bool {
	path.starts_with("tests/")
		|| path.contains("/tests/")
		|| path.ends_with("_test.rs")
		|| path.ends_with("tests.rs")
		|| path.contains("/test_")
}

fn architecture_recovery_path_is_owned(path: &str) -> bool {
	path.starts_with("apps/")
		|| path.starts_with("plugins/")
		|| path.starts_with("automations/")
		|| path.starts_with("scripts/")
		|| path.starts_with("dev/")
		|| path.starts_with("site/")
		|| path.starts_with(".github/")
		|| matches!(
			path,
			"cargo.toml"
				| "cargo.lock"
				| "makefile.toml"
				| "clippy.toml"
				| "rust-toolchain.toml"
				| "decodex.example.toml"
		)
}

fn architecture_recovery_path_is_config(path: &str) -> bool {
	path == "cargo.toml"
		|| path == "cargo.lock"
		|| path == "makefile.toml"
		|| path == "clippy.toml"
		|| path == "rust-toolchain.toml"
		|| path == "decodex.example.toml"
		|| path.starts_with(".github/")
		|| path.ends_with(".toml")
		|| path.ends_with(".yaml")
		|| path.ends_with(".yml")
		|| path.ends_with(".json")
		|| path.ends_with(".env")
		|| architecture_recovery_path_is_dotfile(path)
}

fn architecture_recovery_path_is_dotfile(path: &str) -> bool {
	path.rsplit('/').next().is_some_and(|name| name.starts_with('.') && name != ".gitkeep")
}

fn architecture_recovery_path_is_public_api(path: &str) -> bool {
	architecture_recovery_path_has_segment(path, "cli")
		|| architecture_recovery_path_has_segment(path, "mcp")
		|| architecture_recovery_path_has_segment(path, "protocol")
		|| architecture_recovery_path_has_segment(path, "api")
		|| path.contains("tracker_tool_bridge")
		|| path.contains("app_bridge")
}

fn architecture_recovery_path_is_security(path: &str) -> bool {
	path.contains("auth")
		|| path.contains("credential")
		|| path.contains("secret")
		|| path.contains("security")
		|| path.contains("signing")
		|| path.contains("token")
}

fn architecture_recovery_path_is_privacy(path: &str) -> bool {
	path.contains("privacy") || path.contains("public_text") || path.contains("redact")
}

fn architecture_recovery_path_is_data(path: &str) -> bool {
	path.contains("database")
		|| path.contains("migration")
		|| path.contains("payload")
		|| path.contains("record")
		|| path.contains("sqlite")
		|| path.contains("state")
}

fn architecture_recovery_path_is_billing(path: &str) -> bool {
	path.contains("account")
		|| path.contains("billing")
		|| path.contains("credit")
		|| path.contains("invoice")
		|| path.contains("usage")
}

fn architecture_recovery_path_is_validation(path: &str) -> bool {
	path.contains("repo_gate")
		|| path.contains("validation")
		|| path.contains("validator")
		|| path.contains("verify")
}

fn architecture_recovery_path_is_review_policy(path: &str) -> bool {
	path.contains("review_policy") || path.contains("review_landing") || path.contains("landing")
}

fn architecture_recovery_path_is_runtime(path: &str) -> bool {
	architecture_recovery_path_is_owned(path)
}

fn architecture_recovery_path_has_segment(path: &str, segment: &str) -> bool {
	path.split('/')
		.any(|part| part == segment || part.strip_suffix(".rs").is_some_and(|stem| stem == segment))
}

#[cfg(test)]
mod tests {
	use crate::orchestrator::execution_architecture_recovery::{
		AuthorityBoundarySurface, surface::path_classification,
	};

	#[test]
	fn explanatory_markdown_does_not_create_authority_surfaces() {
		assert!(
			path_classification::architecture_recovery_surfaces_for_path(
				"notes/operations/commands-and-validation.md"
			)
			.is_empty()
		);
	}

	#[test]
	fn root_tests_remain_authority_surfaces() {
		assert_eq!(
			path_classification::architecture_recovery_surfaces_for_path(
				"tests/scripts/test_vnext_architecture.py"
			),
			vec![AuthorityBoundarySurface::Tests]
		);
	}

	#[test]
	fn repository_config_remains_an_authority_surface() {
		for path in [
			".dockerignore",
			".editorconfig",
			".gitignore",
			".prettierignore",
			".prettierrc",
			".taplo.toml",
			".rustfmt.toml",
			"assets/decodex/icon.json",
			"site/.prettierrc",
		] {
			assert_eq!(
				path_classification::architecture_recovery_surfaces_for_path(path),
				vec![AuthorityBoundarySurface::Config],
				"unexpected authority classification for {path}"
			);
		}
	}

	#[test]
	fn owned_validation_source_still_blocks_landing() {
		assert_eq!(
			path_classification::architecture_recovery_surfaces_for_path(
				"apps/decodex/src/repo_gate/validation.rs"
			),
			vec![AuthorityBoundarySurface::Validation]
		);
	}
}
