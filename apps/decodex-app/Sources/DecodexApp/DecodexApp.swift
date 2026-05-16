import AppKit
import SwiftUI

final class AppDelegate: NSObject, NSApplicationDelegate {
	func applicationDidFinishLaunching(_ notification: Notification) {
		NSApp.setActivationPolicy(.accessory)
	}
}

private enum AppAssets {
	static let statusBarIcon: NSImage = {
		let image = NSImage(named: "StatusBarIcon")
			?? Bundle.main.url(forResource: "StatusBarIcon", withExtension: "png")
				.flatMap(NSImage.init(contentsOf:))
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
	@StateObject private var store = AccountStore()

	var body: some Scene {
		MenuBarExtra {
			AccountPanelView(store: store)
				.task {
					await store.refresh()
				}
		} label: {
			Label {
				Text("Decodex")
			} icon: {
				Image(nsImage: AppAssets.statusBarIcon)
			}
		}
		.menuBarExtraStyle(.window)

		Settings {
			SettingsView(store: store)
		}
	}
}
