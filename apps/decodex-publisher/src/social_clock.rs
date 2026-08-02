//! Auxiliary-owned timestamps for production social commands.

use time::{Duration, OffsetDateTime};

use crate::prelude::{Result, eyre};

const RESERVATION_LIFETIME: Duration = Duration::hours(1);
const CONTENT_CREATE_MINIMUM_UTC_WINDOW: Duration = Duration::minutes(2);

#[cfg(test)]
std::thread_local! {
	static CONTENT_CREATE_NOW: std::cell::Cell<Option<OffsetDateTime>> = const {
		std::cell::Cell::new(None)
	};
}

#[cfg(test)]
struct TestNowReset<'a> {
	clock: &'a std::cell::Cell<Option<OffsetDateTime>>,
	previous: Option<OffsetDateTime>,
}

#[cfg(test)]
impl Drop for TestNowReset<'_> {
	fn drop(&mut self) {
		self.clock.set(self.previous);
	}
}

#[derive(Clone, Debug)]
pub(crate) struct SocialClock {
	pub(crate) now: String,
	pub(crate) expires_at: String,
	pub(crate) day: String,
}

pub(crate) fn require_current_content_create_window(reservation_day: &str) -> Result<()> {
	require_content_create_window(reservation_day, content_create_now())
}

fn require_content_create_window(reservation_day: &str, now: OffsetDateTime) -> Result<()> {
	let now = now.to_offset(time::UtcOffset::UTC);
	let current_day = format!("{:04}-{:02}-{:02}", now.year(), u8::from(now.month()), now.day());
	if reservation_day != current_day {
		eyre::bail!(
			"content create is closed because reservation day {reservation_day} is not current UTC day {current_day}"
		);
	}
	let next_midnight = now
		.replace_time(time::Time::MIDNIGHT)
		.checked_add(Duration::days(1))
		.ok_or_else(|| eyre::eyre!("content-create UTC boundary overflowed"))?;
	if next_midnight - now <= CONTENT_CREATE_MINIMUM_UTC_WINDOW {
		eyre::bail!("content create is closed during the final two minutes of the UTC day");
	}
	Ok(())
}

fn content_create_now() -> OffsetDateTime {
	#[cfg(test)]
	if let Some(now) = CONTENT_CREATE_NOW.with(std::cell::Cell::get) {
		return now;
	}
	OffsetDateTime::now_utc()
}

#[cfg(test)]
pub(crate) fn with_content_create_now_for_test<T>(
	now: OffsetDateTime,
	action: impl FnOnce() -> T,
) -> T {
	CONTENT_CREATE_NOW.with(|clock| {
		let _reset = TestNowReset { clock, previous: clock.replace(Some(now)) };
		action()
	})
}

#[cfg(test)]
pub(crate) fn with_default_content_create_now_for_test<T>(
	now: OffsetDateTime,
	action: impl FnOnce() -> T,
) -> T {
	if CONTENT_CREATE_NOW.with(|clock| clock.get().is_some()) {
		action()
	} else {
		with_content_create_now_for_test(now, action)
	}
}

impl SocialClock {
	pub(crate) fn current() -> Result<Self> {
		Self::from_now(OffsetDateTime::now_utc())
	}

	fn from_now(now: OffsetDateTime) -> Result<Self> {
		let expires_at = now
			.checked_add(RESERVATION_LIFETIME)
			.ok_or_else(|| eyre::eyre!("reservation expiry overflowed"))?;

		Ok(Self {
			now: rfc3339_seconds(now),
			expires_at: rfc3339_seconds(expires_at),
			day: format!("{:04}-{:02}-{:02}", now.year(), u8::from(now.month()), now.day()),
		})
	}
}

fn rfc3339_seconds(value: OffsetDateTime) -> String {
	format!(
		"{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
		value.year(),
		u8::from(value.month()),
		value.day(),
		value.hour(),
		value.minute(),
		value.second()
	)
}

#[cfg(test)]
mod tests {
	use time::{OffsetDateTime, format_description::well_known::Rfc3339};

	use super::SocialClock;

	#[test]
	fn derives_day_and_expiry_from_one_instant() {
		let now = OffsetDateTime::parse("2026-07-27T23:59:59Z", &Rfc3339).expect("fixed timestamp");
		let clock = SocialClock::from_now(now).expect("clock");

		assert_eq!(clock.now, "2026-07-27T23:59:59Z");
		assert_eq!(clock.expires_at, "2026-07-28T00:59:59Z");
		assert_eq!(clock.day, "2026-07-27");
	}

	#[test]
	fn content_create_window_is_bounded_by_the_reserved_utc_day() {
		let before = OffsetDateTime::parse("2026-07-27T23:57:59Z", &Rfc3339).expect("before");
		let inside = OffsetDateTime::parse("2026-07-27T23:58:00Z", &Rfc3339).expect("inside");
		let after = OffsetDateTime::parse("2026-07-28T00:00:01Z", &Rfc3339).expect("after");

		super::require_content_create_window("2026-07-27", before).expect("safe window");
		assert!(super::require_content_create_window("2026-07-27", inside).is_err());
		assert!(super::require_content_create_window("2026-07-27", after).is_err());
	}
}
