import AppKit
import Sparkle

/// Sparkle owns the complete app update cycle, including its standard user interface.
@MainActor
final class UpdateHost {
    private let controller: SPUStandardUpdaterController

    init?() {
        guard let key = Bundle.main.object(forInfoDictionaryKey: "SUPublicEDKey") as? String,
              !key.isEmpty,
              let feed = Bundle.main.object(forInfoDictionaryKey: "SUFeedURL") as? String,
              let url = URL(string: feed), url.scheme == "https", url.host != nil
        else { return nil }
        controller = SPUStandardUpdaterController(startingUpdater: true, updaterDelegate: nil, userDriverDelegate: nil)
    }

    func check() -> Bool {
        guard controller.updater.canCheckForUpdates else { return false }
        controller.checkForUpdates(nil)
        return true
    }
}

@_cdecl("decodex_update_create")
public func decodexUpdateCreate() -> UnsafeMutableRawPointer? {
    let address = MainActor.assumeIsolated {
        UpdateHost().map { UInt(bitPattern: Unmanaged.passRetained($0).toOpaque()) } ?? 0
    }
    return UnsafeMutableRawPointer(bitPattern: address)
}

@_cdecl("decodex_update_check")
public func decodexUpdateCheck(_ host: UnsafeMutableRawPointer?) -> Bool {
    guard let host else { return false }
    let address = UInt(bitPattern: host)
    return MainActor.assumeIsolated {
        guard let pointer = UnsafeMutableRawPointer(bitPattern: address) else { return false }
        return Unmanaged<UpdateHost>.fromOpaque(pointer).takeUnretainedValue().check()
    }
}

@_cdecl("decodex_update_destroy")
public func decodexUpdateDestroy(_ host: UnsafeMutableRawPointer?) {
    guard let host else { return }
    let address = UInt(bitPattern: host)
    MainActor.assumeIsolated {
        guard let pointer = UnsafeMutableRawPointer(bitPattern: address) else { return }
        Unmanaged<UpdateHost>.fromOpaque(pointer).release()
    }
}
