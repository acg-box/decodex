use std::fs;

use tempfile::TempDir;

use crate::docs_okf::{self, tests};

#[test]
fn okf_graph_skips_links_outside_the_bundle() {
	let temp_dir = TempDir::new().expect("tempdir");
	let bundle = temp_dir.path().join("bundle");

	fs::create_dir_all(&bundle).expect("bundle");
	tests::write(&temp_dir.path().join("README.md"), "# External repo doc\n");
	tests::write(&bundle.join("index.md"), "# Bundle\n");
	tests::write(
		&bundle.join("alpha.md"),
		"---\ntype: Concept\ntitle: Alpha\ndescription: Alpha concept.\n---\n\nSee [Beta](beta.md) and [repo readme](../README.md).\n",
	);
	tests::write(
		&bundle.join("beta.md"),
		"---\ntype: Concept\ntitle: Beta\ndescription: Beta concept.\n---\n\nBeta.\n",
	);

	let graph = docs_okf::build_okf_graph(&bundle).expect("graph");

	assert_eq!(graph.broken_links, Vec::new());
	assert_eq!(graph.edges.len(), 1);
	assert_eq!(graph.edges[0].target, "beta");
}
