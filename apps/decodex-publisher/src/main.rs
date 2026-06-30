//! Decodex Publisher auxiliary tool binary entrypoint.

#![allow(unused_crate_dependencies)]

use color_eyre::Result;

fn main() -> Result<()> {
	decodex_publisher::run()
}
