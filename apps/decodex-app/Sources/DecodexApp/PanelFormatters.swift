import Foundation

func formatUsagePercent(_ value: Double) -> String {
	guard value.isFinite else {
		return "-"
	}

	let rounded = value.rounded()
	if abs(value - rounded) < 0.05 {
		return "\(Int(rounded))%"
	}

	return String(format: "%.1f%%", value)
}

func formatDailyUsageRate(_ value: Double) -> String {
	let percent = formatUsagePercent(value)
	guard percent != "-" else {
		return "-"
	}

	return "\(percent)/d"
}

func compactUsageDate(_ value: String) -> String {
	let formatter = DateFormatter()
	formatter.locale = Locale(identifier: "en_US_POSIX")
	formatter.dateFormat = "yyyy-MM-dd"
	guard let date = formatter.date(from: value) else {
		return value
	}

	formatter.dateFormat = "MMM d"
	return formatter.string(from: date)
}

func formatPercentagePointDelta(_ value: Double) -> String {
	guard value.isFinite else {
		return "-"
	}

	let absValue = abs(value)
	let sign = value > 0.05 ? "+" : (value < -0.05 ? "-" : "")
	let rounded = absValue.rounded()
	if abs(absValue - rounded) < 0.05 {
		return "\(sign)\(Int(rounded))pp"
	}

	return String(format: "%@%.1fpp", sign, absValue)
}

func formatActivityDuration(_ seconds: Int?) -> String? {
	guard let seconds else {
		return nil
	}

	let value = max(0, seconds)
	if value < 60 {
		return "\(value)s"
	}

	let hours = value / 3_600
	let minutes = (value % 3_600) / 60
	let remainderSeconds = value % 60
	if hours > 0 {
		return minutes > 0 ? "\(hours)h \(minutes)m" : "\(hours)h"
	}
	if minutes > 0 {
		return remainderSeconds > 0 ? "\(minutes)m \(remainderSeconds)s" : "\(minutes)m"
	}

	return "\(remainderSeconds)s"
}

func formatCompactCount(_ value: Int) -> String {
	let absoluteValue = abs(Double(value))
	let sign = value < 0 ? "-" : ""

	if absoluteValue >= 1_000_000_000 {
		return "\(sign)\(formatCompactDecimal(absoluteValue / 1_000_000_000))B"
	}
	if absoluteValue >= 1_000_000 {
		return "\(sign)\(formatCompactDecimal(absoluteValue / 1_000_000))M"
	}
	if absoluteValue >= 1_000 {
		return "\(sign)\(formatCompactDecimal(absoluteValue / 1_000))K"
	}

	return "\(value)"
}

func formatCompactBytes(_ value: Int) -> String {
	let absoluteValue = max(0, Double(value))
	if absoluteValue >= 1_073_741_824 {
		return "\(formatCompactDecimal(absoluteValue / 1_073_741_824))GiB"
	}
	if absoluteValue >= 1_048_576 {
		return "\(formatCompactDecimal(absoluteValue / 1_048_576))MiB"
	}
	if absoluteValue >= 1_024 {
		return "\(formatCompactDecimal(absoluteValue / 1_024))KiB"
	}

	return "\(max(0, value))B"
}

private func formatCompactDecimal(_ value: Double) -> String {
	let rounded = (value * 10).rounded() / 10
	if rounded >= 10 || rounded.rounded() == rounded {
		return String(format: "%.0f", rounded)
	}

	return String(format: "%.1f", rounded)
}
