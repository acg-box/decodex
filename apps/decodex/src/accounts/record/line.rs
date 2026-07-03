use serde::{Deserialize, Serialize};

use crate::{
	accounts::{
		auth_json::{self, AuthDotJson},
		record::model::{AccountPoolRecord, is_false},
	},
	prelude::Result,
};

#[derive(Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub(in crate::accounts) enum AccountPoolLine {
	Wrapped {
		#[serde(skip_serializing_if = "Option::is_none")]
		email: Option<String>,
		#[serde(default, skip_serializing_if = "is_false")]
		disabled: bool,
		#[serde(skip_serializing_if = "Option::is_none")]
		cooldown_until_unix_epoch: Option<i64>,
		#[serde(skip_serializing_if = "Option::is_none")]
		cooldown_until: Option<String>,
		#[serde(skip_serializing_if = "Option::is_none")]
		last_selected_at_unix_epoch: Option<i64>,
		#[serde(skip_serializing_if = "Option::is_none")]
		auth_failed_at_unix_epoch: Option<i64>,
		#[serde(skip_serializing_if = "Option::is_none")]
		auth_failure: Option<String>,
		auth: AuthDotJson,
	},
	Flat(AccountPoolRecord),
}
impl AccountPoolLine {
	pub(in crate::accounts) fn into_record(self) -> Result<AccountPoolRecord> {
		match self {
			Self::Flat(record) => Ok(record),
			Self::Wrapped {
				email,
				disabled,
				cooldown_until_unix_epoch,
				cooldown_until,
				last_selected_at_unix_epoch,
				auth_failed_at_unix_epoch,
				auth_failure,
				auth,
			} => {
				let mut record = AccountPoolRecord::from_auth(auth)?;

				record.email = auth_json::first_nonblank_string(email, record.email);
				record.disabled = disabled;
				record.cooldown_until_unix_epoch = cooldown_until_unix_epoch;
				record.cooldown_until = cooldown_until;
				record.last_selected_at_unix_epoch = last_selected_at_unix_epoch;
				record.auth_failed_at_unix_epoch = auth_failed_at_unix_epoch;
				record.auth_failure = auth_failure;

				Ok(record)
			},
		}
	}
}
