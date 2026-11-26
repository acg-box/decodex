//! One recoverable cold snapshot of the Compose project. The pending directory is also
//! the journal: data can change only after "ready", and rotation only after "committed".
use super::*;

const CONFIG: &[&str] = &[
	"installation.json",
	"compose.json",
	"service_conf.yaml.template",
	"init.sql",
	"kvrocks.conf",
	"infinity.toml",
	"infinity-runtime/Dockerfile",
];

pub(super) fn lock(directory: &Path) -> Result<fs::File> {
	let parent = directory.parent().ok_or("Missing component directory.")?;
	fs::create_dir_all(parent).map_err(io)?;
	let lock = fs::File::options()
		.read(true)
		.write(true)
		.create(true)
		.truncate(false)
		.open(parent.join("operations.lock"))
		.map_err(io)?;
	lock.try_lock().map_err(|_| "Another RAGFlow operation is in progress.")?;
	Ok(lock)
}

pub(super) fn recovery_pending(service: &Path) -> bool {
	let dir = service.with_file_name("upgrades");
	["pending", "retired", "previous/restore-requested"].iter().any(|p| dir.join(p).exists())
}

pub(super) fn lock_configuration(directory: &Path) -> Result<fs::File> {
	let lock = lock(directory)?;
	if recovery_pending(directory) {
		Snapshot::new(directory, &docker()?).recover()?;
	}
	Ok(lock)
}

fn io(error: std::io::Error) -> String {
	format!("RAGFlow backup: {error}")
}

fn remove(path: &Path) -> Result<()> {
	match fs::remove_dir_all(path) {
		Ok(()) => Ok(()),
		Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
		Err(e) => Err(io(e)),
	}
}

fn copy_config(from: &Path, to: &Path) -> Result<()> {
	for name in CONFIG {
		let dest = to.join(name);
		fs::create_dir_all(dest.parent().expect("config parent")).map_err(io)?;
		private_file(&dest, &fs::read(from.join(name)).map_err(io)?)?;
	}
	Ok(())
}

pub(super) struct Snapshot<'a> {
	service: &'a Path,
	docker: &'a Path,
	root: PathBuf,
}
impl<'a> Snapshot<'a> {
	pub fn new(service: &'a Path, docker: &'a Path) -> Self {
		Self { service, docker, root: service.with_file_name("upgrades") }
	}

	fn pending(&self) -> PathBuf {
		self.root.join("pending")
	}

	pub fn compose(&self, config: &Installation, file: &Path, args: &[&str]) -> Result<String> {
		let mut cmd = Command::new(self.docker);
		cmd.args(["compose", "--project-name", &config.project, "--project-directory"])
			.arg(file.parent().expect("compose directory"))
			.arg("--file")
			.arg(file)
			.args(args);
		run_command(&mut cmd, &config.password)
	}

	fn volume_archives(&self, config: &Installation, restore: bool) -> Result<()> {
		let dir = self.pending();
		let frozen: Value = serde_json::from_slice(&fs::read(dir.join("frozen.json")).map_err(io)?)
			.map_err(|_| "Invalid recovery manifest.")?;
		let mut cmd = Command::new(self.docker);
		cmd.args(["run", "--rm", "--network", "none", "--mount"]).arg(format!(
			"type=bind,source={},target=/backup{}",
			dir.display(),
			if restore { ",readonly" } else { "" }
		));
		let volumes = frozen["volumes"].as_object().ok_or("Missing managed volumes.")?;
		for name in volumes.keys() {
			let volume = format!("{}_{}", config.project, name);
			// An absent source must not be silently recreated as an empty backup.
			run_command(
				Command::new(self.docker).args([
					"volume",
					"inspect",
					"--format",
					"{{.Name}}",
					&volume,
				]),
				&config.password,
			)?;
			cmd.arg("--mount").arg(format!(
				"type=volume,source={volume},target=/data/{name}{}",
				if restore { "" } else { ",readonly" }
			));
		}
		let helper =
			frozen["services"]["mysql"]["image"].as_str().ok_or("Missing recovery image.")?;
		cmd.args(["--entrypoint","/bin/sh",helper,"-ec",if restore {
            r#"for v do (cd "/data/$v" && find . -mindepth 1 -delete && tar --numeric-owner -xpf "/backup/$v.tar"); done"#
        } else {
            r#"for v do (cd "/data/$v" && tar --sparse --numeric-owner -cpf "/backup/$v.tar" . && tar --compare --numeric-owner -f "/backup/$v.tar"); done"#
        },"volumes"]).args(volumes.keys());
		run_command(&mut cmd, &config.password).map(|_| ())
	}

	pub fn create(&self, config: &Installation) -> Result<()> {
		let dir = self.pending();
		fs::create_dir_all(&self.root).map_err(io)?;
		fs::set_permissions(&self.root, fs::Permissions::from_mode(0o700)).map_err(io)?;
		fs::create_dir(&dir).map_err(io)?;
		copy_config(self.service, &dir)?;
		let manifest = self.service.join("compose.json");
		let mut frozen: Value = serde_json::from_slice(&fs::read(&manifest).map_err(io)?)
			.map_err(|_| "Invalid installed service manifest.")?;
		let mut save = Command::new(self.docker);
		save.args(["image", "save", "--output"]).arg(dir.join("images.tar"));
		for (name, svc) in
			frozen["services"].as_object_mut().ok_or("Missing installed services.")?
		{
			let container = self.compose(config, &manifest, &["ps", "--all", "--quiet", name])?;
			let id = if !container.trim().is_empty() {
				run_command(
					Command::new(self.docker).args([
						"inspect",
						"--format",
						"{{.Image}}",
						container.trim(),
					]),
					&config.password,
				)?
			} else {
				let image = if name == "ragflow" {
					&config.image
				} else {
					svc["image"].as_str().ok_or("Missing installed image.")?
				};
				run_command(
					Command::new(self.docker)
						.args(["image", "inspect", "--format", "{{.Id}}", image]),
					&config.password,
				)?
			};
			let id = id.trim();
			if !super::super::update::valid_digest(id) {
				return Err("Invalid installed image identity.".into());
			}
			svc["image"] = json!(id);
			svc.as_object_mut().expect("installed service").remove("build");
			save.arg(id);
		}
		private_file(
			&dir.join("frozen.json"),
			&serde_json::to_vec(&frozen).map_err(|e| e.to_string())?,
		)?;
		run_command(&mut save, &config.password)?;
		private_file(&dir.join("stopping"), b"")?;
		self.compose(config, &manifest, &["stop"])?;
		self.volume_archives(config, false)?;
		private_file(&dir.join("ready"), b"")
	}

	fn rotate(&self) -> Result<()> {
		let previous = self.root.join("previous");
		let retired = self.root.join("retired");
		remove(&retired)?;
		if previous.exists() {
			fs::rename(&previous, &retired).map_err(io)?;
		}
		fs::rename(self.pending(), previous).map_err(io)?;
		remove(&retired)
	}

	pub fn commit(&self) -> Result<()> {
		private_file(&self.pending().join("committed"), b"")?;
		self.rotate()
	}

	pub fn restore_previous(&self) -> Result<()> {
		self.recover()?;
		let previous = self.root.join("previous");
		if !previous.join("committed").is_file() {
			return Err("No previous RAGFlow backup is available.".into());
		}
		private_file(&previous.join("restore-requested"), b"")?;
		self.recover()
	}

	pub fn recover(&self) -> Result<()> {
		let dir = self.pending();
		let previous = self.root.join("previous");
		if !dir.exists() && previous.join("restore-requested").is_file() {
			fs::rename(previous, &dir).map_err(io)?;
		}
		if !dir.exists() {
			return remove(&self.root.join("retired"));
		}
		if dir.join("committed").exists() && !dir.join("restore-requested").exists() {
			return self.rotate();
		}
		if dir.join("stopping").exists() {
			let config = read(&dir)?.ok_or("Missing recovery receipt.")?;
			let active = self.service.join("compose.json");
			if dir.join("ready").exists() {
				self.compose(&config, &active, &["stop"])?;
				run_command(
					Command::new(self.docker)
						.args(["image", "load", "--input"])
						.arg(dir.join("images.tar")),
					&config.password,
				)?;
				self.volume_archives(&config, true)?;
				copy_config(&dir, self.service)?;
			}
			if config.installed {
				// Resolve the old bind mounts against the live service directory.
				let mut cmd = Command::new(self.docker);
				cmd.args(["compose", "--project-name", &config.project, "--project-directory"])
					.arg(self.service)
					.arg("--file")
					.arg(dir.join("frozen.json"))
					.args([
						"up",
						"--detach",
						"--no-build",
						"--pull",
						"never",
						"--wait",
						"--wait-timeout",
						"600",
					]);
				run_command(&mut cmd, &config.password)?;
				wait_ready(&config)?;
			}
		}
		remove(&dir)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn keep_one_committed_snapshot_across_retries_and_interrupted_rotation() {
		let tmp = tempfile::tempdir().unwrap();
		let service = tmp.path().join("service");
		let backup = Snapshot::new(&service, Path::new("/unused"));
		for n in 0..4 {
			fs::create_dir_all(backup.pending()).unwrap();
			fs::write(backup.pending().join("version"), n.to_string()).unwrap();
			backup.commit().unwrap();
			fs::create_dir(backup.pending()).unwrap();
			backup.recover().unwrap();
			assert_eq!(
				fs::read_to_string(backup.root.join("previous/version")).unwrap(),
				n.to_string()
			);
			assert_eq!(fs::read_dir(&backup.root).unwrap().count(), 1);
		}
		fs::create_dir(backup.pending()).unwrap();
		private_file(&backup.pending().join("committed"), b"").unwrap();
		fs::rename(backup.root.join("previous"), backup.root.join("retired")).unwrap();
		backup.recover().unwrap();
		assert_eq!(fs::read_dir(&backup.root).unwrap().count(), 1);
		assert!(backup.root.join("previous/committed").is_file());
	}
}
