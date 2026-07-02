pub(in crate::agent::app_server::schema_probe) fn command_output_excerpt(output: &[u8]) -> String {
	let text = String::from_utf8_lossy(output);
	let trimmed = text.trim();
	let excerpt = trimmed.chars().take(1_000).collect::<String>();

	if excerpt.is_empty() { String::from("<empty>") } else { excerpt }
}
