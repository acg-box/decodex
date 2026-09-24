//! One durable activation reservation per account. No prompts or responses are stored.

use decodex_core::{AccountId, AccountQuotaWindow};
use rusqlite::{OptionalExtension as _, TransactionBehavior, params};

use crate::{SqliteStore, StoreError, error::sqlite_error};

const REJECTION_BACKOFF_MICROS: i64 = 900_000_000;
const COUNTDOWN_SAMPLE_MICROS: i64 = 30_000_000;
const RESET_CLOCK_TOLERANCE_MICROS: i64 = 15_000_000;

fn full_window_ahead(reset: i64, observed_at: i64) -> bool {
	let duration = i64::from(AccountQuotaWindow::SEVEN_DAYS_MINUTES) * 60_000_000;
	reset.saturating_sub(observed_at).abs_diff(duration) <= RESET_CLOCK_TOLERANCE_MICROS as u64
}

impl SqliteStore {
	/// Observe expiry or a floating weekly reset and reserve activation before any network effect.
	/// Completed or ambiguous attempts remain suppressed until the provider advances the reset.
	/// Percentages never determine whether a window needs activation.
	pub async fn claim_quota_activation(
		&self,
		account_id: &AccountId,
		account_revision: i64,
		weekly: AccountQuotaWindow,
		can_send: bool,
		now: i64,
	) -> Result<bool, StoreError> {
		if now <= 0 || weekly.duration_minutes != AccountQuotaWindow::SEVEN_DAYS_MINUTES {
			return Err(StoreError::InvalidInput("invalid activation observation"));
		}
		let account_id = account_id.as_str().to_owned();
		self.run(move |connection| {
			let tx = connection
				.transaction_with_behavior(TransactionBehavior::Immediate)
				.map_err(sqlite_error)?;
			let enabled: bool = tx
				.query_row(
					"SELECT EXISTS(SELECT 1 FROM accounts, desktop_settings
				 WHERE accounts.account_id=?1 AND accounts.revision=?2 AND accounts.enabled=1
				 AND accounts.tombstoned_at_micros IS NULL AND desktop_settings.auto_activate_quota=1)",
					params![account_id, account_revision],
					|row| row.get(0),
				)
				.map_err(sqlite_error)?;
			if !enabled {
				return Ok(false);
			}
			let prior: Option<(i64, i64, Option<i64>)> = tx
				.query_row(
					"SELECT observed_reset_at_micros,next_due_at_micros,observed_at_micros
				 FROM account_quota_activation WHERE account_id=?1",
					[&account_id],
					|row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
				)
				.optional()
				.map_err(sqlite_error)?;
			let reported_reset = weekly.resets_at_unix_micros;
			// An unstarted window reports approximately now + seven days on every poll.
			// Keep the first sample as an anchor; polling must not move the deadline forever.
			let full_window = full_window_ahead(reported_reset, now);
			let floating = full_window && prior.is_some_and(|(reset, _, observed)| {
				observed.is_some_and(|at| full_window_ahead(reset, at)
					&& now.saturating_sub(at) >= COUNTDOWN_SAMPLE_MICROS
					&& reported_reset > reset)
			});
			let (reset, mut due, observed_at) = match prior {
				Some((reset, due, Some(at))) if full_window && full_window_ahead(reset, at) =>
					(reset, due, at),
				Some((previous_reset, due, None)) if full_window =>
					(reported_reset, if due == previous_reset { reported_reset } else { due }, now),
				Some((previous_reset, due, _)) if reported_reset <= previous_reset =>
					(previous_reset, due, now),
				_ => (reported_reset, reported_reset, now),
			};
			let expired = reported_reset == reset && reset <= now;
			// A floating window can activate before its nominal reset. A reservation or
			// rejection backoff still takes precedence over that trigger.
			let ready = due <= now || (floating && due == reset);
			let claim = can_send && (expired || floating) && ready;
			if claim {
				// A floating reset stays reserved until a real countdown starts, even after restart.
				due = i64::MAX;
			}

			tx.execute(
				"INSERT INTO account_quota_activation
				 (account_id,observed_reset_at_micros,next_due_at_micros,attempted_at_micros,outcome,observed_at_micros)
				 VALUES (?1,?2,?3,?4,?5,?7)
				 ON CONFLICT(account_id) DO UPDATE SET observed_reset_at_micros=?2,next_due_at_micros=?3,
				 observed_at_micros=?7,
				 attempted_at_micros=CASE WHEN ?6 THEN ?4 ELSE attempted_at_micros END,
				 outcome=CASE WHEN ?6 THEN 'unknown' ELSE outcome END",
				params![
					account_id,
					reset,
					due,
					claim.then_some(now),
					if claim { "unknown" } else { "idle" },
					claim,
					observed_at
				],
			)
			.map_err(sqlite_error)?;
			tx.commit().map_err(sqlite_error)?;
			Ok(claim)
		})
		.await
	}

	/// Record a positive completion or a known pre-acceptance failure for this reservation.
	/// Transport failures leave the pre-send unknown outcome intact and never cause replay.
	pub async fn finish_quota_activation(
		&self,
		account_id: &AccountId,
		attempted_at: i64,
		completed: bool,
	) -> Result<(), StoreError> {
		let account_id = account_id.as_str().to_owned();
		self.run(move |connection| {
			connection
				.execute(
					"UPDATE account_quota_activation SET outcome=?3,
				 next_due_at_micros=CASE WHEN ?4 THEN next_due_at_micros ELSE ?5 END
				 WHERE account_id=?1 AND attempted_at_micros=?2 AND outcome='unknown'",
					params![
						account_id,
						attempted_at,
						if completed { "completed" } else { "rejected" },
						completed,
						attempted_at.saturating_add(REJECTION_BACKOFF_MICROS)
					],
				)
				.map_err(sqlite_error)?;
			Ok(())
		})
		.await
	}
}

#[cfg(test)]
mod tests {
	use super::{REJECTION_BACKOFF_MICROS, SqliteStore, sqlite_error};
	use decodex_core::{AccountId, AccountQuotaWindow};
	fn quota(used: u8, reset: i64) -> AccountQuotaWindow {
		AccountQuotaWindow::new(10_080, used, reset).expect("quota")
	}
	async fn fixture() -> (tempfile::TempDir, SqliteStore, AccountId) {
		let dir = tempfile::tempdir().expect("directory");
		let store = SqliteStore::open_test(&dir.path().join("state.sqlite3")).expect("store");
		let id = AccountId::new("10000000-0000-4000-8000-000000000001").expect("id");
		let key = id.as_str().to_owned();
		store.run(move |conn| {
   conn.execute("INSERT INTO account_identities VALUES (?1,1)",[&key]).map_err(sqlite_error)?;
   conn.execute("INSERT INTO accounts(account_id,display_label,enabled,state,revision,provider,provider_account_id,created_at_micros,updated_at_micros) VALUES (?1,'test',1,'available',1,'chatgpt','provider-test',1,1)",[&key]).map_err(sqlite_error)?;
   Ok(())
  }).await.expect("account");
		(dir, store, id)
	}
	#[tokio::test]
	async fn floating_reset_activates_once_and_rearms_only_after_countdown_starts() {
		let (dir, store, id) = fixture().await;
		let week = 604_800_000_000;
		let start = 1_790_000_000_000_000;
		for seconds in [0, 5, 15] {
			let now = start + seconds * 1_000_000;
			assert!(
				!store
					.claim_quota_activation(&id, 1, quota(0, now + week), true, now)
					.await
					.unwrap()
			);
		}
		drop(store);
		let store = SqliteStore::open_test(&dir.path().join("state.sqlite3")).unwrap();
		let now = start + 30_000_000;
		assert!(
			!store.claim_quota_activation(&id, 1, quota(0, now + week), false, now).await.unwrap()
		);
		assert!(
			store.claim_quota_activation(&id, 1, quota(0, now + week), true, now).await.unwrap()
		);
		for seconds in [35, 120, 3600] {
			let later = start + seconds * 1_000_000;
			assert!(
				!store
					.claim_quota_activation(&id, 1, quota(0, later + week), true, later)
					.await
					.unwrap()
			);
		}
		// A positive receipt alone does not permit another request for the drifting window.
		store.finish_quota_activation(&id, now, true).await.unwrap();
		let later = start + 7_200_000_000;
		assert!(
			!store
				.claim_quota_activation(&id, 1, quota(0, later + week), true, later)
				.await
				.unwrap()
		);
		// The provider eventually starts a real countdown, even if usage still rounds to zero.
		let reset = later + week;
		assert!(
			!store
				.claim_quota_activation(&id, 1, quota(0, reset), true, later + 60_000_000)
				.await
				.unwrap()
		);
		assert!(store.claim_quota_activation(&id, 1, quota(0, reset), true, reset).await.unwrap());
	}

	#[tokio::test]
	async fn upgraded_idle_window_can_activate_but_existing_reservation_cannot_replay() {
		for reserved in [false, true] {
			let (_dir, store, id) = fixture().await;
			let start = 1_790_000_000_000_000;
			let week = 604_800_000_000;
			let key = id.as_str().to_owned();
			store.run(move |conn| {
				conn.execute("INSERT INTO account_quota_activation(account_id,observed_reset_at_micros,next_due_at_micros,outcome) VALUES(?1,?2,?3,?4)", rusqlite::params![key, start + week - 60_000_000, if reserved { i64::MAX } else { start + week - 60_000_000 }, if reserved { "unknown" } else { "idle" }]).map_err(sqlite_error)?;
				Ok(())
			}).await.unwrap();
			assert!(
				!store
					.claim_quota_activation(&id, 1, quota(0, start + week), true, start)
					.await
					.unwrap()
			);
			let later = start + 30_000_000;
			assert_eq!(
				store
					.claim_quota_activation(&id, 1, quota(0, later + week), true, later)
					.await
					.unwrap(),
				!reserved
			);
		}
	}

	#[tokio::test]
	async fn fixed_future_reset_is_not_mistaken_for_an_unstarted_window() {
		let (_dir, store, id) = fixture().await;
		let start = 1_790_000_000_000_000;
		let reset = start + 604_800_000_000;
		for seconds in [0, 5, 15, 30, 60, 3600] {
			let now = start + seconds * 1_000_000;
			assert!(
				!store.claim_quota_activation(&id, 1, quota(0, reset), true, now).await.unwrap()
			);
		}
	}

	#[tokio::test]
	async fn floating_reset_rejection_keeps_backoff_despite_timestamp_drift() {
		let (_dir, store, id) = fixture().await;
		let start = 1_790_000_000_000_000;
		let week = 604_800_000_000;
		assert!(
			!store
				.claim_quota_activation(&id, 1, quota(0, start + week), true, start)
				.await
				.unwrap()
		);
		let attempt = start + 30_000_000;
		assert!(
			store
				.claim_quota_activation(&id, 1, quota(0, attempt + week), true, attempt)
				.await
				.unwrap()
		);
		store.finish_quota_activation(&id, attempt, false).await.unwrap();
		let early = attempt + 60_000_000;
		assert!(
			!store
				.claim_quota_activation(&id, 1, quota(0, early + week), true, early)
				.await
				.unwrap()
		);
		let retry = attempt + super::REJECTION_BACKOFF_MICROS;
		assert!(
			store
				.claim_quota_activation(&id, 1, quota(0, retry + week), true, retry)
				.await
				.unwrap()
		);
	}

	#[tokio::test]
	async fn reset_time_drives_activation_regardless_of_rounded_percentage() {
		for used in [0, 1, 100] {
			let (_dir, store, id) = fixture().await;
			assert!(
				!store
					.claim_quota_activation(&id, 1, quota(used, 100), true, 99)
					.await
					.expect("future")
			);
			assert!(
				store
					.claim_quota_activation(&id, 1, quota(used, 100), true, 100)
					.await
					.expect("expired")
			);
			store.finish_quota_activation(&id, 100, true).await.expect("completed");
			assert!(
				!store
					.claim_quota_activation(&id, 1, quota(0, 100), true, 900_000_000_000)
					.await
					.expect("no replay")
			);
			assert!(
				!store
					.claim_quota_activation(&id, 1, quota(0, 200), true, 101)
					.await
					.expect("advanced countdown")
			);
			assert!(
				!store
					.claim_quota_activation(&id, 1, quota(0, 100), true, 201)
					.await
					.expect("stale reset")
			);
			assert!(
				store
					.claim_quota_activation(&id, 1, quota(0, 200), true, 201)
					.await
					.expect("next expiry")
			);
		}
	}
	#[tokio::test]
	async fn default_enabled_reservation_survives_restart_concurrency_and_toggle() {
		let (dir, store, id) = fixture().await;
		let q = quota(0, 100);
		assert!(store.read_desktop_settings().await.expect("default").auto_activate_quota);
		store.set_desktop_settings(1, true, Some(false)).await.expect("disable");
		assert!(!store.claim_quota_activation(&id, 1, q, true, 100).await.expect("disabled"));
		store.set_desktop_settings(2, true, Some(true)).await.expect("enable");
		assert!(!store.claim_quota_activation(&id, 2, q, true, 100).await.expect("revision"));
		assert!(!store.claim_quota_activation(&id, 1, q, false, 100).await.expect("blocked"));
		let (a, b) = tokio::join!(
			store.claim_quota_activation(&id, 1, q, true, 100),
			store.claim_quota_activation(&id, 1, q, true, 100)
		);
		assert_ne!(a.expect("claim"), b.expect("claim"));
		store.set_desktop_settings(3, true, Some(false)).await.expect("disable");
		store.set_desktop_settings(4, true, Some(true)).await.expect("enable");
		drop(store);
		let store = SqliteStore::open_test(&dir.path().join("state.sqlite3")).expect("reopen");
		assert!(
			!store
				.claim_quota_activation(&id, 1, q, true, 900_000_000_000)
				.await
				.expect("unknown stays suppressed")
		);
	}
	#[tokio::test]
	async fn only_known_rejection_retries_same_expired_reset_after_backoff() {
		let (_dir, store, id) = fixture().await;
		let q = quota(0, 100);
		assert!(store.claim_quota_activation(&id, 1, q, true, 100).await.expect("claim"));
		store.finish_quota_activation(&id, 100, false).await.expect("reject");
		assert!(!store.claim_quota_activation(&id, 1, q, true, 101).await.expect("backoff"));
		assert!(
			store
				.claim_quota_activation(&id, 1, q, true, 100 + REJECTION_BACKOFF_MICROS)
				.await
				.expect("retry")
		);
	}
}
