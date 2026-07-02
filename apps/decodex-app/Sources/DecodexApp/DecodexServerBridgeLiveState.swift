import Foundation

extension DecodexServerBridge {
	func hasFreshLiveCheck(_ baseURL: URL) -> Bool {
		guard liveCheckBaseURL == baseURL, let liveCheckedAt else {
			return false
		}

		return Date().timeIntervalSince(liveCheckedAt) < liveCheckFreshness
	}

	func noteLive(_ baseURL: URL) {
		serverBaseURL = baseURL
		liveCheckBaseURL = baseURL
		liveCheckedAt = Date()
	}

	func clearLive(_ baseURL: URL) {
		if serverBaseURL == baseURL {
			serverBaseURL = nil
		}
		if liveCheckBaseURL == baseURL {
			liveCheckBaseURL = nil
			liveCheckedAt = nil
		}
	}
}
