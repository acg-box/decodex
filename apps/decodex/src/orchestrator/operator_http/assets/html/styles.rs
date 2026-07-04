mod accounts;
mod activity;
mod details;
mod foundation;
mod layout;
mod responsive;

pub(super) fn append_style_parts(html: &mut String) {
	foundation::append_style_parts(html);
	layout::append_style_parts(html);
	accounts::append_style_parts(html);
	activity::append_style_parts(html);
	details::append_style_parts(html);
	responsive::append_style_parts(html);
}
