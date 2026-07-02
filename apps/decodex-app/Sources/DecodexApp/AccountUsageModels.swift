import Foundation

struct AccountUsageEstimate: Decodable, Equatable {
	let windowDays: Int
	let accountCount: Int
	let accountEstimateCount: Int
	let totalCapacityPercent: Int
	let totalUsedPercent: Int
	let totalUsedOfCapacityPercent: Double
	let averageDailyUsedPercent: Double
	let averageDailyPoolPercent: Double

	enum CodingKeys: String, CodingKey {
		case windowDays = "window_days"
		case accountCount = "account_count"
		case accountEstimateCount = "account_estimate_count"
		case totalCapacityPercent = "total_capacity_percent"
		case totalUsedPercent = "total_used_percent"
		case totalUsedOfCapacityPercent = "total_used_of_capacity_percent"
		case averageDailyUsedPercent = "average_daily_used_percent"
		case averageDailyPoolPercent = "average_daily_pool_percent"
	}
}

struct AccountUsageRecord: Decodable, Identifiable, Equatable {
	let date: String
	let usedPercent: Int
	let capacityMultiplier: Int?
	let checkedAtUnixEpoch: Int

	var id: String {
		"\(date)-\(checkedAtUnixEpoch)"
	}

	enum CodingKeys: String, CodingKey {
		case date
		case usedPercent = "used_percent"
		case capacityMultiplier = "capacity_multiplier"
		case checkedAtUnixEpoch = "checked_at_unix_epoch"
	}
}

struct AccountProfileDailyUsage: Decodable, Identifiable, Equatable {
	let date: String
	let tokens: Int

	var id: String {
		date
	}
}
