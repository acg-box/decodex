//! Explicit user-selected export of a bounded recovery record.
use std::{
	env,
	fs::{File, OpenOptions},
	io::Write as _,
	os::unix::fs::OpenOptionsExt as _,
	path::{Path, PathBuf},
};

use crate::shell::agent_surface::drafts::storage::{AgentSurface, Context};
use decodex_protocol::{DesktopDraftDocument, DesktopRecoveredDraft};

impl AgentSurface {
	pub(crate) fn export_draft_copy(
		&mut self,
		copy: DesktopRecoveredDraft,
		cx: &mut Context<Self>,
	) {
		if !self.recovered_drafts().contains(&copy) {
			return;
		}

		let directory = env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| "/tmp".into());
		let destination = cx.prompt_for_new_path(&directory, Some("decodex-draft-copy.json"));

		cx.spawn(async move |surface, cx| {
			let Ok(Ok(Some(path))) = destination.await else { return };
			let result =
				cx.background_executor().spawn(async move { export_copy(&path, copy) }).await;
			let _ = surface.update(cx, |surface, cx| {
				surface.feedback = match result {
					Ok(()) => "Draft copy exported. The saved copy is still retained.".into(),
					Err(reason) => reason.into(),
				};

				cx.notify();
			});
		})
		.detach();
	}
}

fn export_copy(path: &Path, copy: DesktopRecoveredDraft) -> Result<(), &'static str> {
	let document = DesktopDraftDocument { recovered: vec![copy], ..Default::default() };
	let bytes = document.encode()?;
	let mut file = OpenOptions::new()
		.write(true)
		.create_new(true)
		.mode(0o600)
		.open(path)
		.map_err(|_| "Export could not create a new file. Choose a different filename.")?;

	file.write_all(&bytes)
		.and_then(|_| file.sync_all())
		.map_err(|_| "Export could not be confirmed. The saved draft copy is unchanged.")?;

	let parent = path.parent().ok_or("Export directory is unavailable")?;

	File::open(parent)
		.and_then(|parent| parent.sync_all())
		.map_err(|_| "Export could not be confirmed. The saved draft copy is unchanged.")?;

	Ok(())
}

#[cfg(test)]
mod tests {
	use std::os::unix::fs::{self, MetadataExt as _};

	use crate::shell::agent_surface::drafts::storage::recovery::export::{
		self, DesktopDraftDocument, DesktopRecoveredDraft,
	};
	#[test]
	fn exported_copy_round_trips_and_never_overwrites_existing_files() {
		let root = tempfile::tempdir().unwrap();
		let path = root.path().join("draft.json");
		let mut copy = DesktopRecoveredDraft { scope: None, draft: Default::default() };

		copy.draft.composer.text = "完整草稿 🧭".into();

		export::export_copy(&path, copy.clone()).unwrap();

		let data = std::fs::read(&path).unwrap();

		assert!(DesktopDraftDocument::decode(&data).unwrap().recovered[0] == copy);
		assert_eq!(std::fs::metadata(&path).unwrap().mode() & 0o777, 0o600);
		assert!(export::export_copy(&path, copy.clone()).is_err());

		let alias = root.path().join("alias.json");

		fs::symlink(&path, &alias).unwrap();

		assert!(export::export_copy(&alias, copy).is_err());
		assert_eq!(std::fs::read(path).unwrap(), data);
	}
}
