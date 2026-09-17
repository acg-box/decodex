import AppKit
import AVFoundation
import WebKit

/// Native media only. Rust owns authentication, session authorization and task state.
/// The isolated document receives SDP, never account credentials or conversation history.
@MainActor
final class VoiceMediaHost: NSObject, WKScriptMessageHandler, WKUIDelegate, WKNavigationDelegate {
    private var webView: WKWebView?
    private var events: [String] = []
    private var retainedEvent: UnsafeMutablePointer<CChar>?
    private var isClosed = false
    private var isReady = false
    private let initializedAt = Date()
    private var initializationFailed = false
    private var pendingCommand: String?
    private var desiredMute = false
    private var syntheticAudio = false
    private let origin = URL(string: "https://decodex.invalid")!

    init(syntheticAudioForTesting: Bool = false, sampleForTesting: Data? = nil, hostWindow: NSWindow? = nil) {
        super.init()
        syntheticAudio = syntheticAudioForTesting
        let configuration = WKWebViewConfiguration()
        configuration.websiteDataStore = .nonPersistent()
        configuration.mediaTypesRequiringUserActionForPlayback = []
        configuration.userContentController.add(self, name: "voice")
        let view = WKWebView(frame: NSRect(x: 0, y: 0, width: 1, height: 1), configuration: configuration)
        view.uiDelegate = self
        view.navigationDelegate = self
        webView = view
        // WebKit capture and media scheduling require a host window.
        // Keep the media view behind the native interface, not in a detached page.
        if let content = hostWindow?.contentView ?? NSApp.keyWindow?.contentView ?? NSApp.mainWindow?.contentView {
            content.addSubview(view, positioned: .below, relativeTo: nil)
        }
        var document = Self.document
        #if DEBUG
        if syntheticAudioForTesting {
            document = document.replacingOccurrences(
                of: "await navigator.mediaDevices.getUserMedia({video:false,audio:{echoCancellation:true,noiseSuppression:true,autoGainControl:true}})",
                with: "(window.testAudioContext = new AudioContext(), window.testAudioDestination = testAudioContext.createMediaStreamDestination(), testAudioDestination.stream)"
            )
        }
        if syntheticAudioForTesting, let sampleForTesting {
            document = document.replacingOccurrences(of: "/* TEST_AUDIO */", with:
                "const sample = Uint8Array.from(atob('" + sampleForTesting.base64EncodedString() + "'),c=>c.charCodeAt(0)); (async()=>{const source=testAudioContext.createBufferSource();source.buffer=await testAudioContext.decodeAudioData(sample.buffer);source.connect(testAudioDestination);source.connect(testAudioContext.destination);source.start();await testAudioContext.resume();})();")
        }
        #endif
        view.loadHTMLString(document, baseURL: origin)
    }

    func command(_ json: String) -> Bool {
        guard !isClosed, json.utf8.count <= 65_536,
              let bytes = json.data(using: .utf8),
              let value = try? JSONSerialization.jsonObject(with: bytes) as? [String: Any],
              let operation = value["operation"] as? String,
              ["start", "answer", "mute", "stop"].contains(operation) else { return false }
        if operation == "mute" { desiredMute = value["muted"] as? Bool ?? false }
        if !isReady {
            if operation == "mute" { return true }
            guard operation == "start", pendingCommand == nil else { return false }
            pendingCommand = json
            return true
        }
        if operation == "start" { startCapture(json) } else { evaluate(json) }
        return true
    }

    private func startCapture(_ json: String) {
        emit(["type":"status", "message":"Waiting for microphone permission…"])
        if syntheticAudio { finishAuthorization(true, command: json); return }
        AVCaptureDevice.requestAccess(for: .audio) { [weak self] permitted in
            DispatchQueue.main.async { self?.finishAuthorization(permitted, command: json) }
        }
    }

    private func finishAuthorization(_ permitted: Bool, command: String) {
        guard !isClosed else { return }
        guard permitted else {
            emit(["type":"error", "message":"Microphone access is unavailable. Allow Decodex in System Settings > Privacy & Security > Microphone."])
            return
        }
        emit(["type":"status", "message":"Opening microphone…"])
        evaluate("{\"operation\":\"mute\",\"muted\":\(desiredMute)}")
        evaluate(command)
    }

    private func evaluate(_ json: String) {
        // callAsyncJavaScript passes data as an argument; SDP never becomes executable source.
        webView?.callAsyncJavaScript(
            "await window.decodexVoice.command(JSON.parse(command))",
            arguments: ["command": json], in: nil, in: .page
        ) { [weak self] result in
            guard case .failure = result else { return }
            self?.emit(["type": "error", "message": "The audio session could not continue."])
        }
    }

    #if DEBUG
    func connectionDiagnostics() async -> String {
        guard let webView else { return "closed" }
        return await withCheckedContinuation { continuation in
            webView.callAsyncJavaScript("return JSON.stringify(await window.decodexVoice.diagnostics())", arguments: [:], in: nil, in: .page) { result in
                continuation.resume(returning: (try? result.get()) as? String ?? "unavailable")
            }
        }
    }
    #endif

    func poll() -> UnsafePointer<CChar>? {
        if !isReady && !initializationFailed && initializedAt.timeIntervalSinceNow < -10 {
            initializationFailed = true
            emit(["type":"error", "message":"The audio host did not initialize. Start a new call."])
        }
        if let retainedEvent { free(retainedEvent); self.retainedEvent = nil }
        guard !events.isEmpty else { return nil }
        retainedEvent = strdup(events.removeFirst())
        return retainedEvent.map { UnsafePointer($0) }
    }

    private func emit(_ value: [String: Any]) {
        guard !isClosed else { return }
        if events.count >= 128 {
            events = [#"{"type":"error","message":"Audio updates could not be delivered. The call stopped."}"#]
            close()
            return
        }
        guard let data = try? JSONSerialization.data(withJSONObject: value),
              data.count <= 65_536, let text = String(data: data, encoding: .utf8) else { return }
        events.append(text)
    }

    func userContentController(_ userContentController: WKUserContentController,
                               didReceive message: WKScriptMessage) {
        guard message.frameInfo.isMainFrame,
              message.webView === webView,
              let value = message.body as? [String: Any],
              let type = value["type"] as? String,
              ["ready", "offer", "connected", "caption", "ended", "error"].contains(type)
        else { return }
        if type == "ready" {
            guard value["canCapture"] as? Bool == true else {
                emit(["type":"error", "message":"The native audio environment could not initialize."])
                return
            }
            isReady = true
            if let command = pendingCommand { pendingCommand = nil; startCapture(command) }
        } else {
            emit(value)
        }
    }

    func webView(_ webView: WKWebView, requestMediaCapturePermissionFor origin: WKSecurityOrigin,
                 initiatedByFrame frame: WKFrameInfo, type: WKMediaCaptureType,
                 decisionHandler: @escaping @MainActor (WKPermissionDecision) -> Void) {
        // macOS retains the microphone permission decision. Never request camera access.
        decisionHandler(!isClosed && frame.isMainFrame && origin.host == self.origin.host
                        && type == .microphone && AVCaptureDevice.authorizationStatus(for: .audio) == .authorized ? .grant : .deny)
    }

    func webView(_ webView: WKWebView, decidePolicyFor navigationAction: WKNavigationAction,
                 decisionHandler: @escaping @MainActor (WKNavigationActionPolicy) -> Void) {
        decisionHandler(navigationAction.request.url?.absoluteString == "about:blank"
                        || navigationAction.request.url?.host == origin.host ? .allow : .cancel)
    }

    func webViewWebContentProcessDidTerminate(_ webView: WKWebView) {
        emit(["type": "error", "message": "The audio process stopped. Start a new call to reconnect."])
        close()
    }

    func close() {
        guard !isClosed else { return }
        isClosed = true
        pendingCommand = nil
        webView?.configuration.userContentController.removeScriptMessageHandler(forName: "voice")
        webView?.setMicrophoneCaptureState(.none, completionHandler: nil)
        webView?.pauseAllMediaPlayback(completionHandler: nil)
        webView?.stopLoading()
        webView?.uiDelegate = nil
        webView?.navigationDelegate = nil
        webView?.removeFromSuperview()
        webView = nil
        if let retainedEvent { free(retainedEvent); self.retainedEvent = nil }
    }

    static let document = #"""
    <!doctype html><meta charset="utf-8">
    <meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'unsafe-inline'; connect-src 'none'; media-src blob: mediastream:">
    <audio id="reply" autoplay></audio>
    <script>
    (() => {
      let peer = null, microphone = null, channel = null, generation = 0, connectionTimeout = null, muted = false;
      const emit = value => window.webkit.messageHandlers.voice.postMessage(value);
      const reply = document.getElementById('reply');
      function stop() {
        generation++; clearTimeout(connectionTimeout); connectionTimeout = null;
        microphone?.getTracks().forEach(track => track.stop());
        channel?.close(); peer?.close();
        reply.pause(); reply.srcObject = null;
        microphone = null; channel = null; peer = null;
      }
      async function start() {
        stop(); const active = generation;
        let capture;
        try {
          capture = await navigator.mediaDevices.getUserMedia({video:false,audio:{echoCancellation:true,noiseSuppression:true,autoGainControl:true}});
          if (active !== generation) { capture.getTracks().forEach(track => track.stop()); return; }
          microphone = capture;
          capture.getAudioTracks().forEach(track => track.enabled = !muted);
          connectionTimeout = setTimeout(() => { if (active === generation) { stop(); emit({type:'error',message:'The audio connection timed out.'}); } }, 30000);
          const connection = new RTCPeerConnection(); peer = connection;
          capture.getAudioTracks().forEach(track => connection.addTrack(track,capture));
          connection.ontrack = event => { reply.srcObject = event.streams[0] || new MediaStream([event.track]); reply.play().catch(() => emit({type:'error',message:'Audio playback was blocked.'})); };
          connection.onconnectionstatechange = () => {
            if (active !== generation) return;
            if (connection.connectionState === 'connected') { clearTimeout(connectionTimeout); emit({type:'connected'}); }
            if (['failed','disconnected'].includes(connection.connectionState)) { stop(); emit({type:'error',message:'The audio connection was lost.'}); }
          };
          channel = connection.createDataChannel('oai-events');
          channel.onmessage = event => {
            if (active !== generation || event.data.length > 65536) return;
            try {
              const value = JSON.parse(event.data);
              if (value.type === 'session.started') { /* TEST_AUDIO */ }
              // Captions are presentation data, never agent instructions.
              if (['input_transcript.added','output_transcript.added','turn.created','turn.done','turn.delta'].includes(value.type)) emit({type:'caption',event:value});
            } catch {}
          };
          await connection.setLocalDescription(await connection.createOffer());
          if (connection.iceGatheringState !== 'complete') await new Promise(resolve => {
            const finish = () => { clearTimeout(timer); connection.removeEventListener('icegatheringstatechange', changed); resolve(); };
            const changed = () => { if (connection.iceGatheringState === 'complete') finish(); };
            const timer = setTimeout(finish, 3000);
            connection.addEventListener('icegatheringstatechange', changed);
          });
          if (active === generation) emit({type:'offer',sdp:connection.localDescription.sdp});
        } catch { if (active === generation) { stop(); emit({type:'error',message:'Microphone access is unavailable. Check Decodex microphone permission in System Settings.'}); } }
      }
      window.decodexVoice = {async diagnostics() { const stats=peer ? [...(await peer.getStats()).values()].filter(s=>['outbound-rtp','inbound-rtp'].includes(s.type)).map(s=>({type:s.type,bytesSent:s.bytesSent,bytesReceived:s.bytesReceived,packetsSent:s.packetsSent})) : []; return {muted:microphone?.getAudioTracks().every(track=>!track.enabled),audio:window.testAudioContext?.state,stats,connection:peer?.connectionState,ice:peer?.iceConnectionState,gathering:peer?.iceGatheringState,signaling:peer?.signalingState,channel:channel?.readyState,localCandidates:(peer?.localDescription?.sdp.match(/a=candidate:/g)||[]).length,remoteCandidates:(peer?.remoteDescription?.sdp.match(/a=candidate:/g)||[]).length}; },async command(value) {
        if (value.operation === 'start') return start();
        if (value.operation === 'stop') { stop(); emit({type:'ended'}); return; }
        if (value.operation === 'mute') { muted = !!value.muted; microphone?.getAudioTracks().forEach(track => track.enabled = !muted); return; }
        if (value.operation === 'answer' && peer && typeof value.sdp === 'string') await peer.setRemoteDescription({type:'answer',sdp:value.sdp});
      }};
      window.addEventListener('pagehide',stop);
      emit({type:'ready',canCapture:window.isSecureContext && !!navigator.mediaDevices?.getUserMedia});
    })();
    </script>
    """#
}

@_cdecl("decodex_voice_media_abi_version")
public func decodexVoiceMediaABIVersion() -> UInt32 { 1 }

@_cdecl("decodex_voice_media_create")
public func decodexVoiceMediaCreate() -> UnsafeMutableRawPointer? {
    guard Thread.isMainThread else { return nil }
    let address = MainActor.assumeIsolated { UInt(bitPattern: Unmanaged.passRetained(VoiceMediaHost()).toOpaque()) }
    return UnsafeMutableRawPointer(bitPattern: address)
}

@_cdecl("decodex_voice_media_command")
public func decodexVoiceMediaCommand(_ pointer: UnsafeMutableRawPointer?, _ json: UnsafePointer<CChar>?) -> Bool {
    guard Thread.isMainThread, let pointer, let json else { return false }
    let address = UInt(bitPattern: pointer)
    let command = String(cString: json)
    return MainActor.assumeIsolated {
        guard let host = UnsafeMutableRawPointer(bitPattern: address) else { return false }
        return Unmanaged<VoiceMediaHost>.fromOpaque(host).takeUnretainedValue().command(command)
    }
}

@_cdecl("decodex_voice_media_poll")
public func decodexVoiceMediaPoll(_ pointer: UnsafeMutableRawPointer?) -> UnsafePointer<CChar>? {
    guard Thread.isMainThread, let pointer else { return nil }
    let address = UInt(bitPattern: pointer)
    let event: UInt = MainActor.assumeIsolated {
        guard let host = UnsafeMutableRawPointer(bitPattern: address) else { return 0 }
        return UInt(bitPattern: Unmanaged<VoiceMediaHost>.fromOpaque(host).takeUnretainedValue().poll())
    }
    return UnsafePointer<CChar>(bitPattern: event)
}

@_cdecl("decodex_voice_media_destroy")
public func decodexVoiceMediaDestroy(_ pointer: UnsafeMutableRawPointer?) {
    guard Thread.isMainThread, let pointer else { return }
    let address = UInt(bitPattern: pointer)
    MainActor.assumeIsolated {
        guard let host = UnsafeMutableRawPointer(bitPattern: address) else { return }
        Unmanaged<VoiceMediaHost>.fromOpaque(host).takeRetainedValue().close()
    }
}
