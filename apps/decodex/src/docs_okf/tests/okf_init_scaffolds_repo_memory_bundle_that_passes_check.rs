use tempfile::TempDir;

use crate::docs_okf::{self, OkfCheckProfile};

#[test]
fn okf_init_scaffolds_repo_memory_bundle_that_passes_check() {
	let temp_dir = TempDir::new().expect("tempdir");
	let bundle = temp_dir.path().join("knowledge");
	let init_report =
		docs_okf::init_okf_bundle(&bundle, OkfCheckProfile::RepoMemory).expect("init");
	let check_report =
		docs_okf::run_okf_check(&bundle, OkfCheckProfile::RepoMemory).expect("check");
	let graph = docs_okf::build_okf_graph(&bundle).expect("graph initialized bundle");

	assert_eq!(init_report.profile(), OkfCheckProfile::RepoMemory);
	assert_eq!(init_report.created.len(), 3);
	assert!(init_report.unchanged.is_empty());
	assert!(!check_report.has_issues(), "{check_report:#?}");
	assert!(graph.concepts.iter().any(|concept| concept.id == "overview"));
}
