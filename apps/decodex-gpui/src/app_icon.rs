use std::path::PathBuf;

pub(crate) fn app_icon_path() -> PathBuf {
	let packaged = std::env::current_exe()
		.ok()
		.and_then(|executable| executable.parent().map(std::path::Path::to_path_buf))
		.and_then(|macos| macos.parent().map(std::path::Path::to_path_buf))
		.map(|contents| contents.join("Resources/AppIcon.png"));

	packaged.filter(|path| path.is_file()).unwrap_or_else(|| {
		PathBuf::from(env!("CARGO_MANIFEST_DIR"))
			.join("../../assets/app-icon/generated/app-icon-flat.png")
	})
}
