//! Verify the WebRTC archive used by the LiveKit build script.

mod archive;
mod download;

pub use webrtc_upstream::{
	IGNORE_DEFINES, SCRATH_PATH, WEBRTC_TAG, android_ndk_toolchain, custom_dir, download_url,
	prebuilt_dir, target_arch, target_os, use_debug, webrtc_triple,
};

use std::{path::PathBuf, sync::OnceLock};

use anyhow::{self, Context, Result};

const RELEASE: &str = "webrtc-a545de4";

static PREPARED: OnceLock<Result<PathBuf, String>> = OnceLock::new();

/// Return a verified official build or an explicitly supplied custom directory.
///
/// Custom directories are developer-provided inputs and are not official ZIPs.
#[must_use]
pub fn webrtc_dir() -> PathBuf {
	prepared().unwrap_or_else(|error| panic!("WebRTC build preparation failed: {error:#}"))
}

/// Prepare the official archive with the same checks used by [`webrtc_dir`].
///
/// # Errors
///
/// Returns an error if the archive, headers, cache, or custom directory is invalid.
pub fn download_webrtc() -> Result<()> {
	prepared().map(|_| ())
}

/// Read LiveKit's preprocessor definitions after preparing the build directory.
#[must_use]
pub fn webrtc_defines() -> Vec<(String, Option<String>)> {
	let _ = webrtc_dir();

	webrtc_upstream::webrtc_defines()
}

/// Configure LiveKit's JNI symbols after preparing the build directory.
///
/// # Errors
///
/// Returns an error if preparation or LiveKit's JNI configuration fails.
pub fn configure_jni_symbols() -> Result<()> {
	prepared()?;

	webrtc_upstream::configure_jni_symbols()
}

fn pinned_digest(triple: &str) -> Result<&'static str> {
	anyhow::ensure!(WEBRTC_TAG == RELEASE, "the LiveKit release changed without new archive pins");

	match triple {
		"mac-arm64-release" =>
			Ok("153a1f1a268b812b4a501deda6505037f29481691efec52be817f560ff09e206"),
		"mac-x64-release" => Ok("c53d7d20277f7729956854f1268c372360f08732520136b559034eedae0980a2"),
		_ => Err(anyhow::anyhow!("no verified WebRTC archive is pinned for {triple}")),
	}
}

fn existing_custom_dir(path: PathBuf) -> Result<PathBuf> {
	anyhow::ensure!(path.is_dir(), "LK_CUSTOM_WEBRTC must point to an existing build directory");

	Ok(path)
}

fn prepare() -> Result<PathBuf> {
	if let Some(path) = custom_dir() {
		return existing_custom_dir(path);
	}

	let triple = webrtc_triple();
	let digest = pinned_digest(&triple)?;
	let destination = prebuilt_dir();
	let scratch = destination
		.ancestors()
		.nth(3)
		.context("the LiveKit WebRTC cache has an unexpected path")?;

	archive::prepare(scratch, &destination, &triple, digest, |output| {
		download::archive(&download_url(), output)
	})?;

	Ok(destination)
}

fn prepared() -> Result<PathBuf> {
	PREPARED
		.get_or_init(|| prepare().map_err(|error| format!("{error:#}")))
		.clone()
		.map_err(|error| anyhow::anyhow!(error))
}

#[cfg(test)]
mod tests {

	#[test]
	fn supported_targets_have_distinct_pins() -> anyhow::Result<()> {
		assert_ne!(
			crate::pinned_digest("mac-arm64-release")?,
			crate::pinned_digest("mac-x64-release")?
		);
		assert!(crate::pinned_digest("mac-arm64-debug").is_err());
		assert!(crate::pinned_digest("linux-x64-release").is_err());

		Ok(())
	}

	#[test]
	fn custom_build_must_exist() -> anyhow::Result<()> {
		let directory = tempfile::tempdir()?;

		assert_eq!(crate::existing_custom_dir(directory.path().to_owned())?, directory.path());
		assert!(crate::existing_custom_dir(directory.path().join("missing")).is_err());

		Ok(())
	}
}
