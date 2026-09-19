import Foundation

/// One server utterance can receive many revisions. Final text replaces provisional text.
struct DictationTranscript {
    struct Segment { var revision: Int; var text: String; var final: Bool }
    private var order: [String] = []
    private var segments: [String: Segment] = [:]
    mutating func apply(id: String, revision: Int, text: String, final: Bool) -> Bool {
        if let old = segments[id], old.final || revision < old.revision { return false }
        if segments[id] == nil { order.append(id) }
        segments[id] = Segment(revision: revision, text: text, final: final)
        return true
    }
    var text: String { order.compactMap { segments[$0]?.text.trimmingCharacters(in: .whitespacesAndNewlines) }.filter { !$0.isEmpty }.joined(separator: " ") }
}

/// Service-only transport. Its token comes from the retained native app-server connection.
/// URLSession owns networking; this class never opens a microphone or starts an agent turn.
final class DictationStream: @unchecked Sendable {
    private let queue = DispatchQueue(label: "box.acg.decodex.dictation")
    private let session: URLSession
    private let socket: URLSessionWebSocketTask
    private var events: [String] = []
    private var outgoing: [String] = []
    private var sending = false
    private var finishing = false
    private var closed = false
    private var retained: UnsafeMutablePointer<CChar>?
    private var transcript = DictationTranscript()

    init(token: String) {
        let configuration = URLSessionConfiguration.ephemeral
        configuration.urlCache = nil
        configuration.httpCookieStorage = nil
        configuration.urlCredentialStorage = nil
        configuration.timeoutIntervalForRequest = 20
        configuration.timeoutIntervalForResource = 300
        session = URLSession(configuration: configuration)
        socket = session.webSocketTask(with: URL(string: "wss://chatgpt.com/backend-api/dictation/stream")!, protocols: ["chatgpt-dictation", "openai-bearer." + token, "codex-desktop"])
        socket.maximumMessageSize = 131_072
        socket.resume()
        queue.async { [self] in
            enqueue(["type":"session.start", "config":["input_audio_format":"pcm16", "sample_rate_hz":24000, "num_channels":1, "max_buffer_size_bytes":4194304, "max_utterance_duration_ms":30000, "session_ttl_ms":300000, "provider_mode":"streaming_sse", "transcript_delivery_mode":"segment", "vad":["type":"server_vad", "threshold":0.5, "prefix_padding_ms":300, "silence_duration_ms":500]]])
            receive()
        }
    }

    func command(_ json: String) -> Bool {
        guard json.utf8.count <= 70_000, let data = json.data(using: .utf8),
              let value = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let operation = value["operation"] as? String else { return false }
        return queue.sync {
            guard !closed else { return false }
            switch operation {
            case "audio":
                guard !finishing, let audio = value["audio"] as? String,
                      let pcm = Data(base64Encoded: audio), !pcm.isEmpty,
                      pcm.count <= 32_768, pcm.count.isMultiple(of: 2) else { return false }
                enqueue(["type":"audio.append", "audio":audio])
            case "finish":
                if !finishing { finishing = true; enqueue(["type":"session.close"]) }
            default: return false
            }
            return !closed
        }
    }

    func poll() -> UnsafePointer<CChar>? {
        queue.sync {
            if let retained { free(retained); self.retained = nil }
            guard !events.isEmpty else { return nil }
            retained = strdup(events.removeFirst())
            return retained.map { UnsafePointer($0) }
        }
    }

    func close() { queue.sync { stop() } }

    private func enqueue(_ value: [String: Any]) {
        guard outgoing.count < 32, let data = try? JSONSerialization.data(withJSONObject: value) else {
            fail("Audio could not be sent fast enough. Your received text remains in the draft.")
            return
        }
        outgoing.append(String(decoding: data, as: UTF8.self))
        drain()
    }

    private func drain() {
        guard !closed, !sending, !outgoing.isEmpty else { return }
        sending = true
        socket.send(.string(outgoing.removeFirst())) { [weak self] error in
            guard let self else { return }
            queue.async {
                self.sending = false
                if error != nil { self.fail("Dictation connection failed. Your received text remains in the draft.") }
                else { self.drain() }
            }
        }
    }

    private func receive() {
        socket.receive { [weak self] result in
            guard let self else { return }
            queue.async {
                guard !self.closed else { return }
                switch result {
                case .failure:
                    self.fail("Dictation disconnected before final correction. Your received text remains in the draft.")
                case .success(let message):
                    let data: Data
                    switch message {
                    case .data(let value): data = value
                    case .string(let value): data = Data(value.utf8)
                    @unknown default: self.fail("Unsupported dictation response."); return
                    }
                    self.handle(data)
                    if !self.closed { self.receive() }
                }
            }
        }
    }

    private func handle(_ data: Data) {
        guard data.count <= 131_072,
              let event = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let type = event["type"] as? String else { fail("Invalid dictation response."); return }
        switch type {
        case "session.started": emit(["kind":"ready"])
        case "transcript.segment", "transcript.final":
            guard let id = event["utterance_id"] as? String, id.count <= 256,
                  let revision = event["revision"] as? Int,
                  let text = event["text"] as? String, text.utf8.count <= 32_768 else { fail("Dictation exceeded the draft limit."); return }
            if transcript.apply(id: id, revision: revision, text: text, final: type == "transcript.final") {
                guard transcript.text.utf8.count <= 32_768 else { fail("Dictation exceeded the draft limit."); return }
                emit(["kind":"transcript", "text":transcript.text])
            }
        case "session.error", "error": fail("The subscription dictation service could not finish this recording.")
        case "session.closed": complete()
        case "session.updated":
            if (event["session"] as? [String:Any])?["status"] as? String == "closed" { complete() }
        default: break
        }
    }

    private func complete() { emit(["kind":"complete", "text":transcript.text]); stop() }
    private func fail(_ message: String) {
        guard !closed else { return }
        emit(["kind":"error", "message":message]); stop()
    }
    private func emit(_ event: [String:Any]) {
        if event["kind"] as? String == "transcript" { events.removeAll { $0.contains("\"kind\":\"transcript\"") } }
        guard let data = try? JSONSerialization.data(withJSONObject:event,options:.sortedKeys) else { return }
        if events.count >= 64 { events.removeFirst() }
        events.append(String(decoding:data,as:UTF8.self))
    }
    private func stop() {
        guard !closed else { return }
        closed = true
        outgoing.removeAll()
        socket.cancel(with:.normalClosure,reason:nil)
        session.invalidateAndCancel()
    }
    deinit { if let retained { free(retained) } }
}

@_cdecl("decodex_dictation_create")
public func decodexDictationCreate(_ token: UnsafePointer<CChar>?) -> UnsafeMutableRawPointer? {
    guard let token else { return nil }
    let value = String(cString:token)
    guard !value.isEmpty, value.utf8.count <= 16_384 else { return nil }
    return Unmanaged.passRetained(DictationStream(token:value)).toOpaque()
}
@_cdecl("decodex_dictation_command")
public func decodexDictationCommand(_ host: UnsafeMutableRawPointer?, _ json: UnsafePointer<CChar>?) -> Bool {
    guard let host, let json else { return false }
    return Unmanaged<DictationStream>.fromOpaque(host).takeUnretainedValue().command(String(cString:json))
}
@_cdecl("decodex_dictation_poll")
public func decodexDictationPoll(_ host: UnsafeMutableRawPointer?) -> UnsafePointer<CChar>? {
    guard let host else { return nil }
    return Unmanaged<DictationStream>.fromOpaque(host).takeUnretainedValue().poll()
}
@_cdecl("decodex_dictation_destroy")
public func decodexDictationDestroy(_ host: UnsafeMutableRawPointer?) {
    guard let host else { return }
    Unmanaged<DictationStream>.fromOpaque(host).takeRetainedValue().close()
}
