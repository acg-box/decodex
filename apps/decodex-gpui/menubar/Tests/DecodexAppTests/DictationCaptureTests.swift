import XCTest
import AVFoundation
@testable import DecodexApp

@MainActor
final class DictationCaptureTests: XCTestCase {
    func testConversionBatchesMono24kPCMAndFlushesTheTail() throws {
        let format = try XCTUnwrap(AVAudioFormat(commonFormat: .pcmFormatFloat32, sampleRate: 48_000, channels: 2, interleaved: false))
        let buffer = try XCTUnwrap(AVAudioPCMBuffer(pcmFormat: format, frameCapacity: 4800))
        buffer.frameLength = 4800
        for channel in 0..<2 {
            let samples = try XCTUnwrap(buffer.floatChannelData?[channel])
            for index in 0..<4800 { samples[index] = 0.25 }
        }
        let encoder = try DictationPCMEncoder(format: format)
        let first = try XCTUnwrap(encoder.encode(buffer))
        XCTAssertTrue(first.first)
        XCTAssertFalse(first.final)
        XCTAssertEqual(first.frames.first?.count, 4096)
        XCTAssertGreaterThan(first.level, 0)
        encoder.requestFinish()
        let last = try XCTUnwrap(encoder.encode(buffer))
        XCTAssertFalse(last.first)
        XCTAssertTrue(last.final)
        XCTAssertFalse(last.frames.isEmpty)
        let bytes = (first.frames + last.frames).reduce(0) { $0 + $1.count }
        XCTAssertGreaterThanOrEqual(bytes, 9600, "The converter must flush its buffered audio")
        XCTAssertLessThanOrEqual(bytes, 9648, "Allow at most 1 ms of resampling filter tail")
        XCTAssertTrue(last.frames.allSatisfy { !$0.isEmpty && $0.count <= 4096 && $0.count.isMultiple(of: 2) })
        XCTAssertLessThan(try XCTUnwrap(last.frames.last).count, 4096, "The final partial buffer must not be discarded")
    }
}
