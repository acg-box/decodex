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
            var label: Unmanaged<CFString>?
            var labelSize = UInt32(MemoryLayout<Unmanaged<CFString>?>.size)
            var labelProperty = AudioObjectPropertyAddress(mSelector: kAudioObjectPropertyName, mScope: kAudioObjectPropertyScopeGlobal, mElement: kAudioObjectPropertyElementMain)
            guard AudioObjectGetPropertyData(device, &labelProperty, 0, nil, &labelSize, &label) == noErr, let label else { continue }
            let deviceName = label.takeRetainedValue() as String
            var streams = AudioObjectPropertyAddress(mSelector: kAudioDevicePropertyStreams, mScope: kAudioDevicePropertyScopeInput, mElement: kAudioObjectPropertyElementMain)
            var streamSize: UInt32 = 0
            if AudioObjectGetPropertyDataSize(device, &streams, 0, nil, &streamSize) == noErr && streamSize > 0 { inputs.append((device, deviceName)) }
        }
        return try namedInput(name, in: inputs)
    }

    nonisolated static func namedInput(_ name: String, in inputs: [(id: AudioDeviceID, name: String)]) throws -> AudioDeviceID {
        let matches = inputs.filter { $0.name == name }
        guard let input = matches.first else { throw CaptureError.device }
        guard matches.count == 1 else { throw CaptureError.ambiguous }
        return input.id
    }
}
