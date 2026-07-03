use crate::docs_okf::{DocsCheckIssue, PathBuf};

pub(in crate::docs_okf) fn issue(path: Option<PathBuf>, message: String) -> DocsCheckIssue {
	DocsCheckIssue { path, message }
}
