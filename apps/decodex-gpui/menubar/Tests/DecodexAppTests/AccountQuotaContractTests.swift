@testable import DecodexApp
import Foundation
import XCTest

final class AccountQuotaContractTests: XCTestCase {
    func testQuotaBandsMatchDesktopContract() throws {
        var root = URL(fileURLWithPath: #filePath)
        for _ in 0..<6 { root.deleteLastPathComponent() }
        let data = try Data(contentsOf: root.appendingPathComponent("tests/fixtures/account-quota-presentation.json"))
        struct Sample: Decodable { let remaining: Double; let tone: String }
        for sample in try JSONDecoder().decode([Sample].self, from: data) {
            XCTAssertEqual(String(describing: ResetCardQuotaPresentationTone.remaining(sample.remaining)), sample.tone)
        }
    }
}
