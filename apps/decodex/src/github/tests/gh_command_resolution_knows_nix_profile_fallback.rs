#[test]
fn gh_command_resolution_knows_nix_profile_fallback() {
	assert!(crate::github::GH_FALLBACK_PATHS.contains(&"/run/current-system/sw/bin/gh"));
}
