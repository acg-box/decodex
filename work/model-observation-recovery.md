# Model observation invalidation

The inherited `clear_activity_detail` function also reset model settings. The
current UI separates these panels, but disconnect and failed-snapshot paths lost
that invalidation when the coupling was removed. Cached native settings could
remain after the service became unavailable.

Reset model observations directly in `mark_stale` and the unsuccessful
`apply_result` branch. The reset increments the observation epoch, drops the
pending read, and clears cached observations. A late read cannot restore the old
result. Closing an activity detail keeps independent model observations intact.
Explicit next-message choices keep their existing owner.

A GPUI regression failed on the original disconnect path. It now checks both
unavailable-service paths, epoch invalidation, and retention when only activity
details close. This is a local state-recovery fix, not evidence of live-provider
or packaged desktop acceptance.
