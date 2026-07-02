@testable import DecodexApp
import XCTest

final class AccountDisplayModelTests: XCTestCase {
	func testCompactEmailKeepsDottedLocalSuffixesConsistent() {
		XCTAssertEqual(AccountDisplay.compactEmail("aurevoirxavier@gmail.com"), "aur...ier@gmail.com")
		XCTAssertEqual(AccountDisplay.compactEmail("aurevoirxavier.us@gmail.com"), "aur...us@gmail.com")
		XCTAssertEqual(AccountDisplay.compactEmail("aurevoirxavier.jp@gmail.com"), "aur...jp@gmail.com")
		XCTAssertEqual(AccountDisplay.compactEmail("aurevoirxavier.hk@gmail.com"), "aur...hk@gmail.com")
		XCTAssertEqual(AccountDisplay.compactEmail("xavier.lau@helixbox.ai"), "xav...lau@helixbox.ai")
	}
}
