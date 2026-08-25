import Foundation
import XCTest
@testable import DecodexApp

final class ResetCardInventoryReadCoordinatorTests: XCTestCase {
	func testSameRevisionReadsCoalesceIntoOneProviderCall() async throws {
		let account = Self.account
		let client = CoordinatedInventoryClient(account: account)
		let coordinator = ResetCardInventoryReadCoordinator(client: client)
		let first = Task.detached {
			try await coordinator.inventory(for: account)
		}
		try await waitForCallCount(1, client: client)
		let second = Task.detached {
			try await coordinator.inventory(for: account)
		}

		try await Task.sleep(for: .milliseconds(20))
		let coalescedCallCount = await client.callCount()
		XCTAssertEqual(coalescedCallCount, 1)
		await client.release(call: 1)

		let firstInventory = try await first.value
		let secondInventory = try await second.value
		let finalCallCount = await client.callCount()
		let maximumActiveCallCount = await client.maximumActiveCallCount()
		XCTAssertEqual(firstInventory, secondInventory)
		XCTAssertEqual(finalCallCount, 1)
		XCTAssertEqual(maximumActiveCallCount, 1)
	}

	func testInvalidatedReadWaitsForThePreEffectReadToFinish() async throws {
		let account = Self.account
		let client = CoordinatedInventoryClient(account: account)
		let coordinator = ResetCardInventoryReadCoordinator(client: client)
		let preEffect = Task.detached {
			try await coordinator.inventory(for: account)
		}
		try await waitForCallCount(1, client: client)

		await coordinator.invalidate(account.accountID)
		let postEffect = Task.detached {
			try await coordinator.inventory(for: account)
		}
		try await Task.sleep(for: .milliseconds(20))
		let callCountBeforeRelease = await client.callCount()
		XCTAssertEqual(
			callCountBeforeRelease,
			1,
			"A post-effect read must queue behind the older daemon-value read."
		)

		await client.release(call: 1)
		try await waitForCallCount(2, client: client)
		let maximumActiveCallCount = await client.maximumActiveCallCount()
		XCTAssertEqual(maximumActiveCallCount, 1)
		await client.release(call: 2)

		_ = try await preEffect.value
		_ = try await postEffect.value
		let finalCallCount = await client.callCount()
		let finalMaximumActiveCallCount = await client.maximumActiveCallCount()
		XCTAssertEqual(finalCallCount, 2)
		XCTAssertEqual(finalMaximumActiveCallCount, 1)
	}

	func testEffectBlocksFreshReadsUntilDispatchEnds() async throws {
		let account = Self.account
		let client = CoordinatedInventoryClient(account: account)
		let coordinator = ResetCardInventoryReadCoordinator(client: client)
		let preEffect = Task.detached {
			try await coordinator.inventory(for: account)
		}
		try await waitForCallCount(1, client: client)

		let effect = Task.detached {
			await coordinator.beginEffect(account.accountID)
		}
		try await Task.sleep(for: .milliseconds(20))
		await client.release(call: 1)
		await effect.value

		let postEffect = Task.detached {
			try await coordinator.inventory(for: account)
		}
		try await Task.sleep(for: .milliseconds(20))
		let callCountDuringEffect = await client.callCount()
		XCTAssertEqual(
			callCountDuringEffect,
			1,
			"A fresh read must wait until the use dispatch releases its effect gate."
		)

		await coordinator.endEffect(account.accountID)
		try await waitForCallCount(2, client: client)
		await client.release(call: 2)
		_ = try await preEffect.value
		_ = try await postEffect.value

		let finalCallCount = await client.callCount()
		let maximumActiveCallCount = await client.maximumActiveCallCount()
		XCTAssertEqual(finalCallCount, 2)
		XCTAssertEqual(maximumActiveCallCount, 1)
	}

	private func waitForCallCount(
		_ expected: Int,
		client: CoordinatedInventoryClient
	) async throws {
		for _ in 0 ..< 200 {
			if await client.callCount() >= expected {
				return
			}
			try await Task.sleep(for: .milliseconds(5))
		}
		await client.releaseAll()
		XCTFail("Timed out waiting for provider call \(expected).")
		throw CoordinatorTestError.timedOut
	}

	private static let authority = ResetCardAuthority(
		profileName: "local",
		serverID: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"
	)

	private static let account = ResetCardAccountRecord(
		authority: authority,
		accountID: "018f0f9e-7b6e-4a31-8f4c-1d2e3f405160",
		alias: "Account 00000-00001",
		accountRevision: 7,
		enabled: true,
		observedState: .available,
		lifecycleReadiness: .ready,
		fiveHourQuota: .unknown(durationMinutes: 300),
		sevenDayQuota: .unknown(durationMinutes: 10_080)
	)
}

private enum CoordinatorTestError: Error {
	case timedOut
}

private actor CoordinatedInventoryClient: ResetCardClient {
	private let account: ResetCardAccountRecord
	private var calls = 0
	private var activeCalls = 0
	private var maximumActiveCalls = 0
	private var pendingCalls = [Int: CheckedContinuation<Void, Never>]()

	init(account: ResetCardAccountRecord) {
		self.account = account
	}

	func accounts(
		authority: ResetCardAuthority?
	) async throws -> [ResetCardAccountRecord] {
		[account]
	}

	func inventory(
		for account: ResetCardAccountRecord
	) async throws -> ResetCardInventory {
		calls += 1
		let call = calls
		activeCalls += 1
		maximumActiveCalls = max(maximumActiveCalls, activeCalls)
		await withCheckedContinuation { continuation in
			pendingCalls[call] = continuation
		}
		activeCalls -= 1
		return ResetCardInventory(
			authority: Self.authority(for: account),
			accountID: account.accountID,
			accountRevision: account.accountRevision,
			cards: [],
			fiveHourQuota: account.fiveHourQuota,
			sevenDayQuota: account.sevenDayQuota,
			observationError: nil
		)
	}

	func use(_ attempt: ResetCardUseAttempt) async throws -> ResetCardOperationState {
		.prepared
	}

	func status(for attempt: ResetCardUseAttempt) async throws -> ResetCardOperationState {
		.notFound
	}

	func callCount() -> Int {
		calls
	}

	func maximumActiveCallCount() -> Int {
		maximumActiveCalls
	}

	func release(call: Int) {
		pendingCalls.removeValue(forKey: call)?.resume()
	}

	func releaseAll() {
		let continuations = pendingCalls.values
		pendingCalls.removeAll()
		for continuation in continuations {
			continuation.resume()
		}
	}

	nonisolated private static func authority(
		for account: ResetCardAccountRecord
	) -> ResetCardAuthority {
		account.authority
			?? ResetCardAuthority(
				profileName: "local",
				serverID: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"
			)
	}
}
