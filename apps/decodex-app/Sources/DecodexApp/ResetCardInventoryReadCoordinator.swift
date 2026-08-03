import Foundation

/// Owns the per-account daemon-value read ordering contract.
///
/// Reset Card reads can be requested by the periodic refresh, account-control
/// follow-ups, and terminal use reconciliation. Reads return daemon-owned cached
/// values without starting provider work. An effect gate still waits for an older
/// read, blocks new reads through dispatch, and creates a fresh epoch so a value
/// started before the effect cannot publish as post-effect reconciliation.
actor ResetCardInventoryReadCoordinator {
	private struct Operation {
		let id: UInt64
		let accountRevision: UInt64
		let epoch: UInt64
		let task: Task<ResetCardInventory, Error>
	}

	private let client: any ResetCardClient
	private var epochs = [String: UInt64]()
	private var operations = [String: Operation]()
	private var effectAccountIDs = Set<String>()
	private var effectWaiters = [
		String: [CheckedContinuation<Void, Never>]
	]()
	private var nextOperationID: UInt64 = 0

	init(client: any ResetCardClient) {
		self.client = client
	}

	func invalidate(_ accountID: String) {
		epochs[accountID, default: 0] &+= 1
	}

	func beginEffect(_ accountID: String) async {
		while effectAccountIDs.contains(accountID) {
			await waitForEffect(accountID)
		}
		effectAccountIDs.insert(accountID)
		invalidate(accountID)
		if let predecessor = operations[accountID]?.task {
			_ = try? await predecessor.value
		}
	}

	func endEffect(_ accountID: String) {
		guard effectAccountIDs.remove(accountID) != nil else {
			return
		}
		resumeEffectWaiters(accountID)
	}

	func inventory(
		for account: ResetCardAccountRecord
	) async throws -> ResetCardInventory {
		let accountID = account.accountID
		while effectAccountIDs.contains(accountID) {
			await waitForEffect(accountID)
		}
		let epoch = epochs[accountID, default: 0]
		if let operation = operations[accountID],
			operation.accountRevision == account.accountRevision,
			operation.epoch == epoch
		{
			return try await operation.task.value
		}

		let predecessor = operations[accountID]?.task
		nextOperationID &+= 1
		let operationID = nextOperationID
		let client = self.client
		let task = Task { () throws -> ResetCardInventory in
			if let predecessor {
				_ = try? await predecessor.value
			}
			try Task.checkCancellation()
			return try await client.inventory(for: account)
		}
		operations[accountID] = Operation(
			id: operationID,
			accountRevision: account.accountRevision,
			epoch: epoch,
			task: task
		)

		do {
			let inventory = try await task.value
			finish(accountID: accountID, operationID: operationID)
			return inventory
		} catch {
			finish(accountID: accountID, operationID: operationID)
			throw error
		}
	}

	func discard(_ accountID: String) {
		operations.removeValue(forKey: accountID)?.task.cancel()
		epochs.removeValue(forKey: accountID)
		effectAccountIDs.remove(accountID)
		resumeEffectWaiters(accountID)
	}

	func cancelAll() {
		for operation in operations.values {
			operation.task.cancel()
		}
		operations.removeAll()
		epochs.removeAll()
		effectAccountIDs.removeAll()
		let waiters = Array(effectWaiters.values.joined())
		effectWaiters.removeAll()
		for waiter in waiters {
			waiter.resume()
		}
	}

	private func waitForEffect(_ accountID: String) async {
		await withCheckedContinuation { continuation in
			effectWaiters[accountID, default: []].append(continuation)
		}
	}

	private func resumeEffectWaiters(_ accountID: String) {
		let waiters = effectWaiters.removeValue(forKey: accountID) ?? []
		for waiter in waiters {
			waiter.resume()
		}
	}

	private func finish(accountID: String, operationID: UInt64) {
		guard operations[accountID]?.id == operationID else {
			return
		}
		operations.removeValue(forKey: accountID)
	}
}
