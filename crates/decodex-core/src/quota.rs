//! Duration types used by routing observations.

/// The only quota-window identities accepted by the vNext policy.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum QuotaWindowClass {
	/// A rolling five-hour window.
	FiveHour,
	/// A rolling seven-day window.
	SevenDay,
}
impl QuotaWindowClass {
	/// Resolve a window identity from its duration rather than a source-field position.
	pub const fn from_duration_minutes(
		duration_minutes: u32,
	) -> Result<Self, UnknownWindowDuration> {
		match duration_minutes {
			300 => Ok(Self::FiveHour),
			10_080 => Ok(Self::SevenDay),
			_ => Err(UnknownWindowDuration),
		}
	}

	/// Return the canonical duration that defines this window identity.
	pub const fn duration_minutes(self) -> u32 {
		match self {
			Self::FiveHour => 300,
			Self::SevenDay => 10_080,
		}
	}
}

/// A duration that is not one of the two closed quota-window classes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnknownWindowDuration;

#[cfg(test)]
mod tests {
	use super::{QuotaWindowClass, UnknownWindowDuration};
	#[test]
	fn duration_is_the_only_window_identity() {
		assert_eq!(QuotaWindowClass::from_duration_minutes(300), Ok(QuotaWindowClass::FiveHour));
		assert_eq!(QuotaWindowClass::from_duration_minutes(10_080), Ok(QuotaWindowClass::SevenDay));

		for duration in [0, 299, 301, 10_079, 10_081, u32::MAX] {
			assert_eq!(
				QuotaWindowClass::from_duration_minutes(duration),
				Err(UnknownWindowDuration)
			);
		}

		assert_eq!(QuotaWindowClass::FiveHour.duration_minutes(), 300);
		assert_eq!(QuotaWindowClass::SevenDay.duration_minutes(), 10_080);
	}
}
