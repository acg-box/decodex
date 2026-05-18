import Foundation

@MainActor
final class AccountStore: ObservableObject {
	@Published private(set) var accountList: AccountListResponse?
	@Published private(set) var isRefreshing = false
	@Published private(set) var isLoggingIn = false
	@Published var loginTranscript = ""
	@Published var notice: String?

	private let bridge = DecodexAppBridge()

	var isInitialLoading: Bool {
		accountList == nil && isRefreshing
	}

	var accounts: [CodexAccount] {
		accountList?.accounts ?? []
	}

	var modeLabel: String {
		guard let control = accountList?.control else {
			return "Not loaded"
		}

		let codexLabel = accountList?.codexAuth?.displayName ?? "no Codex auth"
		if let selector = control.accountSelector, !selector.isEmpty {
			return "Codex: \(codexLabel) / Decodex: \(selector)"
		}

		let decodexLabel = control.mode == "balanced" ? "balanced" : control.mode
		return "Codex: \(codexLabel) / Decodex: \(decodexLabel)"
	}

	var menuSymbol: String {
		if accounts.contains(where: \.codexActive) {
			return "bolt.circle.fill"
		}
		if accounts.contains(where: \.selected) {
			return "person.crop.circle.badge.checkmark"
		}
		if accounts.isEmpty {
			return "person.crop.circle.badge.plus"
		}

		return "person.2.circle"
	}

	func refresh() async {
		guard !isRefreshing else {
			return
		}

		isRefreshing = true
		defer {
			isRefreshing = false
		}

		do {
			accountList = try await bridge.runJSON(.accountList, as: AccountListResponse.self)
			notice = nil
		} catch {
			notice = error.localizedDescription
		}
	}

	func refreshIfNeeded() async {
		guard accountList == nil else {
			return
		}

		await refresh()
	}

	func useInCodex(_ account: CodexAccount) async {
		do {
			_ = try await bridge.runJSON(
				.accountUse(selector: account.selector),
				as: CodexAuthUseResponse.self
			)
			await refresh()
		} catch {
			notice = error.localizedDescription
		}
	}

	func select(_ account: CodexAccount) async {
		do {
			if account.selected {
				accountList = try await bridge.runJSON(.accountClear, as: AccountListResponse.self)
			} else {
				accountList = try await bridge.runJSON(
					.accountSelect(selector: account.selector),
					as: AccountListResponse.self
				)
			}
			notice = nil
		} catch {
			notice = error.localizedDescription
		}
	}

	func clearSelection() async {
		do {
			accountList = try await bridge.runJSON(.accountClear, as: AccountListResponse.self)
			notice = nil
		} catch {
			notice = error.localizedDescription
		}
	}

	func logout(_ account: CodexAccount) async {
		do {
			accountList = try await bridge.runJSON(
				.accountLogout(selector: account.selector),
				as: AccountListResponse.self
			)
			notice = nil
		} catch {
			notice = error.localizedDescription
		}
	}

	func login() async {
		isLoggingIn = true
		loginTranscript = ""
		notice = nil

		do {
			accountList = try await bridge.runStreaming(.accountLogin(), as: AccountListResponse.self) { [weak self] chunk in
				self?.loginTranscript += chunk
			}
			notice = nil
		} catch {
			notice = error.localizedDescription
		}

		isLoggingIn = false
	}
}
