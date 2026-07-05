use std::{ffi::OsString, path::Path};

use crate::cli::{self};

#[test]
fn builds_macos_open_arguments_for_decodex_app() {
	assert_eq!(
		cli::decodex_app_open_args(None, false),
		vec![OsString::from("-a"), OsString::from("Decodex")]
	);
	assert_eq!(
		cli::decodex_app_open_args(Some(Path::new("target/decodex-app/Decodex.app")), true,),
		vec![
			OsString::from("-n"),
			Path::new("target/decodex-app/Decodex.app").as_os_str().to_owned(),
		]
	);
}
