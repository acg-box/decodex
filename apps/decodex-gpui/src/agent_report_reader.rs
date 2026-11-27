//! Open a report from the original reply in the desktop document overlay.
use super::*;
use crate::shell::agent_surface::OpenReport;
use std::{
	io::{self, ErrorKind, Read},
	path::{Path, PathBuf},
	sync::Arc,
};

impl AgentSurface {
	pub(in crate::shell::agent_surface) fn is_readable_report(path: &Path) -> bool {
		path.is_absolute()
			&& path.extension().and_then(|s| s.to_str()).is_some_and(|s| {
				["md", "markdown", "txt"].iter().any(|ext| ext.eq_ignore_ascii_case(s))
			})
	}

	pub(in crate::shell::agent_surface) fn open_reference_report(
		&mut self,
		path: PathBuf,
		cx: &mut Context<Self>,
	) {
		let source = path.clone();
		let read = cx.background_executor().spawn(async move { read_document(&source) });
		let current = path.clone();
		let task = cx.spawn(async move |weak, cx| {
			let result = read.await;
			let _ = weak.update(cx, |s, cx| {
				if let Some(report) = &mut s.report
					&& report.path == current
				{
					report.content = Some(result);
					cx.notify();
				}
			});
		});
		self.report =
			Some(OpenReport { path, content: None, scroll: Default::default(), _read: task });
		cx.emit(crate::shell::agent_surface::ReportOpened);
		cx.notify();
	}

	pub(in crate::shell) fn close_report(&mut self, cx: &mut Context<Self>) {
		self.report = None;
		cx.notify();
	}

	pub(in crate::shell) fn render_report(&self, cx: &mut Context<Self>) -> AnyElement {
		let Some(report) = &self.report else { return gpui::div().into_any_element() };
		let key = SharedString::from(format!("document-reader-{}", report.path.display()));
		gpui::div()
			.size_full()
			.min_w_0()
			.min_h_0()
			.pt(gpui::px(36.))
			.child(
				gpui::div()
					.id(key.clone())
					.debug_selector(|| "document-viewport".into())
					.size_full()
					.min_h_0()
					.overflow_y_scroll()
					.px(gpui::px(24.))
					.child(
						gpui::div()
							.w_full()
							.max_w(gpui::px(780.))
							.mx_auto()
							.pb(gpui::px(28.))
							.debug_selector(|| "document-content".into())
							.child(self.document_reader(&report.path, report.content.as_ref(), cx)),
					)
					.smooth_scroll(key)
					.with_handle(&report.scroll)
					.contain_scroll()
					.with_scrollbar("document-scrollbar", "Scroll document"),
			)
			.into_any_element()
	}

	fn document_reader(
		&self,
		path: &Path,
		content: Option<&io::Result<Arc<str>>>,
		cx: &mut Context<Self>,
	) -> Div {
		let title = path.file_name().unwrap_or_default().to_string_lossy();
		let plain = path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("txt"));
		let has_heading = !plain
			&& content
				.and_then(|c| c.as_ref().ok())
				.is_some_and(|text| text.trim_start().starts_with("# "));
		let mut body = reading_body().when(!has_heading, |body| body.child(reader_title(&title)));
		match content {
			Some(Ok(text)) if text.trim().is_empty() =>
				body = body.child(note("This document is empty.")),
			Some(Ok(text)) if plain =>
				body =
					body.child(crate::shell::agent_surface::selectable_text::SelectableText::plain(
						"reference-plain-document".into(),
						text.to_string(),
					)),
			Some(Ok(text)) =>
				body = body.child(markdown::render(
					text,
					&format!("reference-document-{}", path.display()),
				)),
			Some(Err(error)) => {
				let retry = path.to_owned();
				let message = match error.kind() {
					ErrorKind::NotFound =>
						"This file is no longer at the location linked in the conversation.".into(),
					ErrorKind::PermissionDenied =>
						"Decodex does not have permission to read this file.".into(),
					_ => error.to_string(),
				};
				body = body.child(note(&message)).child(self.reference_link(
					"reference-retry-document".into(),
					"Try again",
					Symbol::Restore,
					move |s, cx| s.open_reference_report(retry.clone(), cx),
					cx,
				));
			},
			None => body = body.child(note("Opening document…")),
		}
		if content
			.is_some_and(|result| matches!(result, Err(err) if err.kind() == ErrorKind::NotFound))
		{
			return body;
		}
		let file = path.to_owned();
		body.child(self.reference_link(
			"reference-show-file".into(),
			if cfg!(target_os = "macos") { "Show in Finder" } else { "Show in folder" },
			Symbol::Forward,
			move |_, cx| cx.reveal_path(&file),
			cx,
		))
	}
}

fn read_document(path: &Path) -> io::Result<Arc<str>> {
	const LIMIT: u64 = 2 * 1024 * 1024;
	let metadata = path.metadata()?;
	if !metadata.is_file() {
		return Err(io::Error::new(ErrorKind::InvalidInput, "This path is not a regular file."));
	}
	let size_error = "This document is too large for an inline preview.";
	if metadata.len() > LIMIT {
		return Err(io::Error::new(ErrorKind::InvalidData, size_error));
	}
	let mut bytes = Vec::with_capacity(metadata.len() as usize);
	std::fs::File::open(path)?.take(LIMIT + 1).read_to_end(&mut bytes)?;
	if bytes.len() as u64 > LIMIT {
		return Err(io::Error::new(ErrorKind::InvalidData, size_error));
	}
	String::from_utf8(bytes).map(Arc::from).map_err(|_| {
		io::Error::new(ErrorKind::InvalidData, "This file does not contain UTF-8 text.")
	})
}
