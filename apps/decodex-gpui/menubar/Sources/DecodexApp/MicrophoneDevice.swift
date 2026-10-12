import AppKit
import AVFoundation
import AudioToolbox
import CoreAudio

enum CaptureError: Error { case device, ambiguous }

/// Device discovery only. Rust owns the system voice-processing audio engine.
enum MicrophoneDevice {
    nonisolated static func device(named name: String) throws -> AudioDeviceID {
        // Zero means follow the system input, not pin today's default device ID.
        if name.isEmpty { return 0 }

        return try namedInput(name, in: inputs())
    }

    /// Use the same CoreAudio catalog for the menu and capture authorization.
    nonisolated static func inputs() throws -> [(id: AudioDeviceID, name: String)] {
        var property = AudioObjectPropertyAddress(mSelector: kAudioHardwarePropertyDevices, mScope: kAudioObjectPropertyScopeGlobal, mElement: kAudioObjectPropertyElementMain)
        var size: UInt32 = 0
        guard AudioObjectGetPropertyDataSize(AudioObjectID(kAudioObjectSystemObject), &property, 0, nil, &size) == noErr else { throw CaptureError.device }
        var devices = [AudioDeviceID](repeating: 0, count: Int(size) / MemoryLayout<AudioDeviceID>.size)
        let status = try devices.withUnsafeMutableBytes { buffer in
            guard let baseAddress = buffer.baseAddress else { throw CaptureError.device }
            return AudioObjectGetPropertyData(AudioObjectID(kAudioObjectSystemObject), &property, 0, nil, &size, baseAddress)
        }
        guard status == noErr else { throw CaptureError.device }
        var inputs: [(id: AudioDeviceID, name: String)] = []
        for device in devices {
            guard scalar(device, kAudioDevicePropertyDeviceIsAlive) == 1,
                  scalar(device, kAudioDevicePropertyIsHidden) != 1,
                  hasInputChannels(device) else { continue }
            var label: Unmanaged<CFString>?
            var labelSize = UInt32(MemoryLayout<Unmanaged<CFString>?>.size)
            var labelProperty = AudioObjectPropertyAddress(mSelector: kAudioObjectPropertyName, mScope: kAudioObjectPropertyScopeGlobal, mElement: kAudioObjectPropertyElementMain)
            guard AudioObjectGetPropertyData(device, &labelProperty, 0, nil, &labelSize, &label) == noErr, let label else { continue }
            let deviceName = label.takeRetainedValue() as String
            inputs.append((device, deviceName))
        }
        return inputs
    }

    nonisolated private static func scalar(_ device: AudioDeviceID, _ selector: AudioObjectPropertySelector) -> UInt32? {
        var property = AudioObjectPropertyAddress(mSelector: selector, mScope: kAudioObjectPropertyScopeGlobal, mElement: kAudioObjectPropertyElementMain)
        var value: UInt32 = 0
        var size = UInt32(MemoryLayout<UInt32>.size)
        guard AudioObjectGetPropertyData(device, &property, 0, nil, &size, &value) == noErr else { return nil }
        return value
    }

    nonisolated private static func hasInputChannels(_ device: AudioDeviceID) -> Bool {
        var property = AudioObjectPropertyAddress(mSelector: kAudioDevicePropertyStreamConfiguration, mScope: kAudioDevicePropertyScopeInput, mElement: kAudioObjectPropertyElementMain)
        var size: UInt32 = 0
        guard AudioObjectGetPropertyDataSize(device, &property, 0, nil, &size) == noErr,
              size >= MemoryLayout<AudioBufferList>.size else { return false }
        let storage = UnsafeMutableRawPointer.allocate(byteCount: Int(size), alignment: MemoryLayout<AudioBufferList>.alignment)
        defer { storage.deallocate() }
        guard AudioObjectGetPropertyData(device, &property, 0, nil, &size, storage) == noErr else { return false }
        return UnsafeMutableAudioBufferListPointer(storage.assumingMemoryBound(to: AudioBufferList.self)).contains { $0.mNumberChannels > 0 }
    }

    nonisolated static func namedInput(_ name: String, in inputs: [(id: AudioDeviceID, name: String)]) throws -> AudioDeviceID {
        let matches = inputs.filter { $0.name == name }
        guard let input = matches.first else { throw CaptureError.device }
        guard matches.count == 1 else { throw CaptureError.ambiguous }
        return input.id
    }
}
