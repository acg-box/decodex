@testable import DecodexApp
import XCTest

final class AccountStoreAccountListTests: XCTestCase {
	@MainActor
	func testOptimisticLogoutRemovalMasksStaleAccountListsUntilBackendCatchesUp() {
		let removedAccount = makeAccount(
			status: "available",
			email: "remove@example.com",
			accountFingerprint: "fp-remove"
		)
		let keptAccount = makeAccount(
			status: "available",
			email: "keep@example.com",
			accountFingerprint: "fp-keep"
		)
		let store = AccountStore()

		store.applyAccountList(makeAccountList([removedAccount, keptAccount]))
		XCTAssertEqual(store.accounts.map(\.id), [removedAccount.id, keptAccount.id])

		store.beginOptimisticLogoutRemoval(removedAccount)
		XCTAssertEqual(store.accounts.map(\.id), [keptAccount.id])

		store.applyAccountList(makeAccountList([removedAccount, keptAccount]))
		XCTAssertEqual(store.accounts.map(\.id), [keptAccount.id])

		store.applyAccountList(makeAccountList([keptAccount]))
		XCTAssertEqual(store.accounts.map(\.id), [keptAccount.id])

		store.applyAccountList(makeAccountList([removedAccount, keptAccount]))
		XCTAssertEqual(store.accounts.map(\.id), [removedAccount.id, keptAccount.id])
	}
}
