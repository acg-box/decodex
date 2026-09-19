import XCTest
@testable import DecodexTransport
final class DictationTranscriptTests: XCTestCase {
    func testFinalReplacesProvisionalAndRejectsLateRevisions() {
        var text = DictationTranscript()
        XCTAssertTrue(text.apply(id:"a",revision:1,text:" This is",final:false))
        XCTAssertTrue(text.apply(id:"a",revision:2,text:" This is live",final:false))
        XCTAssertTrue(text.apply(id:"b",revision:1,text:"第二句话。",final:true))
        XCTAssertTrue(text.apply(id:"a",revision:3,text:"This is live.",final:true))
        XCTAssertFalse(text.apply(id:"a",revision:2,text:"stale",final:false))
        XCTAssertFalse(text.apply(id:"a",revision:4,text:"late",final:false))
        XCTAssertEqual(text.text,"This is live. 第二句话。")
    }
}
