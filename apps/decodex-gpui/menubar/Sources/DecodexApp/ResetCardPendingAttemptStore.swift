import Foundation

enum ResetCardPendingAttemptLoad: Equatable {
	case available([ResetCardUseAttempt])
	case recoveryBlocked([ResetCardUseAttempt])

	var attempts: [ResetCardUseAttempt] {
		switch self {
		case let .available(attempts), let .recoveryBlocked(attempts):
			return attempts
		}
	}

	var isRecoveryBlocked: Bool {
		if case .recoveryBlocked = self {
			return true
		}

		return false
	}
}

enum ResetCardPendingDispatchJournalUpdate: Equatable {
	case retained
	case removed
	case removalFailed
}

struct ResetCardPendingDispatchResult<Value> {
	let value: Value
	let journalUpdate: ResetCardPendingDispatchJournalUpdate
}

@MainActor
struct ResetCardPendingAttemptStore {
	static let maximumAttempts = 64
	let journalURL: URL
	private let nativeRequest: (Data) throws -> Data

	init(
		nativeRequest: @escaping (Data) throws -> Data = DecodexNativeClient.resetCardJournal,
		journalURL: URL = ResetCardPendingAttemptStore.defaultJournalURL()
	) {
		self.nativeRequest = nativeRequest
		self.journalURL = journalURL
	}

	func load() -> ResetCardPendingAttemptLoad {
		guard let result: Loaded = perform(Request(operation: "load", path: journalURL.path)) else {
			return .recoveryBlocked([])
		}
		return result.blocked ? .recoveryBlocked(result.attempts) : .available(result.attempts)
	}

	func insert(_ attempt: ResetCardUseAttempt) -> [ResetCardUseAttempt]? {
		let result: Attempts? = perform(Request(operation: "insert", path: journalURL.path, attempt: attempt))
		return result?.attempts
	}

	func remove(_ attempt: ResetCardUseAttempt) -> [ResetCardUseAttempt]? {
		let result: Attempts? = perform(Request(operation: "remove", path: journalURL.path, attempt: attempt))
		return result?.attempts
	}

	func withDispatchLock<Value>(
		for attempt: ResetCardUseAttempt,
		operation: () async -> Value,
		shouldRemove: (Value) -> Bool
	) async -> ResetCardPendingDispatchResult<Value>? {
		guard let lease: Lease = perform(Request(operation: "begin_dispatch", path: journalURL.path, attempt: attempt)) else {
			return nil
		}
		// Rust retains the journal lock across the async request. Completion only
		// retires this exact saved identity; cancellation never starts another send.
		let value = await operation()
		let remove = shouldRemove(value)
		let result: Finished? = perform(Request(operation: "finish_dispatch", lease: lease.lease, remove: remove))
		return ResetCardPendingDispatchResult(
			value: value,
			journalUpdate: remove ? (result?.removed == true ? .removed : .removalFailed) : .retained
		)
	}

	private func perform<T: Decodable>(_ request: Request) -> T? {
		guard let data = try? JSONEncoder().encode(request),
			let response = try? nativeRequest(data) else { return nil }
		return try? JSONDecoder().decode(T.self, from: response)
	}

	private struct Request: Encodable {
		let operation: String
		var path: String? = nil
		var attempt: ResetCardUseAttempt? = nil
		var lease: UInt64? = nil
		var remove: Bool? = nil
	}
	private struct Loaded: Decodable {
		let blocked: Bool
		let attempts: [ResetCardUseAttempt]
	}
	private struct Attempts: Decodable { let attempts: [ResetCardUseAttempt] }
	private struct Lease: Decodable { let lease: UInt64 }
	private struct Finished: Decodable { let removed: Bool }

	private static func defaultJournalURL() -> URL {
		let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first
			?? FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("Library/Application Support", isDirectory: true)
		return base.appendingPathComponent("Decodex", isDirectory: true)
			.appendingPathComponent("reset-card-pending-v1.json", isDirectory: false)
	}
}
