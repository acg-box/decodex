use std::path::Path;

use clap::Parser;

use crate::cli::{
	Cli, Command,
	docs_okf_commands::{
		DocsCommand, DocsGraphCommand, DocsSubcommand, OkfCommand, OkfFindCommand, OkfFindFilters,
		OkfSubcommand,
	},
};

#[test]
fn parses_okf_and_docs_find_graph_commands() {
	let okf_cli = Cli::parse_from([
		"decodex",
		"okf",
		"find",
		"docs",
		"--tag",
		"okf",
		"--text",
		"command design",
	]);
	let Command::Okf(OkfCommand {
		command:
			OkfSubcommand::Find(OkfFindCommand {
				root,
				filters: OkfFindFilters { tag, text: Some(text), .. },
			}),
	}) = okf_cli.command
	else {
		panic!("expected okf find command");
	};

	assert_eq!(root, Path::new("docs"));
	assert_eq!(tag, vec![String::from("okf")]);
	assert_eq!(text, "command design");

	let docs_cli = Cli::parse_from(["decodex", "docs", "graph", "--json"]);

	assert!(matches!(
		docs_cli.command,
		Command::Docs(DocsCommand {
			root,
			command: DocsSubcommand::Graph(DocsGraphCommand {
				json: true,
			}),
		}) if root == Path::new("docs")
	));
}
