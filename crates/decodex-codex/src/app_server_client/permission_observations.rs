//! Bounded wire-order task settings observations, before the coordinator consumes its event queue.
use super::settings_guard::SettingsGuard;
use serde::Serialize;
use std::{
	collections::HashMap,
	sync::{
		Arc, Mutex,
		atomic::{AtomicU64, Ordering},
	},
};
#[derive(Clone)]
pub(super) struct SettingsObservations<T>(Arc<Mutex<HashMap<String, Entry<T>>>>, Arc<AtomicU64>);
impl<T> Default for SettingsObservations<T> {
	fn default() -> Self {
		Self(Arc::new(Mutex::new(HashMap::new())), Arc::new(AtomicU64::new(0)))
	}
}
struct Entry<T> {
	settings: Option<T>,
	guard: Option<SettingsGuard>,
	active_turn: Option<String>,
	bytes: usize,
}
impl<T: Clone + Serialize> SettingsObservations<T> {
	pub(super) fn revision(&self) -> u64 {
		self.1.load(Ordering::Acquire)
	}

	fn invalidate_hydration(&self) {
		let _ =
			self.1.fetch_update(Ordering::AcqRel, Ordering::Acquire, |v| Some(v.saturating_add(1)));
	}

	pub(super) fn record(&self, thread: &str, settings: Option<T>, guard: Option<SettingsGuard>) {
		self.invalidate_hydration();
		let Ok(mut rows) = self.0.lock() else {
			return;
		};
		let active_turn = rows.remove(thread).and_then(|row| row.active_turn);
		let (settings, guard, bytes) = match (settings, guard) {
			(Some(settings), Some(guard)) => {
				let bytes = serde_json::to_vec(&settings).map_or(usize::MAX, |v| v.len());
				if rows.values().map(|r| r.bytes).sum::<usize>().saturating_add(bytes)
					> 8 * 1024 * 1024
				{
					(None, None, 0)
				} else {
					(Some(settings), Some(guard), bytes)
				}
			},
			_ => (None, None, 0),
		};
		if rows.len() >= 256 || (settings.is_none() && active_turn.is_none()) {
			return;
		}
		rows.insert(thread.into(), Entry { settings, guard, active_turn, bytes });
	}

	pub(super) fn configured(&self, thread: &str) -> Option<T> {
		self.0.lock().ok()?.get(thread)?.settings.clone()
	}

	pub(super) fn get(&self, thread: &str) -> Option<(T, SettingsGuard)> {
		let rows = self.0.lock().ok()?;
		let row = rows.get(thread)?;
		if row.active_turn.is_some() {
			return None;
		}
		let guard = row.guard.as_ref()?;
		if !guard.is_live() {
			return None;
		}
		Some((row.settings.clone()?, guard.clone()))
	}

	pub(super) fn start_turn(&self, thread: &str, turn: Option<&str>) {
		self.invalidate_hydration();
		let Ok(mut rows) = self.0.lock() else {
			return;
		};
		let valid =
			|s: &str| !s.trim().is_empty() && s.len() <= 512 && !s.chars().any(char::is_control);
		let Some(turn) = turn.filter(|turn| valid(turn) && valid(thread)) else {
			rows.remove(thread);
			return;
		};
		if let Some(row) = rows.get_mut(thread) {
			row.active_turn = Some(turn.into());
			row.guard = None;
		} else if rows.len() < 256 {
			rows.insert(
				thread.into(),
				Entry { settings: None, guard: None, active_turn: Some(turn.into()), bytes: 0 },
			);
		}
	}

	pub(super) fn finish_turn(
		&self,
		thread: &str,
		turn: Option<&str>,
		guard: Option<SettingsGuard>,
	) {
		let Ok(mut rows) = self.0.lock() else {
			return;
		};
		let Some(row) = rows.get_mut(thread) else {
			return;
		};
		if turn.is_some() && row.active_turn.as_deref() == turn {
			// Native publishes every effective persistent-settings change. An unchanged turn has
			// no settings publication; retain the last facts, but never revive malformed facts.
			row.active_turn = None;
			row.guard = guard;
			self.invalidate_hydration();
		}
	}

	pub(super) fn remove(&self, thread: &str) {
		self.invalidate_hydration();
		if let Ok(mut rows) = self.0.lock() {
			rows.remove(thread);
		}
	}

	pub(super) fn clear(&self) {
		self.invalidate_hydration();
		if let Ok(mut rows) = self.0.lock() {
			rows.clear();
		}
	}
}
