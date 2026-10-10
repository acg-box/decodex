import AppKit
import AVFoundation

/// macOS microphone permission and dictation adapter. Rust owns live media and signaling.
@MainActor
final class VoiceMediaHost: NSObject {
    private var events: [String] = []
    private var retainedEvent: UnsafeMutablePointer<CChar>?
    private var captureIdentity: UUID?
    private var captureCancelled = false
    private var isClosed = false
    private let authorizationRequestForTesting: ((@escaping @MainActor (Bool) -> Void) -> Void)?
    private let deviceForTesting: (() throws -> AudioDeviceID)?

    init(authorizationRequestForTesting: ((@escaping @MainActor (Bool) -> Void) -> Void)? = nil,
         deviceForTesting: (() throws -> AudioDeviceID)? = nil) {
        self.authorizationRequestForTesting = authorizationRequestForTesting
        self.deviceForTesting = deviceForTesting
        super.init()
    }

    func command(_ json: String) -> Bool {
        guard !isClosed, json.utf8.count <= 65_536,
              let bytes = json.data(using: .utf8),
              let value = try? JSONSerialization.jsonObject(with: bytes) as? [String: Any],
              let operation = value["operation"] as? String else { return false }
        switch operation {
        case "prepare":
            if AVCaptureDevice.authorizationStatus(for: .audio) == .authorized {
                if let device = try? MicrophoneDevice.device(named: value["input"] as? String ?? "") {
                    emit(["type":"dictation_prepare_authorized", "device":device])
                }
            }
        case "devices":
            let discovery = AVCaptureDevice.DiscoverySession(deviceTypes: [.microphone, .external], mediaType: .audio, position: .unspecified)
            emit(["type":"devices", "inputs":discovery.devices.map { $0.localizedName }])
        case "dictate", "start":
            events.removeAll()
            captureCancelled = false
            let identity = UUID()
            captureIdentity = identity
            let input = value["input"] as? String ?? ""
            let authorized: @MainActor (Bool) -> Void = { [weak self] allowed in
                guard let self, !self.isClosed, !self.captureCancelled, self.captureIdentity == identity else { return }
                guard allowed else { self.emit(["type":"error", "message":"Allow microphone access in System Settings."]); return }
                do {
                    let device = try self.deviceForTesting?() ?? MicrophoneDevice.device(named: input)
                    self.emit(["type":operation == "dictate" ? "dictation_authorized" : "voice_authorized", "device":device])
                } catch CaptureError.ambiguous {
                    self.emit(["type":"error", "message":"More than one microphone has this name. Choose System default or another microphone."])
                } catch { self.emit(["type":"error", "message":"The selected microphone is unavailable."]) }
            }
            if let authorizationRequestForTesting { authorizationRequestForTesting(authorized) }
            else if AVCaptureDevice.authorizationStatus(for: .audio) == .authorized { authorized(true) }
            else {
                AVCaptureDevice.requestAccess(for: .audio) { allowed in
                    DispatchQueue.main.async { authorized(allowed) }
                }
            }
        case "finish":
            captureCancelled = true
            captureIdentity = nil
            emit(["type":"ended"])
        case "stop":
            events.removeAll()
            captureCancelled = true
            captureIdentity = nil
            emit(["type":"ended"])
        default: return false
        }
        return true
    }

    func poll() -> UnsafePointer<CChar>? {
        if let retainedEvent { free(retainedEvent); self.retainedEvent = nil }
        guard !events.isEmpty else { return nil }
        retainedEvent = strdup(events.removeFirst())
        return retainedEvent.map { UnsafePointer($0) }
    }

    private func emit(_ value: [String: Any]) {
        guard !isClosed else { return }
        if value["type"] as? String == "level" {
            events.removeAll { $0.contains("\"type\":\"level\"") }
        }
        if events.count >= 128 {
            events = [#"{"type":"error","message":"Audio updates could not be delivered. The call stopped."}"#]
            close()
            return
        }
        guard let data = try? JSONSerialization.data(withJSONObject: value),
              data.count <= 65_536, let text = String(data: data, encoding: .utf8) else { return }
        events.append(text)
    }

    func close() {
        if let retainedEvent { free(retainedEvent); self.retainedEvent = nil }
        guard !isClosed else { return }
        isClosed = true
        captureCancelled = true
        captureIdentity = nil
    }
}

@_cdecl("decodex_voice_media_abi_version")
public func decodexVoiceMediaABIVersion() -> UInt32 { 4 }

@_cdecl("decodex_voice_media_create")
public func decodexVoiceMediaCreate(_ nativeView: UnsafeMutableRawPointer?) -> UnsafeMutableRawPointer? {
    guard Thread.isMainThread, let nativeView else { return nil }
    let viewAddress = UInt(bitPattern: nativeView)
    let address = MainActor.assumeIsolated {
        guard let viewPointer = UnsafeMutableRawPointer(bitPattern: viewAddress) else { return UInt(0) }
        let view = Unmanaged<NSView>.fromOpaque(viewPointer).takeUnretainedValue()
        guard view.window != nil else { return UInt(0) }
        return UInt(bitPattern: Unmanaged.passRetained(VoiceMediaHost()).toOpaque())
    }
    return UnsafeMutableRawPointer(bitPattern: address)
}

@_cdecl("decodex_voice_media_command")
public func decodexVoiceMediaCommand(_ pointer: UnsafeMutableRawPointer?, _ json: UnsafePointer<CChar>?) -> Bool {
    guard Thread.isMainThread, let pointer, let json else { return false }
    let address = UInt(bitPattern: pointer)
    let command = String(cString: json)
    return MainActor.assumeIsolated {
        guard let host = UnsafeMutableRawPointer(bitPattern: address) else { return false }
        return Unmanaged<VoiceMediaHost>.fromOpaque(host).takeUnretainedValue().command(command)
    }
}

@_cdecl("decodex_voice_media_poll")
public func decodexVoiceMediaPoll(_ pointer: UnsafeMutableRawPointer?) -> UnsafePointer<CChar>? {
    guard Thread.isMainThread, let pointer else { return nil }
    let address = UInt(bitPattern: pointer)
    let event: UInt = MainActor.assumeIsolated {
        guard let host = UnsafeMutableRawPointer(bitPattern: address) else { return 0 }
        return UInt(bitPattern: Unmanaged<VoiceMediaHost>.fromOpaque(host).takeUnretainedValue().poll())
    }
    return UnsafePointer<CChar>(bitPattern: event)
}

@_cdecl("decodex_voice_media_destroy")
public func decodexVoiceMediaDestroy(_ pointer: UnsafeMutableRawPointer?) {
    guard Thread.isMainThread, let pointer else { return }
    let address = UInt(bitPattern: pointer)
    MainActor.assumeIsolated {
        guard let host = UnsafeMutableRawPointer(bitPattern: address) else { return }
        Unmanaged<VoiceMediaHost>.fromOpaque(host).takeRetainedValue().close()
    }
}
