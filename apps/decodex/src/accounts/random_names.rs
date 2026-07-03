use std::collections::BTreeSet;

use crate::accounts::{ACCOUNT_RANDOM_NAMES, AccountSummary};

pub(super) fn random_name_seed_for(account_fingerprint: &str, email: Option<String>) -> String {
	if !account_fingerprint.trim().is_empty() {
		return account_fingerprint.to_owned();
	}

	if let Some(email) = email.filter(|value| !value.trim().is_empty()) {
		return email;
	}

	String::from("account")
}

pub(super) fn random_name_key(seed: &str) -> String {
	format!("{:08x}", account_identity_hash(seed))
}

pub(super) fn random_name(seed: &str, offset: i64) -> String {
	let index = (u64::from(account_identity_hash(seed))
		+ u64::try_from(normalize_random_name_offset(offset)).unwrap_or_default())
		% u64::try_from(ACCOUNT_RANDOM_NAMES.len()).unwrap_or(1);

	ACCOUNT_RANDOM_NAMES[usize::try_from(index).unwrap_or_default()].to_owned()
}

pub(super) fn assign_unique_random_names(accounts: &mut [AccountSummary]) {
	if accounts.len() < 2 {
		return;
	}

	let mut account_indexes = (0..accounts.len()).collect::<Vec<_>>();

	account_indexes.sort_by(|left, right| {
		accounts[*left]
			.random_name_key
			.cmp(&accounts[*right].random_name_key)
			.then_with(|| accounts[*left].selector.cmp(&accounts[*right].selector))
	});

	let mut used_names = BTreeSet::new();

	for index in account_indexes {
		let preferred_index = random_name_index(&accounts[index].random_name).unwrap_or_default();
		let name = unique_random_name_from(preferred_index, &used_names);

		used_names.insert(name.clone());

		accounts[index].random_name = name;
	}
}

pub(super) fn normalize_random_name_offset(offset: i64) -> i64 {
	offset.rem_euclid(i64::try_from(ACCOUNT_RANDOM_NAMES.len()).unwrap_or(1))
}

fn random_name_index(name: &str) -> Option<usize> {
	ACCOUNT_RANDOM_NAMES.iter().position(|candidate| *candidate == name)
}

fn unique_random_name_from(start_index: usize, used_names: &BTreeSet<String>) -> String {
	for probe in 0..ACCOUNT_RANDOM_NAMES.len() {
		let name = ACCOUNT_RANDOM_NAMES[(start_index + probe) % ACCOUNT_RANDOM_NAMES.len()];

		if !used_names.contains(name) {
			return name.to_owned();
		}
	}

	let base_name = ACCOUNT_RANDOM_NAMES[start_index % ACCOUNT_RANDOM_NAMES.len()];
	let mut suffix = 2;

	loop {
		let name = format!("{base_name} {suffix}");

		if !used_names.contains(&name) {
			return name;
		}

		suffix += 1;
	}
}

fn account_identity_hash(value: &str) -> u32 {
	let text = if value.trim().is_empty() { "account" } else { value };
	let mut hash = 2_166_136_261_u32;

	for unit in text.encode_utf16() {
		hash ^= u32::from(unit);
		hash = hash.wrapping_mul(16_777_619);
	}

	hash
}
