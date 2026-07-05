use std::path::Path;

use clap::Parser;

use crate::cli::{AppCommand, Cli, Command};

#[test]
fn parses_app_bundle_and_new_instance() {
	let cli =
		Cli::parse_from(["decodex", "app", "--bundle", "target/decodex-app/Decodex.app", "--new"]);

	assert!(matches!(
		cli.command,
		Command::App(AppCommand {
			bundle: Some(bundle),
			new: true,
		}) if bundle == Path::new("target/decodex-app/Decodex.app")
	));
}
