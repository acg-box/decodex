import AppKit
@preconcurrency import AVFoundation
import AudioToolbox
import CoreAudio
import OSLog

/// Only the engine's serial tap calls encode. The finish flag also has a UI writer.
final class DictationPCMEncoder: @unchecked Sendable {
    private let converter: AVAudioConverter
    private let output: AVAudioFormat
    private var pending: [Int16] = []
    private var first = true
    private let lock = NSLock()
    private var finishing = false

    init(format: AVAudioFormat) throws {
        guard let output = AVAudioFormat(commonFormat: .pcmFormatFloat32, sampleRate: 24_000, channels: 1, interleaved: false),
              let converter = AVAudioConverter(from: format, to: output) else { throw CaptureError.format }
        self.output = output
        converter.primeMethod = .none
        self.converter = converter
    }

    func requestFinish() { lock.withLock { finishing = true } }

    func encode(_ input: AVAudioPCMBuffer) -> (frames: [Data], level: Float, first: Bool, final: Bool)? {
        let capacity = AVAudioFrameCount(ceil(Double(input.frameLength) * 24_000 / input.format.sampleRate) + 32)
        guard let buffer = AVAudioPCMBuffer(pcmFormat: output, frameCapacity: capacity) else { return nil }
        let final = lock.withLock { finishing }
        var supplied = false
        var energy: Float = 0
        var sampleCount = 0
        while true {
            var error: NSError?
            let status = converter.convert(to: buffer, error: &error) { _, state in
                guard !supplied else { state.pointee = final ? .endOfStream : .noDataNow; return nil }
                supplied = true
                state.pointee = .haveData
                return input
            }
            guard status != .error, error == nil, let samples = buffer.floatChannelData?[0] else { return nil }
            sampleCount += Int(buffer.frameLength)
            for index in 0..<Int(buffer.frameLength) {
                let value = samples[index].isFinite ? min(1, max(-1, samples[index])) : 0
                energy += value * value
                pending.append(Int16(value * (value < 0 ? 32768 : 32767)))
            }
            if status != .haveData || buffer.frameLength == 0 { break }
        }
        var frames: [Data] = []
        while pending.count >= 2048 || (final && !pending.isEmpty) {
            let count = min(2048, pending.count)
            let bytes = Array(pending.prefix(count)).map { $0.littleEndian }
            frames.append(bytes.withUnsafeBytes { Data($0) })
            pending.removeFirst(count)
        }
        let wasFirst = first
        first = false
        return (frames, min(1, sqrt(energy / Float(max(1, sampleCount))) * 5), wasFirst, final)
    }
}

enum CaptureError: Error { case format, device }

@MainActor
protocol DictationCapturing: AnyObject {
    func start(input: String) throws
    func finish()
    func stop()
}

/// Direct Core Audio input for dictation; speech recognition stays in the subscription.
@MainActor
final class DictationCapture: DictationCapturing {
    private static let logger = Logger(subsystem: "box.acg.decodex", category: "DictationCapture")
    private let engine = AVAudioEngine()
    private var encoder: DictationPCMEncoder?
    private var active = false
    private let emit: @MainActor @Sendable ([String: Any]) -> Void
    private let requestedAt = Date()

    init(emit: @escaping @MainActor @Sendable ([String: Any]) -> Void) { self.emit = emit }

    func start(input name: String) throws {
        let input = engine.inputNode
        if !name.isEmpty {
            var device = try Self.device(named: name)
            let status = input.withAudioUnit { unit -> OSStatus in
                guard let unit else { return kAudio_ParamError }
                return AudioUnitSetProperty(unit, kAudioOutputUnitProperty_CurrentDevice, kAudioUnitScope_Global, 0, &device, UInt32(MemoryLayout<AudioDeviceID>.size))
            }
            guard status == noErr else { throw CaptureError.device }
        }
        let format = input.outputFormat(forBus: 0)
        guard format.sampleRate > 0, format.channelCount > 0 else { throw CaptureError.format }
        let encoder = try DictationPCMEncoder(format: format)
        self.encoder = encoder
        try input.__installTap(onBus: 0, bufferSize: AVAudioFrameCount(format.sampleRate / 10), format: format, error: ()) { @Sendable [weak self, encoder] buffer, _ in
            guard let frame = encoder.encode(buffer) else {
                DispatchQueue.main.async { [weak self] in
                    guard let self, self.active else { return }
                    self.stop()
                    self.emit(["type":"error", "message":"The microphone audio format could not be converted."])
                }
                return
            }
            DispatchQueue.main.async { [weak self] in
                guard let self, self.active else { return }
                if frame.first {
                    let elapsed = Date().timeIntervalSince(self.requestedAt) * 1000
                    Self.logger.info("Microphone ready in \(elapsed, privacy: .public) ms")
                    self.emit(["type":"dictation_ready", "capture_ms":elapsed])
                }
                self.emit(["type":"level", "value":frame.level])
                for pcm in frame.frames { self.emit(["type":"pcm", "audio":pcm.base64EncodedString(), "level":frame.level]) }
                if frame.final { self.stop(); self.emit(["type":"ended"]) }
            }
        }
        active = true
        do { engine.prepare(); try engine.start() } catch { stop(); throw error }
    }

    func finish() {
        encoder?.requestFinish()
        Task { [weak self] in
            try? await Task.sleep(for: .milliseconds(200))
            guard let self, self.active else { return }
            self.stop()
            self.emit(["type":"ended"])
        }
    }

    func stop() {
        guard active else { return }
        active = false
        engine.inputNode.removeTap(onBus: 0)
        engine.stop()
        encoder = nil
    }

    private static func device(named name: String) throws -> AudioDeviceID {
        var property = AudioObjectPropertyAddress(mSelector: kAudioHardwarePropertyDevices, mScope: kAudioObjectPropertyScopeGlobal, mElement: kAudioObjectPropertyElementMain)
        var size: UInt32 = 0
        guard AudioObjectGetPropertyDataSize(AudioObjectID(kAudioObjectSystemObject), &property, 0, nil, &size) == noErr else { throw CaptureError.device }
        var devices = [AudioDeviceID](repeating: 0, count: Int(size) / MemoryLayout<AudioDeviceID>.size)
        let status = devices.withUnsafeMutableBytes { AudioObjectGetPropertyData(AudioObjectID(kAudioObjectSystemObject), &property, 0, nil, &size, $0.baseAddress!) }
        guard status == noErr else { throw CaptureError.device }
        for device in devices {
            var label: Unmanaged<CFString>?
            var labelSize = UInt32(MemoryLayout<Unmanaged<CFString>?>.size)
            var labelProperty = AudioObjectPropertyAddress(mSelector: kAudioObjectPropertyName, mScope: kAudioObjectPropertyScopeGlobal, mElement: kAudioObjectPropertyElementMain)
            guard AudioObjectGetPropertyData(device, &labelProperty, 0, nil, &labelSize, &label) == noErr, let label, label.takeRetainedValue() as String == name else { continue }
            var streams = AudioObjectPropertyAddress(mSelector: kAudioDevicePropertyStreams, mScope: kAudioDevicePropertyScopeInput, mElement: kAudioObjectPropertyElementMain)
            var streamSize: UInt32 = 0
            if AudioObjectGetPropertyDataSize(device, &streams, 0, nil, &streamSize) == noErr && streamSize > 0 { return device }
        }
        throw CaptureError.device
    }
}
