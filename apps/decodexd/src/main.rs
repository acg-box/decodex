//! Decodex vNext service composition root.

use decodex_runtime::ServiceComposition;

fn main() {
	let announcement = ServiceComposition::foundation().boot();

	println!(
		"decodexd v{}.{} foundation wired; service unavailable and no endpoint selected",
		announcement.version.major, announcement.version.minor
	);
}
