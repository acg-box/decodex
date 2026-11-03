import AppKit
import SwiftUI

struct ConversationEstimateRow: Decodable, Identifiable, Equatable, Sendable {
	let work_id: String
	let title: String
	let account_id: String?
	let usd_micros: UInt64?
	let native_usd_micros: UInt64?
	let credits_micros: UInt64?
	let observed_at_micros: Int64?
	var id: String { work_id }

	var amount: String? {
		if let native_usd_micros { return "$\(Self.micros(native_usd_micros))" }
		if let usd_micros { return "≈ $\(Self.micros(usd_micros))" }
		return nil
	}

	static func micros(_ value: UInt64) -> String {
		let fraction = value % 1_000_000
		guard fraction != 0 else { return "\(value / 1_000_000)" }
		let tail = String(format: "%06llu", fraction).replacingOccurrences(of: "0+$", with: "", options: .regularExpression)
		return "\(value / 1_000_000).\(tail)"
	}
}

struct ConversationEstimatesRead: Decodable, Equatable, Sendable {
	let available: Bool
	let rows: [ConversationEstimateRow]
	let more_available: Bool

	var pricedRows: [ConversationEstimateRow] {
		available ? rows.filter { $0.amount != nil } : []
	}
}

struct ConversationEstimatesView: View {
	let authority: ResetCardAuthority?
	let accounts: [ResetCardAccountState]
	let showsEmail: Bool
	@State private var opened = false
	@State private var result: ConversationEstimatesRead?
	@State private var revision = 0
	@State private var resultAuthority: ResetCardAuthority?
	@Environment(\.colorScheme) private var colorScheme
	@Environment(\.accessibilityReduceMotion) private var reduceMotion
	private let client = DecodexNativeClient()

	var body: some View {
		Group {
			if resultAuthority == authority, let result, result.pricedRows.isEmpty == false {
				VStack(alignment: .leading, spacing: PanelSpacing.related) {
					Button {
						withAnimation(reduceMotion ? nil : PanelMotion.controlState) {
							opened.toggle()
						}
					} label: {
						HStack(spacing: PanelSpacing.compact) {
							Label("Conversation estimates", systemImage: "dollarsign.circle")
							Spacer(minLength: 0)
							Image(systemName: "chevron.right")
								.rotationEffect(.degrees(opened ? 90 : 0))
						}
						.font(PanelFont.accountDetail)
						.foregroundStyle(PanelPalette.secondaryText(colorScheme))
						.contentShape(Rectangle())
					}
					.buttonStyle(.plain)
					.accessibilityValue(opened ? "Expanded" : "Collapsed")
					if opened {
						ForEach(result.pricedRows) { row in
							VStack(alignment: .leading, spacing: PanelSpacing.micro) {
								HStack(alignment: .firstTextBaseline, spacing: PanelSpacing.related) {
									Text(row.title).lineLimit(1)
									Spacer(minLength: 0)
									Text(row.amount ?? "").monospacedDigit().fixedSize()
								}
								.font(PanelFont.usageValue)
								.foregroundStyle(PanelPalette.primaryText(colorScheme))
								if let id = row.account_id {
									Text(accountLabel(id))
										.font(PanelFont.tertiary)
										.foregroundStyle(PanelPalette.secondaryText(colorScheme))
								}
							}
							.help(row.title)
						}
						Text(result.more_available ? "Latest 8 conversations · estimates may lag" : "Conversation estimates may lag")
							.font(PanelFont.tertiary)
							.foregroundStyle(PanelPalette.secondaryText(colorScheme))
					}
				}
			}
		}
		.onReceive(NotificationCenter.default.publisher(for: NSWindow.didBecomeKeyNotification)) { notification in
			if notification.object is TransparentStatusPanel { revision += 1 }
		}
		.task(id: "\(authority?.profileName ?? "")-\(authority?.serverID ?? "")-\(revision)") {
			guard revision > 0, let authority else {
				result = nil
				resultAuthority = nil
				return
			}
			do {
				let response = try await client.perform(
					DecodexNativeRequest(operation: "get_conversation_estimates"),
					authority: authority,
					as: ConversationEstimatesRead.self
				)
				guard !Task.isCancelled else { return }
				result = response.data
				resultAuthority = authority
			} catch {
				guard !Task.isCancelled else { return }
				result = nil
				resultAuthority = nil
			}
		}
	}

	private func accountLabel(_ id: String) -> String {
		guard let account = accounts.first(where: { $0.id == id }) else { return "Source account unavailable" }
		if showsEmail, let email = account.profile?.email { return email }
		return "Account \((accounts.firstIndex(where: { $0.id == id }) ?? 0) + 1)"
	}
}
