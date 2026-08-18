mod auth_json;
mod commands;
mod file_security;
mod identity;
mod output;
mod random_names;
mod record;
mod store;
mod types;
mod usage_history;

pub(crate) use self::{
	commands::{
		account_clear, account_import, account_list, account_list_with_cached_usage,
		account_logout, account_reroll_name, account_select, account_use,
		hydrate_account_list_usage, run_account_clear, run_account_import, run_account_list,
		run_account_logout, run_account_select, run_account_use,
	},
	file_security::secure_account_file,
	types::{
		AccountIdentitySummary, AccountImportRequest, AccountListResponse, AccountSummary,
		AccountUseRequest, AccountUseResponse,
	},
};

#[cfg(test)]
use self::usage_history::{account_capacity_multiplier, usage_history_path, usage_record_date};

pub(in crate::accounts) const ACCOUNT_RANDOM_NAMES: &[&str] = &[
	"Alex", "Avery", "Bailey", "Blake", "Casey", "Charlie", "Clara", "Dana", "Drew", "Eden",
	"Elliot", "Emery", "Evan", "Finley", "Harper", "Hayden", "Iris", "Jamie", "Jordan", "Kai",
	"Kendall", "Lane", "Liam", "Logan", "Mason", "Maya", "Mia", "Morgan", "Noah", "Nora", "Owen",
	"Paige", "Parker", "Quinn", "Reese", "Remy", "Riley", "Rowan", "Sage", "Sasha", "Sidney",
	"Taylor", "Theo", "Val",
];

#[cfg(test)] mod tests;
