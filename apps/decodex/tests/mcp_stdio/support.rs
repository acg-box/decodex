use std::{
	path::PathBuf,
	process::{Child, ExitStatus, Output},
};

use tempfile::TempDir;

pub(crate) struct TestProject {
	pub(crate) _home: TempDir,
	pub(crate) _project: TempDir,
	pub(crate) repo_path: PathBuf,
	pub(crate) home_path: PathBuf,
	pub(crate) config_path: PathBuf,
}

pub(crate) struct ChildGuard {
	child: Option<Child>,
}
impl ChildGuard {
	pub(crate) fn new(child: Child) -> Self {
		Self { child: Some(child) }
	}

	pub(crate) fn try_wait(&mut self) -> Option<ExitStatus> {
		self.child.as_mut().and_then(|child| child.try_wait().expect("child wait should run"))
	}

	pub(crate) fn stop(mut self) -> Output {
		let mut child = self.child.take().expect("child should exist");
		let _ = child.kill();

		child.wait_with_output().expect("child output should collect")
	}
}

impl Drop for ChildGuard {
	fn drop(&mut self) {
		if let Some(child) = self.child.as_mut() {
			let _ = child.kill();
			let _ = child.wait();
		}
	}
}
