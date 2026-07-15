//! Default-disabled Decodex vNext GPUI client composition root.

#[allow(
	dead_code,
	reason = "XY-1334 will classify and remove or replace remaining future-shell/test-only cache API allowances when the shell constructs the lifecycle"
)]
mod client_cache;
#[allow(dead_code, reason = "XY-1334 will connect the accepted lifecycle to the GPUI shell")]
mod client_lifecycle;

use decodex_protocol::CURRENT_VERSION;

fn main() {
	let version = CURRENT_VERSION;

	println!(
		"Decodex GPUI v{}.{} is disabled: client lifecycle composed; S shell slice remains pending",
		version.major, version.minor
	);
}
