//! Auxiliary-owned timestamps for production social commands.

use time::{Duration, OffsetDateTime};

use crate::prelude::{Result, eyre};

const RESERVATION_LIFETIME: Duration = Duration::hours(1);

#[derive(Clone, Debug)]
pub(crate) struct SocialClock {
	pub(crate) now: String,
	pub(crate) expires_at: String,
	pub(crate) day: String,
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
}
