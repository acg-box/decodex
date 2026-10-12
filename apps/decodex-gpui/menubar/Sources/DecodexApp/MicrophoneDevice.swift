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
                  !isPrivateAggregate(device),
                  hasCaptureStreams(device) else { continue }
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

    nonisolated private static func isPrivateAggregate(_ device: AudioDeviceID) -> Bool {
        guard scalar(device, kAudioDevicePropertyTransportType) == kAudioDeviceTransportTypeAggregate else { return false }
        var property = AudioObjectPropertyAddress(mSelector: kAudioAggregateDevicePropertyComposition, mScope: kAudioObjectPropertyScopeGlobal, mElement: kAudioObjectPropertyElementMain)
        var value: Unmanaged<CFDictionary>?
        var size = UInt32(MemoryLayout<Unmanaged<CFDictionary>?>.size)
        guard AudioObjectGetPropertyData(device, &property, 0, nil, &size, &value) == noErr, let value else { return false }
        let composition = value.takeRetainedValue() as NSDictionary
        return (composition[kAudioAggregateDeviceIsPrivateKey] as? NSNumber)?.boolValue == true
    }

    nonisolated private static func hasCaptureStreams(_ device: AudioDeviceID) -> Bool {
        var property = AudioObjectPropertyAddress(mSelector: kAudioDevicePropertyStreams, mScope: kAudioObjectPropertyScopeGlobal, mElement: kAudioObjectPropertyElementMain)
        var size: UInt32 = 0
        guard AudioObjectGetPropertyDataSize(device, &property, 0, nil, &size) == noErr, size > 0 else { return false }
        var streams = [AudioStreamID](repeating: 0, count: Int(size) / MemoryLayout<AudioStreamID>.size)
        let status = streams.withUnsafeMutableBytes { AudioObjectGetPropertyData(device, &property, 0, nil, &size, $0.baseAddress!) }
        guard status == noErr else { return false }
        return hasCaptureInput(streams: streams.map {
            (direction: scalar($0, kAudioStreamPropertyDirection), terminal: scalar($0, kAudioStreamPropertyTerminalType))
        })
    }

    // VoiceProcessingIO adds echo-reference input streams to output devices.
    // Their terminals are unknown or output terminals, not microphone inputs.
    nonisolated static func hasCaptureInput(streams: [(direction: UInt32?, terminal: UInt32?)]) -> Bool {
        let outputTerminals: Set<UInt32> = [kAudioStreamTerminalTypeSpeaker, kAudioStreamTerminalTypeHeadphones, kAudioStreamTerminalTypeLFESpeaker, kAudioStreamTerminalTypeReceiverSpeaker, kAudioStreamTerminalTypeHDMI, kAudioStreamTerminalTypeDisplayPort]
        var unknownInput = false
        for stream in streams where stream.direction == 1 {
            guard let terminal = stream.terminal else { continue }
            if terminal == 0 || terminal == 0x0200 {
                unknownInput = true
            } else if !outputTerminals.contains(terminal) && !(0x0300..<0x0400).contains(terminal) {
                return true
            }
        }
        return unknownInput && streams.allSatisfy { $0.direction == 1 }
    }

    nonisolated static func namedInput(_ name: String, in inputs: [(id: AudioDeviceID, name: String)]) throws -> AudioDeviceID {
        let matches = inputs.filter { $0.name == name }
        guard let input = matches.first else { throw CaptureError.device }
        guard matches.count == 1 else { throw CaptureError.ambiguous }
        return input.id
    }
}
