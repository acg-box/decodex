import Foundation

struct AccountProfileDailyUsage: Equatable, Identifiable, Sendable {
	let date: String
	let tokens: UInt64

	var id: String {
		date
	}
}

struct AccountProfileSnapshot: Equatable, Sendable {
	let lifetimeTokens: UInt64?
	let peakDailyTokens: UInt64?
	let longestTaskSeconds: UInt64?
	let currentStreakDays: UInt32?
	let longestStreakDays: UInt32?
	let dailyUsage: [AccountProfileDailyUsage]

	var hasContent: Bool {
		lifetimeTokens != nil
			|| peakDailyTokens != nil
			|| longestTaskSeconds != nil
			|| currentStreakDays != nil
			|| longestStreakDays != nil
			|| dailyUsage.isEmpty == false
	}
}

struct AccountProfileAggregate: Equatable, Sendable {
	let accountCount: Int
	let lifetimeTokens: UInt64?
	let lifetimeTokensCoverage: Int
	let peakDailyTokens: UInt64?
	let peakDailyTokensCoverage: Int
	let longestTaskSeconds: UInt64?
	let longestTaskSecondsCoverage: Int
	let currentStreakDays: UInt32?
	let currentStreakDaysCoverage: Int
	let longestStreakDays: UInt32?
	let longestStreakDaysCoverage: Int
	let dailyUsage: [AccountProfileDailyUsage]
	let dailyUsageCoverage: Int

	static func make(_ profiles: [AccountProfileSnapshot]) -> Self? {
		guard profiles.isEmpty == false else {
			return nil
		}

		var lifetimeTokens: UInt64?
		var lifetimeTokensCoverage = 0
		var longestTaskSeconds: UInt64?
		var longestTaskSecondsCoverage = 0
		var currentStreakDays: UInt32?
		var currentStreakDaysCoverage = 0
		var longestStreakDays: UInt32?
		var longestStreakDaysCoverage = 0
		var dailyUsageCoverage = 0
		var usageByDate = [String: UInt64]()

		for profile in profiles {
			if let value = profile.lifetimeTokens {
				lifetimeTokens = (lifetimeTokens ?? 0).addingWithoutOverflow(value)
				lifetimeTokensCoverage += 1
			}
			if let value = profile.longestTaskSeconds {
				longestTaskSeconds = max(longestTaskSeconds ?? 0, value)
				longestTaskSecondsCoverage += 1
			}
			if let value = profile.currentStreakDays {
				currentStreakDays = max(currentStreakDays ?? 0, value)
				currentStreakDaysCoverage += 1
			}
			if let value = profile.longestStreakDays {
				longestStreakDays = max(longestStreakDays ?? 0, value)
				longestStreakDaysCoverage += 1
			}
			if profile.dailyUsage.isEmpty == false {
				dailyUsageCoverage += 1
			}
			for record in profile.dailyUsage {
				usageByDate[record.date] = (usageByDate[record.date] ?? 0)
					.addingWithoutOverflow(record.tokens)
			}
		}

		let dailyUsage = usageByDate
			.map { date, tokens in
				AccountProfileDailyUsage(date: date, tokens: tokens)
			}
			.sorted { $0.date < $1.date }
		let peakDailyTokens = dailyUsage.map(\.tokens).max()
		let aggregate = Self(
			accountCount: profiles.count,
			lifetimeTokens: lifetimeTokens,
			lifetimeTokensCoverage: lifetimeTokensCoverage,
			peakDailyTokens: peakDailyTokens,
			peakDailyTokensCoverage: dailyUsageCoverage,
			longestTaskSeconds: longestTaskSeconds,
			longestTaskSecondsCoverage: longestTaskSecondsCoverage,
			currentStreakDays: currentStreakDays,
			currentStreakDaysCoverage: currentStreakDaysCoverage,
			longestStreakDays: longestStreakDays,
			longestStreakDaysCoverage: longestStreakDaysCoverage,
			dailyUsage: dailyUsage,
			dailyUsageCoverage: dailyUsageCoverage
		)

		return aggregate.hasContent ? aggregate : nil
	}

	var hasContent: Bool {
		lifetimeTokens != nil
			|| peakDailyTokens != nil
			|| longestTaskSeconds != nil
			|| currentStreakDays != nil
			|| longestStreakDays != nil
			|| dailyUsage.isEmpty == false
	}
}

struct AccountProfileOverviewStatus: Equatable, Sendable {
	let totalAccountCount: Int
	let profileCount: Int
	let currentProfileCount: Int
	let degradedProfileCount: Int

	var label: String {
		var parts = [String]()
		if currentProfileCount > 0 {
			parts.append("\(currentProfileCount) current")
		}
		if degradedProfileCount > 0 {
			parts.append("\(degradedProfileCount) stored")
		}
		if profileCount < totalAccountCount {
			parts.append("\(profileCount)/\(totalAccountCount)")
		}
		if parts.isEmpty {
			return "\(profileCount) profiles"
		}
		return parts.joined(separator: " · ")
	}
}

func formatCompactCount(_ value: UInt64) -> String {
	let absolute = Double(value)
	if absolute >= 1_000_000_000 {
		return "\(formatCompactDecimal(absolute / 1_000_000_000))B"
	}
	if absolute >= 1_000_000 {
		return "\(formatCompactDecimal(absolute / 1_000_000))M"
	}
	if absolute >= 1_000 {
		return "\(formatCompactDecimal(absolute / 1_000))K"
	}
	return "\(value)"
}

func formatActivityDuration(_ seconds: UInt64) -> String {
	if seconds < 60 {
		return "\(seconds)s"
	}

	let hours = seconds / 3_600
	let minutes = (seconds % 3_600) / 60
	let remainder = seconds % 60
	if hours > 0 {
		return minutes > 0 ? "\(hours)h \(minutes)m" : "\(hours)h"
	}
	return remainder > 0 ? "\(minutes)m \(remainder)s" : "\(minutes)m"
}

func compactUsageDate(_ value: String) -> String {
	let input = DateFormatter()
	input.locale = Locale(identifier: "en_US_POSIX")
	input.timeZone = TimeZone(secondsFromGMT: 0)
	input.dateFormat = "yyyy-MM-dd"
	guard let date = input.date(from: value) else {
		return value
	}

	input.dateFormat = "MMM d"
	return input.string(from: date)
}

func normalizedDailyUsage(
	_ records: [AccountProfileDailyUsage],
	maximumCount: Int = 36
) -> [AccountProfileDailyUsage] {
	let count = min(36, max(0, maximumCount))
	guard count > 0, records.isEmpty == false else {
		return []
	}

	let formatter = DateFormatter()
	formatter.calendar = Calendar(identifier: .gregorian)
	formatter.locale = Locale(identifier: "en_US_POSIX")
	formatter.timeZone = TimeZone(secondsFromGMT: 0)
	formatter.dateFormat = "yyyy-MM-dd"

	let byDate = Dictionary(
		records.map { ($0.date, $0.tokens) },
		uniquingKeysWith: { _, newest in newest }
	)
	guard let lastDate = byDate.keys
		.compactMap(formatter.date(from:))
		.max()
	else {
		return Array(records.sorted { $0.date < $1.date }.suffix(count))
	}

	var calendar = Calendar(identifier: .gregorian)
	guard let utc = TimeZone(secondsFromGMT: 0) else {
		return []
	}
	calendar.timeZone = utc
	return (0 ..< count).reversed().compactMap { offset in
		guard let date = calendar.date(byAdding: .day, value: -offset, to: lastDate) else {
			return nil
		}
		let key = formatter.string(from: date)
		return AccountProfileDailyUsage(date: key, tokens: byDate[key] ?? 0)
	}
}

private func formatCompactDecimal(_ value: Double) -> String {
	let rounded = (value * 10).rounded() / 10
	if rounded >= 10 || rounded.rounded() == rounded {
		return String(format: "%.0f", rounded)
	}
	return String(format: "%.1f", rounded)
}

extension UInt64 {
	func addingWithoutOverflow(_ value: UInt64) -> UInt64 {
		let (sum, overflow) = addingReportingOverflow(value)
		return overflow ? .max : sum
	}
}
