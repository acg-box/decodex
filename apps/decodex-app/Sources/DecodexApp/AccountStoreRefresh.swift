import Foundation

private let accountUsageRefreshIntervalNanoseconds: UInt64 = 15_000_000_000

extension AccountStore {
	func refresh(force: Bool = false) async {
		guard isRefreshing == false else {
			return
		}

		isRefreshing = true
		defer {
			isRefreshing = false
		}

		do {
			applyAccountList(try await bridge.runJSON(
				.accountList(forceRefresh: force),
				as: AccountListResponse.self
			))
			notice = nil
			await refreshFastMode()
		} catch {
			notice = error.localizedDescription
		}
	}

	func refreshIfNeeded() async {
		guard accountList == nil else {
			return
		}

		await refresh(force: true)
	}

	func startAutomaticRefresh() {
		guard automaticRefreshTask == nil else {
			return
		}

		automaticRefreshTask = Task { [weak self] in
			while Task.isCancelled == false {
				do {
					try await Task.sleep(nanoseconds: accountUsageRefreshIntervalNanoseconds)
				} catch {
					return
				}

				await self?.refresh(force: true)
			}
		}

		startOperatorSnapshotStream()
	}

	func refreshFastMode() async {
		do {
			fastMode = try await bridge.runJSON(
				.codexFastModeStatus,
				as: CodexFastModeResponse.self
			)
		} catch {
			notice = error.localizedDescription
		}
	}
}
