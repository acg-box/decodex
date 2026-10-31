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
}

struct ConversationEstimatesView: View {
	let authority: ResetCardAuthority?
	let accounts: [ResetCardAccountState]
	let showsEmail: Bool
	@State private var opened = false
	@State private var result: ConversationEstimatesRead?
	@State private var loading = false
	@State private var failed = false
	@State private var revision = 0
	private let client = DecodexNativeClient()

	var body: some View {
		VStack(alignment: .leading, spacing: PanelSpacing.related) {
			Button {
				opened.toggle()
				if opened && result == nil { revision += 1 }
			} label: {
				Label("Conversation estimates", systemImage: "dollarsign.circle")
			}
			.buttonStyle(.plain)
			if opened {
				if loading { BrandLoading() }
				if let result, result.available {
					ForEach(result.rows) { row in
						if let amount = row.amount {
							VStack(alignment: .leading, spacing: 2) {
								HStack {
									Text(row.title).lineLimit(1)
									Spacer()
									Text(amount).monospacedDigit()
								}
								if let id = row.account_id {
									Text(accountLabel(id)).font(.caption).foregroundStyle(.secondary)
								}
							}
							.help(row.title)
						}
					}
					Text(result.rows.allSatisfy { $0.amount == nil } ? "No conversation costs reported." :
						(result.more_available ? "Latest 8 conversations · not an account total" : "Reported conversations · not an account total"))
						.font(.caption).foregroundStyle(.secondary)
					Text("Provider estimates may lag activity.").font(.caption).foregroundStyle(.secondary)
				} else if failed || result != nil {
					Text("Estimates unavailable. Connect an Agent and refresh.").font(.caption)
				}
				Button("Refresh estimates") { revision += 1 }.disabled(loading)
			}
		}
		.task(id: "\(authority?.profileName ?? "")-\(authority?.serverID ?? "")-\(revision)") {
			result = nil; failed = false
			guard opened else { return }
			loading = true
			defer { loading = false }
			do {
				let response = try await client.perform(DecodexNativeRequest(operation: "get_conversation_estimates"), authority: authority, as: ConversationEstimatesRead.self)
				guard !Task.isCancelled else { return }
				result = response.data
			} catch {
				guard !Task.isCancelled else { return }
				failed = true
			}
		}
	}

	private func accountLabel(_ id: String) -> String {
		guard let account = accounts.first(where: { $0.id == id }) else { return "Source account unavailable" }
		if showsEmail, let email = account.profile?.email { return email }
		return "Account \((accounts.firstIndex(where: { $0.id == id }) ?? 0) + 1)"
	}
}
