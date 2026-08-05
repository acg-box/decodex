import Foundation

enum ResetCardRefreshWork: Equatable {
	case full
	case skeleton
}

/// Coalesces refresh intent without losing a daemon observation that arrives
/// while another account operation is active.
struct ResetCardRefreshRequests: Equatable {
	private(set) var manualRefreshRequested = false
	private(set) var observationGeneration: UInt64 = 0
	private var consumedObservationGeneration: UInt64 = 0
	private(set) var skeletonGeneration: UInt64 = 0
	private var consumedSkeletonGeneration: UInt64 = 0

	var hasFullRefreshRequest: Bool {
		manualRefreshRequested
			|| observationGeneration != consumedObservationGeneration
	}

	var hasSkeletonRefreshRequest: Bool {
		skeletonGeneration != consumedSkeletonGeneration
	}

	func didConsumeSkeletonRefresh(upTo generation: UInt64) -> Bool {
		consumedSkeletonGeneration >= generation
	}

	var hasWork: Bool {
		hasFullRefreshRequest || hasSkeletonRefreshRequest
	}

	mutating func requestManualRefresh() {
		manualRefreshRequested = true
	}

	mutating func requestObservationRefresh() {
		observationGeneration &+= 1
	}

	@discardableResult
	mutating func requestSkeletonRefresh() -> UInt64 {
		skeletonGeneration &+= 1
		return skeletonGeneration
	}

	mutating func takeNext() -> ResetCardRefreshWork? {
		if hasFullRefreshRequest {
			manualRefreshRequested = false
			consumedObservationGeneration = observationGeneration
			return .full
		}
		if hasSkeletonRefreshRequest {
			consumedSkeletonGeneration = skeletonGeneration
			return .skeleton
		}
		return nil
	}

	mutating func reset() {
		self = Self()
	}
}
