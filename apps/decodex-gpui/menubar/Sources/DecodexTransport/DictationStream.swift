import Foundation

/// Service-only transport. Its token comes from the retained native app-server connection.
/// URLSession owns networking; this class never opens a microphone or starts an agent turn.
final class DictationStream: @unchecked Sendable {
    private let queue = DispatchQueue(label: "box.acg.decodex.dictation")
    private let session: URLSession
    private let socket: URLSessionWebSocketTask
    private var events: [String] = []
    private var outgoing: [String] = []
    private var sending = false
    private var closed = false
    private var retained: UnsafeMutablePointer<CChar>?

    init(token: String, initialMessage: String) {
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
            enqueue(initialMessage)
            receive()
        }
    }

    func command(_ json: String) -> Bool {
        guard json.utf8.count <= 70_000 else { return false }
        return queue.sync {
            guard !closed else { return false }
            enqueue(json)
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

    private func enqueue(_ message: String) {
        guard outgoing.count < 32 else {
            fail("Audio could not be sent fast enough. Your received text remains in the draft.")
            return
        }
        outgoing.append(message)
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
        guard data.count <= 131_072, let message = String(data: data, encoding: .utf8) else {
            fail("Invalid dictation response.")
            return
        }
        emit(["kind":"message", "message":message])
    }

    private func fail(_ message: String) {
        guard !closed else { return }
        emit(["kind":"error", "message":message]); stop()
    }
    private func emit(_ event: [String:Any]) {
        guard let data = try? JSONSerialization.data(withJSONObject:event,options:.sortedKeys) else { return }
        // Segments are incremental: never discard one to make space for another.
        // Reserve the last slot for a terminal error, after all accepted segments.
        guard events.count < 63 else {
            if !closed {
                events.append("{\"kind\":\"error\",\"message\":\"Dictation updates could not be received fast enough. Your received text remains in the draft.\"}")
                stop()
            }
            return
        }
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

@_cdecl("decodex_dictation_create_v2")
public func decodexDictationCreate(_ token: UnsafePointer<CChar>?, _ initialMessage: UnsafePointer<CChar>?) -> UnsafeMutableRawPointer? {
    guard let token, let initialMessage else { return nil }
    let value = String(cString:token)
    guard !value.isEmpty, value.utf8.count <= 16_384 else { return nil }
    let message = String(cString: initialMessage)
    guard message.utf8.count <= 70_000 else { return nil }
    return Unmanaged.passRetained(DictationStream(token:value, initialMessage:message)).toOpaque()
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
