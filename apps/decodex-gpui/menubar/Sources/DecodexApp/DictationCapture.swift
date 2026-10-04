import AppKit
@preconcurrency import AVFoundation
import AudioToolbox
import CoreAudio
import Darwin
import OSLog

/// Only the engine's serial tap calls encode. The finish flag also has a UI writer.
final class DictationPCMEncoder: @unchecked Sendable {
    private let converter: AVAudioConverter
    private let output: AVAudioFormat
    private var pending: [Int16] = []
    // Deliver 20 ms of 24 kHz PCM without waiting for an 85 ms packet.
    private static let packetSamples = 480
    private var first = true
    private let lock = NSLock()
    private var finishing = false

    init(format: AVAudioFormat) throws {
        guard let output = AVAudioFormat(commonFormat: .pcmFormatFloat32, sampleRate: 24_000, channels: 1, interleaved: false),
              let converter = AVAudioConverter(from: format, to: output) else { throw CaptureError.format }
        self.output = output
        // Voice processing can expose a discrete multichannel layout. Its first
        // channel is the microphone; implicit layout conversion can yield silence.
        converter.channelMap = [0]
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
                pending.append(Int16(value * (value < 0 ? 32_768 : 32_767)))
            }
            if status != .haveData || buffer.frameLength == 0 { break }
        }
        var frames: [Data] = []
        while pending.count >= Self.packetSamples || (final && !pending.isEmpty) {
            let count = min(Self.packetSamples, pending.count)
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

private enum DictationCaptureEvent: Sendable {
    case ready(Double), level(Float), pcm(Data, Float), ended, failed

    var payload: [String: Any] {
        switch self {
        case .ready(let elapsed): ["type": "dictation_ready", "capture_ms": elapsed]
        case .level(let value): ["type": "level", "value": value]
        case .pcm(let data, let level): ["type": "pcm", "audio": data.base64EncodedString(), "level": level]
        case .ended: ["type": "ended"]
        case .failed: ["type": "error", "message": "The microphone could not capture audio. Check the input device and try again."]
        }
    }
}

/// Main-thread adapter; engine operations run on one serial audio queue.
@MainActor
final class DictationCapture: DictationCapturing {
    private static let audioQueue = DispatchQueue(label: "box.acg.decodex.dictation-capture", qos: .userInitiated)
    private static let resources = DictationAudioResources()
    private let worker: DictationAudioWorker

    init(queue: DispatchQueue? = nil,
         emit: @escaping @MainActor @Sendable ([String: Any]) -> Void) {
        worker = DictationAudioWorker(queue: queue ?? Self.audioQueue, resources: queue == nil ? Self.resources : DictationAudioResources()) { event in
            DispatchQueue.main.async { emit(event.payload) }
        }
    }

    static func prepare(input: String) {
        let resources = Self.resources
        audioQueue.async { resources.prepare(input: input) }
    }

    func start(input: String) throws { worker.start(input: input) }
    func finish() { worker.finish() }
    func stop() { worker.stop() }

    nonisolated static func device(named name: String) throws -> AudioDeviceID {
        var property = AudioObjectPropertyAddress(mSelector: kAudioHardwarePropertyDevices, mScope: kAudioObjectPropertyScopeGlobal, mElement: kAudioObjectPropertyElementMain)
        var size: UInt32 = 0
        guard AudioObjectGetPropertyDataSize(AudioObjectID(kAudioObjectSystemObject), &property, 0, nil, &size) == noErr else { throw CaptureError.device }
        var devices = [AudioDeviceID](repeating: 0, count: Int(size) / MemoryLayout<AudioDeviceID>.size)
        let status = try devices.withUnsafeMutableBytes { buffer in
            guard let baseAddress = buffer.baseAddress else { throw CaptureError.device }
            return AudioObjectGetPropertyData(AudioObjectID(kAudioObjectSystemObject), &property, 0, nil, &size, baseAddress)
        }
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

/// All mutable engine state is confined to queue. The tap only uses its captured encoder.
/// Recognition and transcript ownership remain in the subscription runtime.
private final class DictationAudioWorker: @unchecked Sendable {
    private static let logger = Logger(subsystem: "box.acg.decodex", category: "DictationCapture")
    private var engine: AVAudioEngine?
    private let resources: DictationAudioResources
    private let queue: DispatchQueue
    private var encoder: DictationPCMEncoder?
    private var active = false
    private let cancellationLock = NSLock()
    private var cancelled = false
    private let emit: @Sendable (DictationCaptureEvent) -> Void
    private let requestedAt = Date()
    private let requestedHostTime = mach_absolute_time()

    init(queue: DispatchQueue, resources: DictationAudioResources, emit: @escaping @Sendable (DictationCaptureEvent) -> Void) {
        self.queue = queue
        self.resources = resources
        self.emit = emit
    }

    func start(input: String) {
        queue.async { [self] in
            guard !cancellationLock.withLock({ cancelled }) else { return }
            do { try startEngine(input: input) }
            catch { resources.discard(); stopEngine(); emit(.failed) }
        }
    }

    private func startEngine(input name: String) throws {
        let engine = try resources.engine(input: name)
        self.engine = engine
        let input = engine.inputNode
        let format = input.outputFormat(forBus: 0)
        guard format.sampleRate > 0, format.channelCount > 0 else { throw CaptureError.format }
        let encoder = try resources.takeEncoder(format: format)
        self.encoder = encoder
        try input.__installTap(onBus: 0, bufferSize: AVAudioFrameCount(format.sampleRate / 100), format: format, error: ()) { @Sendable [weak self, encoder] buffer, time in
            guard let frame = encoder.encode(buffer) else {
                self?.queue.async { [weak self] in
                    guard let self, self.active else { return }
                    self.resources.discard()
                    self.stopEngine()
                    self.emit(.failed)
                }
                return
            }
            let callbackHostTime = mach_absolute_time()
            let sampleHostTime = time.isHostTimeValid ? time.hostTime : nil
            let bufferMs = Double(buffer.frameLength) / buffer.format.sampleRate * 1_000
            self?.queue.async { [weak self] in
                guard let self, self.active else { return }
                if frame.first {
                    if let sampleHostTime {
                        let sampleMs = (AVAudioTime.seconds(forHostTime: sampleHostTime) - AVAudioTime.seconds(forHostTime: self.requestedHostTime)) * 1_000
                        let deliveryMs = (AVAudioTime.seconds(forHostTime: callbackHostTime) - AVAudioTime.seconds(forHostTime: sampleHostTime)) * 1_000
                        Self.logger.info("First microphone buffer: sample_ms=\(sampleMs, privacy: .public) delivery_ms=\(deliveryMs, privacy: .public) duration_ms=\(bufferMs, privacy: .public)")
                    }
                    let elapsed = Date().timeIntervalSince(self.requestedAt) * 1_000
                    Self.logger.info("Microphone ready in \(elapsed, privacy: .public) ms")
                    self.emit(.ready(elapsed))
                }
                self.emit(.level(frame.level))
                for pcm in frame.frames { self.emit(.pcm(pcm, frame.level)) }
                if frame.final { self.stopEngine(); self.emit(.ended) }
            }
        }
        active = true
        guard !cancellationLock.withLock({ cancelled }) else { stopEngine(); return }
        do {
            engine.prepare()
            let beforeStart = Date()
            try engine.start()
            let startMs = Date().timeIntervalSince(beforeStart) * 1_000
            Self.logger.info("Engine started: start_ms=\(startMs, privacy: .public) channels=\(format.channelCount, privacy: .public) sample_rate=\(format.sampleRate, privacy: .public)")
        } catch { stopEngine(); throw error }
    }

    func finish() {
        queue.async { [self] in
            encoder?.requestFinish()
            queue.asyncAfter(deadline: .now() + .milliseconds(200)) { [self] in
                guard active else { return }
                stopEngine()
                emit(.ended)
            }
        }
    }

    func stop() {
        cancellationLock.withLock { cancelled = true }
        queue.async { [self] in stopEngine() }
    }

    private func stopEngine() {
        if let engine {
            engine.pause()
            Self.logger.info("Microphone paused; capture_running=\(engine.isRunning, privacy: .public)")
        }
        if active {
            active = false
            engine?.inputNode.removeTap(onBus: 0)
        }
        encoder = nil
        engine = nil
        resources.prepareNextCapture()
    }

}

/// One stopped engine, owned exclusively by the shared audio queue. No tap or
/// hardware start occurs during preparation. Device changes invalidate reuse.
private final class DictationAudioResources: @unchecked Sendable {
    private static let logger = Logger(subsystem: "box.acg.decodex", category: "DictationCapture")
    private var prepared: PreparedDictationEngine?
    private var nextEncoder: DictationPCMEncoder?

    func takeEncoder(format: AVAudioFormat) throws -> DictationPCMEncoder {
        defer { nextEncoder = nil }
        return try nextEncoder ?? DictationPCMEncoder(format: format)
    }

    func prepareNextCapture() {
        guard let prepared, prepared.valid, !prepared.engine.isRunning else { return }
        nextEncoder = try? DictationPCMEncoder(format: prepared.engine.inputNode.outputFormat(forBus: 0))
        prepared.engine.prepare()
    }

    func prepare(input: String) {
        guard prepared?.engine.isRunning != true else { return }
        _ = try? engine(input: input)
    }

    func engine(input name: String) throws -> AVAudioEngine {
        if let prepared, prepared.input == name, prepared.valid { return prepared.engine }
        prepared = nil
        nextEncoder = nil
        let started = Date()
        let requestedDevice = name.isEmpty ? nil : try DictationCapture.device(named: name)
        let engine = AVAudioEngine()
        let input = engine.inputNode
        // Enabling voice processing replaces the I/O unit. Select the device and
        // read its format afterward so the encoder uses the processed stream.
        try input.setVoiceProcessingEnabled(true)
        input.voiceProcessingOtherAudioDuckingConfiguration = .init(
            enableAdvancedDucking: false, duckingLevel: .min
        )
        if var device = requestedDevice {
            let status = input.withAudioUnit { unit -> OSStatus in
                guard let unit else { return kAudio_ParamError }
                return AudioUnitSetProperty(unit, kAudioOutputUnitProperty_CurrentDevice, kAudioUnitScope_Global, 0, &device, UInt32(MemoryLayout<AudioDeviceID>.size))
            }
            guard status == noErr else { throw CaptureError.device }
        }
        prepared = PreparedDictationEngine(engine: engine, input: name)
        prepareNextCapture()
        let elapsed = Date().timeIntervalSince(started) * 1_000
        Self.logger.info("Audio configuration ready in \(elapsed, privacy: .public) ms; capture_running=\(engine.isRunning, privacy: .public)")
        return engine
    }

    func discard() { prepared = nil; nextEncoder = nil }
}

private final class PreparedDictationEngine: @unchecked Sendable {
    let engine: AVAudioEngine
    let input: String
    private let lock = NSLock()
    private var invalidated = false
    private var observer: NSObjectProtocol?
    var valid: Bool { lock.withLock { !invalidated } }

    init(engine: AVAudioEngine, input: String) {
        self.engine = engine
        self.input = input
        observer = NotificationCenter.default.addObserver(forName: .AVAudioEngineConfigurationChange, object: engine, queue: nil) { [weak self] _ in
            guard let self else { return }
            self.lock.withLock { self.invalidated = true }
        }
    }

    deinit { if let observer { NotificationCenter.default.removeObserver(observer) } }
}
