//! Explicit developer preparation uses the same component store as the desktop.
use std::{env, path::Path};

use reqwest as _;
use semver as _;
use serde as _;
use serde_json as _;
use sha2 as _;
use tempfile as _;

use decodex_codex as _;
use decodex_components::{ComponentStore, InstallProgress, RuntimeLock};

fn main() -> Result<(), String> {
	let arguments = env::args().skip(1).collect::<Vec<_>>();
	let args = arguments.iter().map(String::as_str).collect::<Vec<_>>();

	match args.as_slice() {
        ["prepare", lock] => prepare(lock, ComponentStore::for_user()?),
        ["prepare", lock, "--store", store] => prepare(lock, ComponentStore::new(store.into())),
        ["import-dev", lock, runtime] => import(lock, runtime, ComponentStore::for_user()?),
        ["import-dev", lock, runtime, "--store", store] => import(lock, runtime, ComponentStore::new(store.into())),
        ["source", lock, output] => decodex_components::prepare_source(&RuntimeLock::read(Path::new(lock))?, Path::new(output), progress),
        ["check-release", lock, baseline] => RuntimeLock::read(Path::new(lock))?.validate_release(&RuntimeLock::read(Path::new(baseline))?),
        ["pin", output, baseline] => {
            let version = env::var("DECODEX_CODEX_RELEASE").map_err(|_| "Set DECODEX_CODEX_RELEASE to an exact official version.")?;
            let lock = decodex_components::pin_release(&version, &RuntimeLock::read(Path::new(baseline))?, progress)?;

            lock.write(Path::new(output))?;

            println!("Pinned official Codex {} ({}).", lock.version, lock.upstream_commit);

            Ok(())
        }

        _ => Err("Usage: decodex-components prepare LOCK [--store DIR] | import-dev LOCK RUNTIME [--store DIR] | source LOCK ARCHIVE | pin OUTPUT BASELINE | check-release LOCK BASELINE".into()),
    }
}

fn prepare(path: &str, store: ComponentStore) -> Result<(), String> {
	let runtime = store.ensure(&RuntimeLock::read(Path::new(path))?, progress)?;

	println!("{}", runtime.executable.display());

	Ok(())
}

fn import(path: &str, runtime: &str, store: ComponentStore) -> Result<(), String> {
	store.import_development(&RuntimeLock::read(Path::new(path))?, Path::new(runtime), progress)
}

fn progress(phase: InstallProgress) {
	if !matches!(phase, InstallProgress::Downloading { .. }) {
		eprintln!("Codex: {phase:?}");
	}
}
