import Foundation

extension AccountStore {
	func useInCodex(_ account: CodexAccount) async {
		let previousAccountList = accountList
		clearNotice(source: .accountAction)
		accountList = accountList?.updatingCodexAuth(account.authIdentity)

		do {
			let response = try await bridge.runJSON(
				.accountUse(selector: account.selector),
				as: CodexAuthUseResponse.self
			)
			accountList = accountList?.updatingCodexAuth(response.account)
			clearNotice(source: .accountAction)
		} catch {
			accountList = previousAccountList
			presentError("Couldn’t switch Codex account", error: error)
		}
	}

	func select(_ account: CodexAccount) async {
		do {
			if account.selected {
				applyAccountList(try await bridge.runJSON(.accountClear, as: AccountListResponse.self))
			} else {
				applyAccountList(try await bridge.runJSON(
					.accountSelect(selector: account.selector),
					as: AccountListResponse.self
				))
			}
			clearNotice(source: .accountAction)
		} catch {
			presentError("Couldn’t update account routing", error: error)
		}
	}

	func clearSelection() async {
		do {
			applyAccountList(try await bridge.runJSON(.accountClear, as: AccountListResponse.self))
			clearNotice(source: .accountAction)
		} catch {
			presentError("Couldn’t update account routing", error: error)
		}
	}

	func setFastMode(_ enabled: Bool) async {
		guard isSettingFastMode == false else {
			return
		}

		let previous = fastMode
		fastMode = CodexFastModeResponse(
			codexConfigPath: previous?.codexConfigPath ?? "",
			enabled: enabled
		)
		isSettingFastMode = true
		defer {
			isSettingFastMode = false
		}

		do {
			fastMode = try await bridge.runJSON(
				.codexFastModeSet(enabled: enabled),
				as: CodexFastModeResponse.self
			)
			clearNotice(source: .fastMode)
		} catch {
			fastMode = previous
			presentError("Couldn’t update fast mode", error: error, source: .fastMode)
		}
	}

	func logout(_ account: CodexAccount) async throws {
		beginOptimisticLogoutRemoval(account)

		do {
			applyAccountList(try await bridge.runJSON(
				.accountLogout(selector: account.selector),
				as: AccountListResponse.self
			))
			clearNotice(source: .accountAction)
		} catch {
			cancelOptimisticLogoutRemoval(account)
			throw error
		}
	}

	@discardableResult
	func login() async -> Bool {
		isLoggingIn = true
		loginTranscript = ""
		clearLoginNotice()
		defer {
			isLoggingIn = false
		}

		do {
			let codexBin = try bridge.codexExecutablePath()
			applyAccountList(
				try await bridge.runStreaming(
					.accountLogin(codexBin: codexBin),
					as: AccountListResponse.self
				) { [weak self] chunk in
					self?.loginTranscript += chunk
				}
			)
			clearLoginNotice()
			await refreshFastMode()
			return true
		} catch {
			presentError(
				"Sign-in failed",
				error: error,
				scope: .signIn,
				source: .signIn
			)
			return false
		}
	}
}
