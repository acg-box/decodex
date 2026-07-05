use std::fs;

use tempfile::TempDir;

use crate::docs_okf::{self, OkfQuery, tests};

#[test]
fn okf_query_matches_structured_frontmatter_refs() {
	let temp_dir = TempDir::new().expect("tempdir");
	let bundle = temp_dir.path().join("docs");

	fs::create_dir_all(&bundle).expect("bundle");
	tests::write(&temp_dir.path().join("src.rs"), "fn main() {}\n");
	tests::write(&bundle.join("index.md"), "# Bundle\n");
	tests::write(
		&bundle.join("alpha.md"),
		"---\ntype: Concept\ntitle: Alpha\ndescription: Alpha concept.\ntags: [runtime]\nsource_refs: [https://example.com/spec]\ncode_refs: [src.rs]\nrelated: [beta.md]\n---\n\nAlpha.\n",
	);
	tests::write(
		&bundle.join("beta.md"),
		"---\ntype: Concept\ntitle: Beta\ndescription: Beta concept.\n---\n\nBeta.\n",
	);

	let query = OkfQuery {
		code_ref: Some(String::from("src.rs")),
		tags: Vec::new(),
		..OkfQuery::default()
	};
	let matches = docs_okf::query_okf_bundle(&bundle, &query).expect("query");

	assert_eq!(matches.len(), 1);
	assert_eq!(matches[0].id, "alpha");
}
