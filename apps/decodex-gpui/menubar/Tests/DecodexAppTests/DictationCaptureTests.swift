import XCTest
import AVFoundation
@testable import DecodexApp

@MainActor
final class DictationCaptureTests: XCTestCase {
    func testStartReturnsWhileAudioQueueIsBusyAndReportsFailureAsynchronously() async throws {
        let queue = DispatchQueue(label: "dictation-start-test")
        queue.suspend()
        let failed = expectation(description: "Invalid device reported asynchronously")
        let capture = DictationCapture(queue: queue) { event in
            if event["type"] as? String == "error" { failed.fulfill() }
        }
        try capture.start(input: "missing-input-" + UUID().uuidString)
        // Reaching this line while the queue is suspended proves start did not wait for audio.
        queue.resume()
        await fulfillment(of: [failed], timeout: 5)
        capture.stop()
    }

    func testCancelBeforeQueuedStartupDoesNotOpenTheEngine() async throws {
        let queue = DispatchQueue(label: "dictation-cancel-test")
        queue.suspend()
        let drained = expectation(description: "Cancelled startup drained")
        let capture = DictationCapture(queue: queue) { _ in
            XCTFail("Cancelled queued startup must not produce capture or device errors")
        }
        try capture.start(input: "missing-input-" + UUID().uuidString)
        capture.stop()
        queue.async { DispatchQueue.main.async { drained.fulfill() } }
        queue.resume()
        await fulfillment(of: [drained], timeout: 5)
    }

    func testConversionBatchesMono24kPCMAndFlushesTheTail() throws {
        let format = try XCTUnwrap(AVAudioFormat(commonFormat: .pcmFormatFloat32, sampleRate: 48_000, channels: 2, interleaved: false))
        let buffer = try XCTUnwrap(AVAudioPCMBuffer(pcmFormat: format, frameCapacity: 4_800))
        buffer.frameLength = 4_800
        for channel in 0..<2 {
            let samples = try XCTUnwrap(buffer.floatChannelData?[channel])
            for index in 0..<4_800 { samples[index] = 0.25 }
        }
        let encoder = try DictationPCMEncoder(format: format)
        let first = try XCTUnwrap(encoder.encode(buffer))
        XCTAssertTrue(first.first)
        XCTAssertFalse(first.final)
        XCTAssertEqual(first.frames.first?.count, 4_096)
        XCTAssertGreaterThan(first.level, 0)
        let pcm = try XCTUnwrap(first.frames.first)
        let samples = stride(from: 0, to: pcm.count, by: 2).map { offset in
            Int16(bitPattern: UInt16(pcm[offset]) | UInt16(pcm[offset + 1]) << 8)
        }
        XCTAssertGreaterThan(samples.map { abs(Int($0)) }.max() ?? 0, 7_000,
                             "A moving level meter must accompany non-silent encoded PCM")
        encoder.requestFinish()
        let last = try XCTUnwrap(encoder.encode(buffer))
        XCTAssertFalse(last.first)
        XCTAssertTrue(last.final)
        XCTAssertFalse(last.frames.isEmpty)
        let bytes = (first.frames + last.frames).reduce(0) { $0 + $1.count }
        XCTAssertGreaterThanOrEqual(bytes, 9_600, "The converter must flush its buffered audio")
        XCTAssertLessThanOrEqual(bytes, 9_648, "Allow at most 1 ms of resampling filter tail")
        XCTAssertTrue(last.frames.allSatisfy { !$0.isEmpty && $0.count <= 4_096 && $0.count.isMultiple(of: 2) })
        XCTAssertLessThan(try XCTUnwrap(last.frames.last).count, 4_096, "The final partial buffer must not be discarded")
    }
}
