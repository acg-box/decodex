use std::sync::{Mutex, MutexGuard, OnceLock};

pub(crate) struct TestEnvLockGuard {
	_lock: MutexGuard<'static, ()>,
}

pub(crate) fn lock_test_env() -> TestEnvLockGuard {
	TestEnvLockGuard {
		_lock: test_env_mutex().lock().expect("test env mutex should not be poisoned"),
	}
}

pub(crate) fn private_tempdir() -> crate::private_fs::PrivateTestDirectory {
	crate::private_fs::create_private_test_directory(&std::env::temp_dir())
		.expect("private temporary directory should be created")
}

fn test_env_mutex() -> &'static Mutex<()> {
	static TEST_ENV_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();

	TEST_ENV_MUTEX.get_or_init(|| Mutex::new(()))
}

#[cfg(test)]
mod tests {
	use std::{
		fs,
		os::unix::fs::{MetadataExt as _, PermissionsExt as _},
	};

	use super::private_tempdir;

	fn private_fixture_directory(path: &std::path::Path) {
		fs::create_dir(path).expect("fixture directory should be created");
		fs::set_permissions(path, fs::Permissions::from_mode(0o700))
			.expect("fixture directory should be private");
	}

	#[test]
	fn private_tempdir_is_private_and_removes_unsafe_test_entries() {
		let temporary = private_tempdir();
		let path = temporary.path().to_path_buf();
		let file = path.join("ordinary");
		let link = path.join("link");

		fs::write(&file, b"fixture").expect("fixture file should be written");
		std::os::unix::fs::symlink(&file, &link).expect("fixture link should be created");
		assert_eq!(fs::metadata(&path).expect("temporary root metadata").mode() & 0o777, 0o700);

		drop(temporary);
		assert!(!path.exists());
	}

	#[test]
	fn private_tempdir_accepts_standard_public_tmp_parent() {
		if std::env::var_os("DECODEX_CANDIDATE_SANDBOX").as_deref()
			== Some(std::ffi::OsStr::new("1"))
		{
			return;
		}

		let temporary =
			crate::private_fs::create_private_test_directory(std::path::Path::new("/tmp"))
				.expect("standard public temporary parent should be accepted");
		let path = temporary.path().to_path_buf();

		assert_eq!(fs::metadata(&path).expect("temporary root metadata").mode() & 0o777, 0o700);
		drop(temporary);
		assert!(!path.exists());
	}

	#[test]
	fn private_tempdir_rejects_non_private_parent_and_resolves_private_symlink() {
		let fixture = private_tempdir();
		let open_parent = fixture.path().join("open-parent");
		let private_parent = fixture.path().join("private-parent");
		let linked_parent = fixture.path().join("linked-parent");

		fs::create_dir(&open_parent).expect("open parent should be created");
		fs::set_permissions(&open_parent, fs::Permissions::from_mode(0o755))
			.expect("open parent mode should be set");
		private_fixture_directory(&private_parent);
		std::os::unix::fs::symlink(&private_parent, &linked_parent)
			.expect("parent symlink should be created");

		assert!(crate::private_fs::create_private_test_directory(&open_parent).is_err());
		let linked = crate::private_fs::create_private_test_directory(&linked_parent)
			.expect("a canonicalized private parent symlink should be accepted");
		let path = linked.path().to_path_buf();

		drop(linked);
		assert!(!path.exists());
	}

	#[test]
	fn private_tempdir_detects_parent_replacement_without_writing_to_replacement() {
		let fixture = private_tempdir();
		let parent = fixture.path().join("parent");
		let displaced = fixture.path().join("displaced");

		private_fixture_directory(&parent);
		let replacement = parent.clone();
		let error = crate::private_fs::create_private_test_directory_with(&parent, || {
			fs::rename(&replacement, &displaced).expect("parent should be displaced");
			private_fixture_directory(&replacement);
		})
		.expect_err("parent replacement must fail closed");

		assert!(
			error.to_string().contains("parent identity changed"),
			"unexpected replacement error: {error:?}"
		);
		assert_eq!(fs::read_dir(&parent).expect("replacement should be readable").count(), 0);
		assert_eq!(fs::read_dir(&displaced).expect("displaced root should be readable").count(), 0);
	}

	#[test]
	fn private_tempdir_cleanup_does_not_remove_a_replacement_directory() {
		let fixture = private_tempdir();
		let temporary = crate::private_fs::create_private_test_directory(fixture.path())
			.expect("nested temporary directory should be created");
		let path = temporary.path().to_path_buf();
		let displaced = fixture.path().join("displaced-cleanup-root");
		let marker = path.join("replacement-marker");
		let error = temporary
			.remove_with_before_unlink(|| {
				fs::rename(&path, &displaced).expect("test directory should be displaced");
				private_fixture_directory(&path);
				fs::write(&marker, b"replacement").expect("replacement marker should be written");
			})
			.expect_err("cleanup must reject a replacement binding");

		assert!(error.to_string().contains("identity changed"));
		assert_eq!(fs::read(&marker).expect("replacement marker should remain"), b"replacement");

		fs::remove_file(&marker).expect("replacement marker should be removed");
		fs::remove_dir(&path).expect("replacement directory should be removed");
		fs::rename(&displaced, &path).expect("original directory binding should be restored");
		drop(temporary);
		assert!(!path.exists());
	}
}
