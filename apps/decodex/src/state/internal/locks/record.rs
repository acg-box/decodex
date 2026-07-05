use std::{
	fs::File,
	io::{Read as _, Seek as _, SeekFrom, Write as _},
	path::Path,
};

use crate::{
	prelude::{Result, eyre},
	state::IssueLease,
};

pub(in crate::state) fn write_issue_claim_record(
	lock_file: &mut File,
	project_id: &str,
	issue_id: &str,
	run_id: &str,
	issue_state: &str,
) -> Result<()> {
	lock_file.set_len(0)?;
	lock_file.seek(SeekFrom::Start(0))?;

	write!(
		lock_file,
		"project_id={project_id}\nissue_id={issue_id}\nrun_id={run_id}\nissue_state={issue_state}\n"
	)?;

	lock_file.flush()?;

	Ok(())
}

pub(in crate::state) fn read_issue_claim_record(path: &Path) -> Result<Option<IssueLease>> {
	let mut body = String::new();
	let mut file = File::open(path)?;

	file.read_to_string(&mut body)?;

	if body.trim().is_empty() {
		return Ok(None);
	}

	let mut project_id = None;
	let mut issue_id = None;
	let mut run_id = None;
	let mut issue_state = None;

	for line in body.lines().filter(|line| !line.trim().is_empty()) {
		let (key, value) = line
			.split_once('=')
			.ok_or_else(|| eyre::eyre!("issue claim record `{}` is malformed", path.display()))?;

		match key {
			"project_id" => project_id = Some(value.to_owned()),
			"issue_id" => issue_id = Some(value.to_owned()),
			"run_id" => run_id = Some(value.to_owned()),
			"issue_state" => issue_state = Some(value.to_owned()),
			_ => {},
		}
	}

	let Some(project_id) = project_id else {
		return Err(eyre::eyre!("issue claim record `{}` is missing project_id", path.display()));
	};
	let Some(issue_id) = issue_id else {
		return Err(eyre::eyre!("issue claim record `{}` is missing issue_id", path.display()));
	};
	let Some(run_id) = run_id else {
		return Err(eyre::eyre!("issue claim record `{}` is missing run_id", path.display()));
	};
	let Some(issue_state) = issue_state else {
		return Err(eyre::eyre!("issue claim record `{}` is missing issue_state", path.display()));
	};

	Ok(Some(IssueLease { project_id, issue_id, run_id, issue_state }))
}
