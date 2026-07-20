import Foundation

extension AccountStore {
	func useInCodex(_ account: CodexAccount) async {
		let previousAccountList = accountList
		notice = nil
		accountList = accountList?.updatingCodexAuth(account.authIdentity)

		do {
			let response = try await bridge.runJSON(
				.accountUse(selector: account.selector),
				as: CodexAuthUseResponse.self
			)
			accountList = accountList?.updatingCodexAuth(response.account)
			notice = nil
		} catch {
			accountList = previousAccountList
			notice = error.localizedDescription
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
			notice = nil
		} catch {
			notice = error.localizedDescription
		}
	}

	func clearSelection() async {
		do {
			applyAccountList(try await bridge.runJSON(.accountClear, as: AccountListResponse.self))
			notice = nil
		} catch {
			notice = error.localizedDescription
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
			notice = nil
		} catch {
			fastMode = previous
			notice = error.localizedDescription
		}
	}

	func logout(_ account: CodexAccount) async throws {
		beginOptimisticLogoutRemoval(account)

		do {
			applyAccountList(try await bridge.runJSON(
				.accountLogout(selector: account.selector),
				as: AccountListResponse.self
			))
			notice = nil
		} catch {
			cancelOptimisticLogoutRemoval(account)
			throw error
		}
	}

	func login() async {
		isLoggingIn = true
		loginTranscript = ""
		notice = nil

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
			notice = nil
			await refreshFastMode()
		} catch {
			notice = error.localizedDescription
		}

		isLoggingIn = false
	}
}
