//! Preserve the signed bundle context of a CLI image during static attestation.
use super::*;

/// Return the image destination after copying its bundle context, if it has one.
pub(super) fn path(source: &Path, destination: &Path) -> Result<PathBuf, SupervisionError> {
	let Some(macos) = source.parent().filter(|p| p.file_name() == Some(OsStr::new("MacOS"))) else {
		return Ok(destination.join("verified-codex-image"));
	};
	let Some(contents) = macos.parent().filter(|p| p.file_name() == Some(OsStr::new("Contents")))
	else {
		return Ok(destination.join("verified-codex-image"));
	};
	let Some(bundle) = contents.parent().filter(|p| p.extension() == Some(OsStr::new("app")))
	else {
		return Ok(destination.join("verified-codex-image"));
	};
	let target = destination.join("Verified.app");
	let mut budget = Budget { bytes: 16 * 1024 * 1024, entries: 1024 };
	copy_context(bundle, bundle, &target, source, &mut budget, 0)
		.map_err(|_| SupervisionError::ExecutableUnavailable)?;
	let relative =
		source.strip_prefix(bundle).map_err(|_| SupervisionError::ExecutableUnavailable)?;
	Ok(target.join(relative))
}

struct Budget {
	bytes: u64,
	entries: usize,
}

fn copy_context(
	root: &Path,
	source: &Path,
	target: &Path,
	image: &Path,
	budget: &mut Budget,
	depth: usize,
) -> io::Result<()> {
	if depth > 32 || budget.entries == 0 {
		return Err(io::Error::other("bundle context limit"));
	}
	budget.entries -= 1;
	if source == image {
		return Ok(());
	}
	let metadata = source.symlink_metadata()?;
	if metadata.file_type().is_symlink() {
		let link = fs::read_link(source)?;
		if link.is_absolute() || !source.canonicalize()?.starts_with(root) {
			return Err(io::Error::other("bundle link escapes its owner"));
		}
		return std::os::unix::fs::symlink(link, target);
	}
	if metadata.is_dir() {
		fs::create_dir(target)?;
		for entry in fs::read_dir(source)? {
			let entry = entry?;
			copy_context(
				root,
				&entry.path(),
				&target.join(entry.file_name()),
				image,
				budget,
				depth + 1,
			)?;
		}
		return Ok(());
	}
	if !metadata.is_file() {
		return Err(io::Error::other("unsupported bundle entry"));
	}
	let mut input = File::open(source)?.take(budget.bytes + 1);
	let mut output = OpenOptions::new().create_new(true).write(true).mode(0o400).open(target)?;
	let copied = io::copy(&mut input, &mut output)?;
	budget.bytes = budget
		.bytes
		.checked_sub(copied)
		.ok_or_else(|| io::Error::other("bundle context byte limit"))?;
	output.sync_all()?;
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	#[ignore = "requires DECODEX_TEST_CODEX_BINARY; installed signed CLI bundle"]
	fn installed_cli_bundle_has_an_exact_valid_snapshot_identity() {
		let image =
			PathBuf::from(std::env::var_os("DECODEX_TEST_CODEX_BINARY").expect("installed binary"));
		let (snapshot, _) = capture_executable_snapshot(&image).expect("capture installed bundle");
		AttestedCodeIdentity::capture(&snapshot.execution_path(), &image)
			.expect("validate installed bundle identity");
	}

	#[test]
	fn signed_cli_bundle_snapshot_preserves_metadata_and_rejects_source_tampering() {
		let home = TempDir::new().unwrap();
		let bundle = home.path().join("Fixture.app");
		let contents = bundle.join("Contents");
		fs::create_dir_all(contents.join("MacOS")).unwrap();
		let image = contents.join("MacOS/codex");
		fs::copy("/bin/cat", &image).unwrap();
		fs::set_permissions(&image, Permissions::from_mode(0o700)).unwrap();
		let info = contents.join("Info.plist");
		fs::write(&info, r#"<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict><key>CFBundleExecutable</key><string>codex</string><key>CFBundleIdentifier</key><string>test.decodex.snapshot</string><key>CFBundlePackageType</key><string>APPL</string><key>CFBundleVersion</key><string>1</string></dict></plist>"#).unwrap();
		assert!(
			Command::new("/usr/bin/codesign")
				.args(["--force", "--sign", "-", "--timestamp=none"])
				.arg(&bundle)
				.output()
				.unwrap()
				.status
				.success()
		);
		let (snapshot, digest) = capture_executable_snapshot(&image).unwrap();
		assert_eq!(snapshot.digest().unwrap(), digest);
		AttestedCodeIdentity::capture(&snapshot.execution_path(), &image).unwrap();
		fs::write(&info, "modified metadata").unwrap();
		assert!(AttestedCodeIdentity::capture(&snapshot.execution_path(), &image).is_err());
		// The retained signed context is independent of later changes to the source bundle.
		AttestedCodeIdentity::capture(&snapshot.execution_path(), &snapshot.execution_path())
			.unwrap();
	}
	#[test]
	fn bundle_context_rejects_links_outside_the_bundle() {
		let home = TempDir::new().unwrap();
		let bundle = home.path().join("Fixture.app");
		let macos = bundle.join("Contents/MacOS");
		fs::create_dir_all(&macos).unwrap();
		let image = macos.join("codex");
		fs::copy("/bin/cat", &image).unwrap();
		std::os::unix::fs::symlink("/etc/hosts", bundle.join("escaped-resource")).unwrap();
		assert!(matches!(
			capture_executable_snapshot(&image),
			Err(SupervisionError::ExecutableUnavailable)
		));
	}
}
