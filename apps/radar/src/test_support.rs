use std::{
	env,
	sync::{Mutex, MutexGuard, OnceLock},
};

use crate::private_fs::{self, PrivateTestDirectory};

pub(crate) struct TestEnvLockGuard {
	_lock: MutexGuard<'static, ()>,
}

pub(crate) fn lock_test_env() -> TestEnvLockGuard {
	TestEnvLockGuard {
		_lock: test_env_mutex().lock().expect("test env mutex should not be poisoned"),
	}
}

pub(crate) fn private_tempdir() -> PrivateTestDirectory {
	private_fs::create_private_test_directory(&env::temp_dir())
		.expect("private temporary directory should be created")
}

fn test_env_mutex() -> &'static Mutex<()> {
	static TEST_ENV_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();

	TEST_ENV_MUTEX.get_or_init(|| Mutex::new(()))
}

#[cfg(test)]
mod tests {
	use std::{
		env,
		ffi::OsStr,
		fs::{self, Permissions},
		os::unix::{
			self,
			fs::{MetadataExt as _, PermissionsExt as _},
		},
		path::Path,
	};

	use crate::{private_fs, test_support};

	fn private_fixture_directory(path: &Path) {
		fs::create_dir(path).expect("fixture directory should be created");
		fs::set_permissions(path, Permissions::from_mode(0o700))
			.expect("fixture directory should be private");
	}

	#[test]
	fn private_tempdir_is_private_and_removes_unsafe_test_entries() {
		let temporary = test_support::private_tempdir();
		let path = temporary.path().to_path_buf();
		let file = path.join("ordinary");
		let link = path.join("link");

		fs::write(&file, b"fixture").expect("fixture file should be written");
		unix::fs::symlink(&file, &link).expect("fixture link should be created");

		assert_eq!(fs::metadata(&path).expect("temporary root metadata").mode() & 0o777, 0o700);

		drop(temporary);

		assert!(!path.exists());
	}

	#[test]
	fn private_tempdir_accepts_standard_public_tmp_parent() {
		if env::var_os("DECODEX_CANDIDATE_SANDBOX").as_deref() == Some(OsStr::new("1")) {
			return;
		}

		let temporary = private_fs::create_private_test_directory(Path::new("/tmp"))
			.expect("standard public temporary parent should be accepted");
		let path = temporary.path().to_path_buf();

		assert_eq!(fs::metadata(&path).expect("temporary root metadata").mode() & 0o777, 0o700);

		drop(temporary);

		assert!(!path.exists());
	}

	#[test]
	fn private_tempdir_rejects_non_private_parent_and_resolves_private_symlink() {
		let fixture = test_support::private_tempdir();
		let open_parent = fixture.path().join("open-parent");
		let private_parent = fixture.path().join("private-parent");
		let linked_parent = fixture.path().join("linked-parent");

		fs::create_dir(&open_parent).expect("open parent should be created");
		fs::set_permissions(&open_parent, Permissions::from_mode(0o755))
			.expect("open parent mode should be set");

		private_fixture_directory(&private_parent);

		unix::fs::symlink(&private_parent, &linked_parent)
			.expect("parent symlink should be created");

		assert!(private_fs::create_private_test_directory(&open_parent).is_err());

		let linked = private_fs::create_private_test_directory(&linked_parent)
			.expect("a canonicalized private parent symlink should be accepted");
		let path = linked.path().to_path_buf();

		drop(linked);

		assert!(!path.exists());
	}

	#[test]
	fn private_tempdir_detects_parent_replacement_without_writing_to_replacement() {
		let fixture = test_support::private_tempdir();
		let parent = fixture.path().join("parent");
		let displaced = fixture.path().join("displaced");

		private_fixture_directory(&parent);

		let replacement = parent.clone();
		let error = private_fs::create_private_test_directory_with(&parent, || {
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
		let fixture = test_support::private_tempdir();
		let temporary = private_fs::create_private_test_directory(fixture.path())
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
