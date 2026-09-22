import Foundation

/// A confirmed reset owns the displayed capacity until a newer observation arrives.
struct ResetQuotaFill {
	static let duration: TimeInterval = 0.85
	let attempt: ResetCardUseAttempt
	let initial: [ResetCardQuotaWindow]
	let started: Date

	func remaining(for window: ResetCardQuotaWindow, at date: Date, reduceMotion: Bool = false) -> Double? {
		guard window.state != .notApplicable,
			let used = initial.first(where: { $0.durationMinutes == window.durationMinutes })?.usedPercent
		else { return nil }
		let elapsed = date.timeIntervalSince(started)
		if elapsed >= Self.duration,
			let observed = window.observedAtUnixMicros,
			Double(observed) > started.timeIntervalSince1970 * 1_000_000 {
			return nil
		}
		let progress = reduceMotion ? 1 : min(1, max(0, elapsed / Self.duration))
		let from = Double(100 - min(100, used))
		return from + (100 - from) * (1 - pow(1 - progress, 3))
	}
}
