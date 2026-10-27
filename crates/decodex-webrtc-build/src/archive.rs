#[cfg(unix)] use std::os::unix::fs::PermissionsExt as _;
use std::{
	collections::HashSet,
	fs::{self, File, OpenOptions},
	io::{self, BufRead as _, BufReader, Read, Seek as _},
	path::{Component, Path, PathBuf},
};

use anyhow::Context;
use fs2::FileExt;
use sha2::{Digest, Sha256};
use tempfile::{Builder, NamedTempFile, TempDir};
use walkdir::WalkDir;
use zip::{ZipArchive, read::ExtractOptions};

struct Entry {
	index: usize,
	path: PathBuf,
	size: u64,
}

struct VerifiedArchive {
	zip: ZipArchive<File>,
	files: Vec<Entry>,
}
impl VerifiedArchive {
	fn open(path: &Path, expected: &str, triple: &str) -> anyhow::Result<Self> {
		let mut file = File::open(path).context("cannot open WebRTC archive")?;

		anyhow::ensure!(
			digest_file(&mut file)? == expected,
			"WebRTC archive does not match its pinned SHA-256"
		);

		let mut zip = ZipArchive::new(file).context("cannot read WebRTC ZIP directory")?;

		zip.validate_header_consistency().context("WebRTC ZIP headers are inconsistent")?;

		let mut files = Vec::new();

		for index in 0..zip.len() {
			let entry = zip.by_index(index).context("cannot read WebRTC ZIP entry")?;

			anyhow::ensure!(!entry.is_symlink(), "WebRTC ZIP contains a symbolic link");

			let name = entry.enclosed_name().context("WebRTC ZIP contains an unsafe path")?;
			let path = name.strip_prefix(triple).context("WebRTC ZIP has an unexpected root")?;

			anyhow::ensure!(
				path.components()
					.all(|part| matches!(part, Component::Normal(_) | Component::CurDir)),
				"WebRTC ZIP contains an unsafe path"
			);

			if entry.is_file() {
				anyhow::ensure!(
					!path.as_os_str().is_empty(),
					"WebRTC ZIP contains an invalid file path"
				);

				files.push(Entry { index, path: path.to_owned(), size: entry.size() });
			}
		}

		anyhow::ensure!(!files.is_empty(), "WebRTC ZIP contains no files");

		Ok(Self { zip, files })
	}

	fn matches(&mut self, directory: &Path) -> anyhow::Result<bool> {
		let Ok(root) = fs::symlink_metadata(directory) else {
			return Ok(false);
		};

		if !root.is_dir() || root.file_type().is_symlink() {
			return Ok(false);
		}

		let mut actual = HashSet::new();

		for item in WalkDir::new(directory).follow_links(false) {
			let Ok(item) = item else {
				return Ok(false);
			};

			if item.file_type().is_symlink() {
				return Ok(false);
			}
			if item.file_type().is_file() {
				actual.insert(item.path().strip_prefix(directory)?.to_owned());
			} else if !item.file_type().is_dir() {
				return Ok(false);
			}
		}

		if actual.len() != self.files.len() {
			return Ok(false);
		}

		for item in &self.files {
			if !actual.contains(&item.path) {
				return Ok(false);
			}

			let Ok(file) = File::open(directory.join(&item.path)) else {
				return Ok(false);
			};

			if file.metadata()?.len() != item.size {
				return Ok(false);
			}

			let entry = self.zip.by_index(item.index)?;

			if !equal_bytes(entry, file)? {
				return Ok(false);
			}
		}

		Ok(true)
	}
}

pub(crate) fn prepare(
	scratch: &Path,
	directory: &Path,
	triple: &str,
	expected: &str,
	download: impl FnOnce(&mut File) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
	fs::create_dir_all(scratch).context("cannot create the LiveKit scratch directory")?;

	let lock = OpenOptions::new()
		.create(true)
		.truncate(false)
		.write(true)
		.open(scratch.join(".lock"))
		.context("cannot open the WebRTC cache lock")?;

	FileExt::lock_exclusive(&lock).context("cannot lock the WebRTC cache")?;

	let archives = scratch.join("verified-archives");

	fs::create_dir_all(&archives).context("cannot create the WebRTC archive cache")?;

	let cached = archives.join(format!("{triple}-{expected}.zip"));
	let pinned = cached.is_file() && digest(&cached)? == expected;

	if !pinned {
		let mut temporary =
			NamedTempFile::new_in(&archives).context("cannot stage WebRTC download")?;

		download(temporary.as_file_mut())?;

		VerifiedArchive::open(temporary.path(), expected, triple)?;

		temporary.persist(&cached).context("cannot save the verified WebRTC archive")?;
	}

	let mut archive = VerifiedArchive::open(&cached, expected, triple)?;

	if archive.matches(directory)? {
		ensure_owner_access(directory)?;

		return Ok(());
	}

	let parent = directory.parent().context("WebRTC cache has no parent")?;

	fs::create_dir_all(parent).context("cannot create the WebRTC cache parent")?;

	let stage = Builder::new().prefix(".verified-webrtc-").tempdir_in(parent)?;

	archive
		.zip
		.extract_with_options(
			stage.path(),
			ExtractOptions::default().enforce_header_consistency(true),
		)
		.context("cannot extract verified WebRTC archive")?;

	let extracted = stage.path().join(triple);

	ensure_owner_access(&extracted)?;

	anyhow::ensure!(
		archive.matches(&extracted)?,
		"extracted WebRTC files do not match the verified archive"
	);

	replace(directory, &extracted, stage)
}

fn digest_file(file: &mut File) -> anyhow::Result<String> {
	file.rewind().context("cannot seek in WebRTC archive")?;

	let mut hash = Sha256::new();
	let mut buffer = [0_u8; 128 * 1_024];

	loop {
		let count = file.read(&mut buffer).context("cannot read WebRTC archive")?;

		if count == 0 {
			break;
		}

		hash.update(&buffer[..count]);
	}

	file.rewind().context("cannot seek in WebRTC archive")?;

	Ok(hash.finalize().iter().map(|byte| format!("{byte:02x}")).collect())
}

fn digest(path: &Path) -> anyhow::Result<String> {
	digest_file(&mut File::open(path).context("cannot open WebRTC archive")?)
}

fn equal_bytes(left: impl Read, right: impl Read) -> io::Result<bool> {
	let mut left = BufReader::new(left);
	let mut right = BufReader::new(right);

	loop {
		let a = left.fill_buf()?;
		let b = right.fill_buf()?;

		if a.is_empty() || b.is_empty() {
			return Ok(a.is_empty() && b.is_empty());
		}

		let length = a.len().min(b.len());

		if a[..length] != b[..length] {
			return Ok(false);
		}

		left.consume(length);
		right.consume(length);
	}
}

#[cfg(unix)]
fn ensure_owner_access(directory: &Path) -> anyhow::Result<()> {
	for item in WalkDir::new(directory).follow_links(false) {
		let item = item?;
		let metadata = fs::symlink_metadata(item.path())?;

		if metadata.file_type().is_symlink() {
			continue;
		}

		let mut permissions = metadata.permissions();
		let original = permissions.mode();
		let required = if metadata.is_dir() { 0o700 } else { 0o600 };

		if original & required != required {
			permissions.set_mode(original | required);

			// Match LiveKit's best-effort normalization for read-only mounts.
			let _ = fs::set_permissions(item.path(), permissions);
		}
	}

	Ok(())
}

#[cfg(not(unix))]
fn ensure_owner_access(_directory: &Path) -> anyhow::Result<()> {
	Ok(())
}

fn replace(directory: &Path, staged: &Path, stage: TempDir) -> anyhow::Result<()> {
	let backup = stage.path().join("previous");
	let had_previous = directory.symlink_metadata().is_ok();

	if had_previous {
		fs::rename(directory, &backup).context("cannot move the old WebRTC cache")?;
	}

	if let Err(error) = fs::rename(staged, directory) {
		if had_previous && let Err(restore) = fs::rename(&backup, directory) {
			let saved = stage.keep();

			anyhow::bail!(
				"cannot publish the WebRTC cache: {error}; cannot restore the old cache: {restore}; it remains at {}",
				saved.display()
			);
		}

		return Err(error).context("cannot publish the WebRTC cache");
	}

	Ok(())
}

#[cfg(test)]
mod tests {
	#[cfg(unix)] use std::os::unix::fs::PermissionsExt as _;
	use std::{
		cell::Cell,
		env, fs,
		io::{Cursor, Write as _},
		path::Path,
	};

	use anyhow::Result;
	use sha2::{Digest, Sha256};
	use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

	use crate::archive::{self, VerifiedArchive};

	const TRIPLE: &str = "mac-arm64-release";

	fn fixture() -> Result<Vec<u8>> {
		let mut zip = ZipWriter::new(Cursor::new(Vec::new()));

		zip.start_file(
			format!("{TRIPLE}/include/example.h"),
			SimpleFileOptions::default().unix_permissions(0o444),
		)?;
		zip.write_all(b"verified header")?;
		zip.start_file(format!("{TRIPLE}/lib/libwebrtc.a"), SimpleFileOptions::default())?;
		zip.write_all(b"verified library")?;

		Ok(zip.finish()?.into_inner())
	}

	fn sha256(bytes: &[u8]) -> String {
		Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
	}

	fn install(scratch: &Path, directory: &Path, bytes: &[u8], expected: &str) -> Result<()> {
		archive::prepare(scratch, directory, TRIPLE, expected, |file| {
			file.write_all(bytes)?;

			Ok(())
		})
	}

	#[test]
	fn wrong_digest_fails_before_extraction_and_preserves_the_cache() -> Result<()> {
		let temporary = tempfile::tempdir()?;
		let directory = temporary.path().join("build");

		fs::create_dir(&directory)?;
		fs::write(directory.join("sentinel"), b"untouched")?;

		let Err(error) =
			install(&temporary.path().join("scratch"), &directory, &fixture()?, &"0".repeat(64))
		else {
			anyhow::bail!("an incorrect digest must be rejected");
		};

		assert!(format!("{error:#}").contains("pinned SHA-256"));
		assert_eq!(fs::read(directory.join("sentinel"))?, b"untouched");
		assert_eq!(fs::read_dir(directory)?.count(), 1);

		Ok(())
	}

	#[test]
	fn mismatched_headers_fail_before_extraction_even_with_the_expected_digest() -> Result<()> {
		let mut bytes = fixture()?;

		// ZIP local-file names start at byte 30. Keep the central-directory name unchanged.
		bytes[30] = b'n';

		let temporary = tempfile::tempdir()?;
		let legacy = temporary.path().join("legacy");

		ZipArchive::new(Cursor::new(&bytes))?.extract(&legacy)?;

		assert!(legacy.join(TRIPLE).join("include/example.h").is_file());

		let directory = temporary.path().join("build");

		fs::create_dir(&directory)?;
		fs::write(directory.join("sentinel"), b"untouched")?;

		let Err(error) =
			install(&temporary.path().join("scratch"), &directory, &bytes, &sha256(&bytes))
		else {
			anyhow::bail!("inconsistent headers must be rejected");
		};

		assert!(format!("{error:#}").contains("headers are inconsistent"));
		assert_eq!(fs::read(directory.join("sentinel"))?, b"untouched");
		assert_eq!(fs::read_dir(directory)?.count(), 1);

		Ok(())
	}

	#[test]
	fn valid_archive_repairs_unverified_or_modified_extracted_files_offline() -> Result<()> {
		let temporary = tempfile::tempdir()?;
		let scratch = temporary.path().join("scratch");
		let directory = temporary.path().join("build");
		let bytes = fixture()?;
		let expected = sha256(&bytes);

		fs::create_dir(&directory)?;
		fs::write(directory.join("old-file"), b"unverified")?;

		install(&scratch, &directory, &bytes, &expected)?;

		assert!(!directory.join("old-file").exists());
		assert_eq!(fs::read(directory.join("include/example.h"))?, b"verified header");

		#[cfg(unix)]
		{
			let header = directory.join("include/example.h");

			assert_eq!(fs::metadata(&header)?.permissions().mode() & 0o600, 0o600);

			fs::set_permissions(&header, fs::Permissions::from_mode(0o400))?;
			archive::prepare(&scratch, &directory, TRIPLE, &expected, |_| {
				anyhow::bail!("unexpected download")
			})?;

			assert_eq!(fs::metadata(header)?.permissions().mode() & 0o600, 0o600);
		}

		fs::write(directory.join("include/example.h"), b"changed")?;
		fs::write(directory.join("extra"), b"unexpected")?;
		archive::prepare(&scratch, &directory, TRIPLE, &expected, |_| {
			anyhow::bail!("unexpected download")
		})?;

		assert!(!directory.join("extra").exists());
		assert_eq!(fs::read(directory.join("include/example.h"))?, b"verified header");

		archive::prepare(&scratch, &directory, TRIPLE, &expected, |_| {
			anyhow::bail!("unexpected download")
		})?;

		Ok(())
	}

	#[test]
	fn corrupt_archive_cache_is_downloaded_and_verified_again() -> Result<()> {
		let temporary = tempfile::tempdir()?;
		let scratch = temporary.path().join("scratch");
		let directory = temporary.path().join("build");
		let bytes = fixture()?;
		let expected = sha256(&bytes);

		install(&scratch, &directory, &bytes, &expected)?;

		let archive = scratch.join("verified-archives").join(format!("{TRIPLE}-{expected}.zip"));

		fs::write(archive, b"damaged")?;

		let downloads = Cell::new(0);

		archive::prepare(&scratch, &directory, TRIPLE, &expected, |file| {
			downloads.set(downloads.get() + 1);
			file.write_all(&bytes)?;

			Ok(())
		})?;

		assert_eq!(downloads.get(), 1);
		assert_eq!(fs::read(directory.join("lib/libwebrtc.a"))?, b"verified library");

		Ok(())
	}

	#[test]
	#[ignore = "requires both official WebRTC release archives"]
	fn official_macos_archives_pass_digest_and_strict_header_validation() -> Result<()> {
		for (triple, variable) in [
			("mac-arm64-release", "DECODEX_WEBRTC_ARM64_ARCHIVE"),
			("mac-x64-release", "DECODEX_WEBRTC_X64_ARCHIVE"),
		] {
			let path =
				env::var_os(variable).ok_or_else(|| anyhow::anyhow!("{variable} is required"))?;
			let archive = VerifiedArchive::open(
				Path::new(&path),
				super::super::pinned_digest(triple)?,
				triple,
			)?;

			assert!(archive.files.len() > 38_000);
			assert!(archive.files.iter().any(|item| item.path == Path::new("lib/libwebrtc.a")));
			println!("{triple}: {} verified file entries", archive.files.len());
		}

		Ok(())
	}
}
