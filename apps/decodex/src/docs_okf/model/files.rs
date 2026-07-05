use std::path::PathBuf;

#[derive(Debug)]
pub(in crate::docs_okf) struct DocsFile {
	pub(in crate::docs_okf) path: PathBuf,
	pub(in crate::docs_okf) relative_path: PathBuf,
	pub(in crate::docs_okf) content: Option<String>,
	pub(in crate::docs_okf) read_error: Option<String>,
}

pub(in crate::docs_okf) struct OkfScaffoldFile {
	pub(in crate::docs_okf) relative_path: &'static str,
	pub(in crate::docs_okf) content: &'static str,
}
