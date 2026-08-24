import AppKit
import SwiftUI

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
	private var store: ResetCardStore?
	private var statusPanelController: StatusPanelController?
	private var terminationIsPending = false

	func applicationDidFinishLaunching(_ notification: Notification) {
		ProcessInfo.processInfo.automaticTerminationSupportEnabled = false
		ProcessInfo.processInfo.disableAutomaticTermination(
			"Decodex owns a persistent menu bar status item."
		)
		ProcessInfo.processInfo.disableSuddenTermination()
		NSApp.setActivationPolicy(.accessory)
		let store = ResetCardStore()
		self.store = store
		statusPanelController = StatusPanelController(store: store)
		store.start()
	}

	func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
		guard terminationIsPending == false else {
			return .terminateLater
		}
		terminationIsPending = true
		Task { [store] in
			await store?.prepareForApplicationTermination()
			await DecodexNativeClient.shutdownSharedSession()
			sender.reply(toApplicationShouldTerminate: true)
		}
		return .terminateLater
	}

	func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
		false
	}
}

enum AppAssets {
	static let statusBarIcon: NSImage = {
		let image = Bundle.main.url(forResource: "StatusBarIcon", withExtension: "png")
			.flatMap(NSImage.init(contentsOf:))
			?? NSImage(named: "StatusBarIcon")
			?? NSImage(systemSymbolName: "person.2.circle", accessibilityDescription: "Decodex")
			?? NSImage()
		image.isTemplate = true
		image.size = NSSize(width: 22, height: 22)
		return image
	}()
}

@main
struct DecodexApp: App {
	@NSApplicationDelegateAdaptor(AppDelegate.self) private var appDelegate

	var body: some Scene {
		Settings {
			EmptyView()
		}
	}
}
