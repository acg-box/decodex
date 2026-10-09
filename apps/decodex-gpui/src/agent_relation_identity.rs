//! Persistent display numbers are independent of graph layout and metric ranking.
use std::{
	collections::BTreeMap,
	sync::{LazyLock, Mutex},
};

static IDS: LazyLock<Mutex<BTreeMap<String, usize>>> = LazyLock::new(|| {
	let saved = path()
		.and_then(|p| std::fs::read(p).ok())
		.and_then(|bytes| serde_json::from_slice(&bytes).ok())
		.unwrap_or_default();
	Mutex::new(saved)
});

fn path() -> Option<std::path::PathBuf> {
	if cfg!(test) {
		return None;
	}
	Some(
		std::path::PathBuf::from(std::env::var_os("HOME")?)
			.join("Library/Application Support/Decodex/agent-labels.json"),
	)
}

fn assign(ids: &mut BTreeMap<String, usize>, keys: impl Iterator<Item = String>) -> bool {
	let mut next = ids.values().copied().max().unwrap_or(0) + 1;
	let mut changed = false;
	for key in keys.collect::<std::collections::BTreeSet<_>>() {
		if let std::collections::btree_map::Entry::Vacant(entry) = ids.entry(key) {
			entry.insert(next);
			next += 1;
			changed = true;
		}
	}
	changed
}

pub(super) fn labels(keys: impl Iterator<Item = String>) -> BTreeMap<String, String> {
	let keys: Vec<_> = keys.collect();
	let mut ids = IDS.lock().unwrap_or_else(|e| e.into_inner());
	if assign(&mut ids, keys.iter().cloned())
		&& let Some(path) = path()
	{
		let save = || -> std::io::Result<()> {
			std::fs::create_dir_all(path.parent().unwrap())?;
			let staging = path.with_extension("tmp");
			std::fs::write(&staging, serde_json::to_vec(&*ids)?)?;
			std::fs::rename(staging, &path)
		};
		if let Err(error) = save() {
			eprintln!("Cannot save graph agent labels: {error}");
		}
	}
	keys.into_iter()
		.map(|key| {
			let label = format!("A{:02}", ids[&key]);
			(key, label)
		})
		.collect()
}

#[cfg(test)]
mod tests {
	#[test]
	fn labels_survive_reordering_additions_and_reload() {
		let mut ids = std::collections::BTreeMap::new();
		super::assign(&mut ids, ["b", "c"].into_iter().map(String::from));
		let first = ids.clone();
		super::assign(&mut ids, ["c", "a", "b"].into_iter().map(String::from));
		assert_eq!(ids["b"], first["b"]);
		assert_eq!(ids["c"], first["c"]);
		assert!(ids["a"] > ids["c"]);
		let bytes = serde_json::to_vec(&ids).unwrap();
		let restored: std::collections::BTreeMap<String, usize> =
			serde_json::from_slice(&bytes).unwrap();
		assert_eq!(restored, ids);
	}
}
