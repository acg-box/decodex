//! Rebuild the adapter when an embedded migration changes.

fn main() {
	println!("cargo:rerun-if-changed=migrations");
}
