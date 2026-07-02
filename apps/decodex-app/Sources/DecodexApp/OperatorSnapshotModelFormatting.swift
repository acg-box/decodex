import Foundation

func operatorTrimmed(_ value: String?) -> String? {
	value?.trimmingCharacters(in: .whitespacesAndNewlines)
}

func operatorRawDisplayToken(_ value: String) -> String {
	value.trimmingCharacters(in: .whitespacesAndNewlines)
}

func formatOperatorActivityDuration(_ seconds: Int?) -> String? {
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

func formatOperatorCompactCount(_ value: Int) -> String {
	let absoluteValue = abs(Double(value))
	let sign = value < 0 ? "-" : ""

	if absoluteValue >= 1_000_000_000 {
		return "\(sign)\(formatOperatorCompactDecimal(absoluteValue / 1_000_000_000))B"
	}
	if absoluteValue >= 1_000_000 {
		return "\(sign)\(formatOperatorCompactDecimal(absoluteValue / 1_000_000))M"
	}
	if absoluteValue >= 1_000 {
		return "\(sign)\(formatOperatorCompactDecimal(absoluteValue / 1_000))k"
	}

	return "\(value)"
}

private func formatOperatorCompactDecimal(_ value: Double) -> String {
	if value >= 100 {
		return String(format: "%.0f", value)
	}
	if value >= 10 {
		return String(format: "%.1f", value)
	}

	return String(format: "%.2f", value)
}

func formatOperatorCompactBytes(_ value: Int) -> String {
	let units = ["B", "KiB", "MiB", "GiB"]
	var amount = Double(max(0, value))
	var unitIndex = 0
	while amount >= 1_024, unitIndex < units.count - 1 {
		amount /= 1_024
		unitIndex += 1
	}

	if unitIndex == 0 {
		return "\(Int(amount))\(units[unitIndex])"
	}
	if amount >= 100 {
		return "\(Int(amount.rounded()))\(units[unitIndex])"
	}

	return String(format: "%.1f%@", amount, units[unitIndex])
}
