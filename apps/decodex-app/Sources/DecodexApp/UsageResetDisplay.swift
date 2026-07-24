import Foundation

struct UsageResetDisplay {
	let short: String
	let date: String
	let accessibility: String

	static func make(
		resetAtUnixEpoch: Int?,
		now: Date = Date(),
		timeZone: TimeZone = .autoupdatingCurrent
	) -> UsageResetDisplay {
		guard let seconds = resetAtUnixEpoch, seconds > 0 else {
			return UsageResetDisplay(
				short: "-",
				date: "",
				accessibility: "reset unavailable"
			)
		}

		let resetAt = Date(timeIntervalSince1970: TimeInterval(seconds))
		guard resetAt.timeIntervalSince1970.isFinite else {
			return UsageResetDisplay(
				short: "unknown",
				date: "",
				accessibility: "remaining unknown"
			)
		}

		let distanceSeconds = Int(floor(resetAt.timeIntervalSince(now)))
		if distanceSeconds <= 0 {
			let date = formatResetDate(resetAt, now: now, timeZone: timeZone)
			return UsageResetDisplay(
				short: "0m",
				date: date,
				accessibility: "reset at \(date), reset due now"
			)
		}

		let short = formatResetDuration(distanceSeconds)
		let date = formatResetDate(resetAt, now: now, timeZone: timeZone)
		return UsageResetDisplay(
			short: short,
			date: date,
			accessibility: "reset at \(date), resets in \(short)"
		)
	}

	private static func formatResetDuration(_ seconds: Int) -> String {
		let value = max(0, seconds)
		if value < 60 {
			return "<1m"
		}

		let days = value / 86_400
		let hours = (value % 86_400) / 3_600
		let minutes = (value % 3_600) / 60

		if days > 0 {
			return hours > 0 ? "\(days)d \(hours)h" : "\(days)d"
		}

		if hours > 0 {
			return "\(hours)h \(minutes)m"
		}

		return "\(minutes)m"
	}

	private static func formatResetDate(_ date: Date, now: Date, timeZone: TimeZone) -> String {
		let formatter = DateFormatter()
		formatter.locale = Locale(identifier: "en_US_POSIX")
		formatter.timeZone = timeZone
		var calendar = Calendar(identifier: .gregorian)
		calendar.timeZone = timeZone
		formatter.dateFormat = calendar.component(.year, from: date) == calendar.component(.year, from: now)
			? "MMM d HH:mm"
			: "MMM d yyyy HH:mm"
		return formatter.string(from: date)
	}
}
