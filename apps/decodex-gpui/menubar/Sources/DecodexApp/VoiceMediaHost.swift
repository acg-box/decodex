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
    private var captureRequestedAt: Date?
    private var nativeDictation: (any DictationCapturing)?
    private let dictationFactoryForTesting: ((@escaping @MainActor @Sendable ([String: Any]) -> Void) -> any DictationCapturing)?
    private var desiredMute = false
    private var captureCancelled = false
    private var captureIdentity: UUID?
    private let authorizationRequestForTesting: ((@escaping @MainActor (Bool) -> Void) -> Void)?
    private var syntheticAudio = false
    private static let mediaDataStore = WKWebsiteDataStore.nonPersistent()
    private let origin = URL(string: "https://decodex.invalid")!

    init(syntheticAudioForTesting: Bool = false, sampleForTesting: Data? = nil, hostWindow: NSWindow? = nil,
         authorizationRequestForTesting: ((@escaping @MainActor (Bool) -> Void) -> Void)? = nil,
         dictationFactoryForTesting: ((@escaping @MainActor @Sendable ([String: Any]) -> Void) -> any DictationCapturing)? = nil) {
        self.authorizationRequestForTesting = authorizationRequestForTesting
        self.dictationFactoryForTesting = dictationFactoryForTesting
        super.init()
        syntheticAudio = syntheticAudioForTesting
        let configuration = WKWebViewConfiguration()
        configuration.websiteDataStore = Self.mediaDataStore
        configuration.mediaTypesRequiringUserActionForPlayback = []
        configuration.userContentController.add(self, name: "voice")
        let view = WKWebView(frame: NSRect(x: 0, y: 0, width: 1, height: 1), configuration: configuration)
        view.uiDelegate = self
        view.navigationDelegate = self
        webView = view
        // WebKit capture and media scheduling require a host window.
        // Keep the media view behind the native interface, not in a detached page.
        let window = hostWindow ?? NSApp.keyWindow ?? NSApp.mainWindow
            ?? NSApp.windows.first { $0.isVisible && $0.level == .normal && $0.contentView != nil }
        guard let content = window?.contentView else {
            emit(["type":"error", "message":"Open a Decodex window before starting voice."])
            return
        }
        content.addSubview(view, positioned: .below, relativeTo: nil)
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
              ["start", "dictate", "finish", "answer", "mute", "stop", "devices"].contains(operation) else { return false }
        if operation == "devices" {
            let discovery = AVCaptureDevice.DiscoverySession(deviceTypes: [.microphone, .external], mediaType: .audio, position: .unspecified)
            emit(["type":"devices", "inputs":discovery.devices.map { $0.localizedName }])
            return true
        }
        if operation == "dictate", !syntheticAudio || dictationFactoryForTesting != nil {
            captureCancelled = false
            let identity = UUID()
            captureIdentity = identity
            nativeDictation?.stop()
            nativeDictation = nil
            captureRequestedAt = Date()
            let input = value["input"] as? String ?? ""
            if let authorizationRequestForTesting {
                authorizationRequestForTesting { [weak self] allowed in
                    guard let self, !self.isClosed, !self.captureCancelled, self.captureIdentity == identity else { return }
                    if allowed { self.beginNativeDictation(input) }
                    else { self.emit(["type":"error", "message":"Allow microphone access in System Settings."]) }
                }
            } else if AVCaptureDevice.authorizationStatus(for: .audio) == .authorized {
                beginNativeDictation(input)
            } else {
                AVCaptureDevice.requestAccess(for: .audio) { [weak self] allowed in
                    DispatchQueue.main.async {
                        guard let self, !self.isClosed, !self.captureCancelled, self.captureIdentity == identity else { return }
                        if allowed { self.beginNativeDictation(input) }
                        else { self.emit(["type":"error", "message":"Allow microphone access in System Settings."]) }
                    }
                }
            }
            return true
        }
        if let nativeDictation, operation == "finish" || operation == "stop" {
            captureCancelled = true
            // Finish must still deliver this capture's final PCM and ended event.
            if operation == "finish" { nativeDictation.finish() }
            else { captureIdentity = nil; nativeDictation.stop(); self.nativeDictation = nil; emit(["type":"ended"]) }
            return true
        }
        if operation == "start" || operation == "dictate" {
            if !isReady && pendingCommand != nil { return false }
            captureCancelled = false
            captureIdentity = UUID()
            nativeDictation?.stop()
            nativeDictation = nil
        }
        if operation == "stop" || operation == "finish" {
            captureCancelled = true
            captureIdentity = nil
            if !isReady { pendingCommand = nil; emit(["type":"ended"]); return true }
        }
        if operation == "mute" { desiredMute = value["muted"] as? Bool ?? false }
        if !isReady {
            if operation == "mute" { return true }
            guard ["start", "dictate"].contains(operation), pendingCommand == nil else { return false }
            pendingCommand = json
            return true
        }
        if operation == "start" || operation == "dictate" { startCapture(json) } else { evaluate(json) }
        return true
    }

    private func beginNativeDictation(_ input: String) {
        guard let identity = captureIdentity, !isClosed, !captureCancelled else { return }
        nativeDictation?.stop()
        let emitCapture: @MainActor @Sendable ([String: Any]) -> Void = { [weak self] value in
            guard let self, !self.isClosed, self.captureIdentity == identity else { return }
            if let type = value["type"] as? String, type == "ended" || type == "error" {
                self.captureIdentity = nil
                self.captureCancelled = true
                self.nativeDictation?.stop()
                self.nativeDictation = nil
            }
            self.emit(value)
        }
        let capture = dictationFactoryForTesting?(emitCapture) ?? DictationCapture(emit: emitCapture)
        nativeDictation = capture
        do { try capture.start(input: input) }
        catch { capture.stop(); nativeDictation = nil; emit(["type":"error", "message":"The selected microphone could not start. Check the input device and try again."]) }
    }

    private func startCapture(_ json: String) {
        guard let identity = captureIdentity, !captureCancelled, !isClosed else { return }
        captureRequestedAt = Date()
        guard let window = webView?.window else {
            emit(["type":"error", "message":"The voice window closed. Start a new call from an open window."])
            return
        }
        if !syntheticAudio && !window.occlusionState.contains(.visible) {
            window.makeKeyAndOrderFront(nil)
            NSApp.activate()
            window.orderFrontRegardless()
        }
        emit(["type":"status", "message":"Waiting for microphone permission…"])
        if let authorizationRequestForTesting {
            authorizationRequestForTesting { [weak self] permitted in
                self?.finishAuthorization(permitted, command: json, identity: identity)
            }
            return
        }
        if syntheticAudio { finishAuthorization(true, command: json, identity: identity); return }
        if AVCaptureDevice.authorizationStatus(for: .audio) == .authorized {
            finishAuthorization(true, command: json, identity: identity)
            return
        }
        AVCaptureDevice.requestAccess(for: .audio) { [weak self] permitted in
            DispatchQueue.main.async { self?.finishAuthorization(permitted, command: json, identity: identity) }
        }
    }

    private func finishAuthorization(_ permitted: Bool, command: String, identity: UUID) {
        guard !isClosed, !captureCancelled, captureIdentity == identity else { return }
        guard permitted else {
            emit(["type":"error", "message":"Microphone access is unavailable. Allow Decodex in System Settings > Privacy & Security > Microphone."])
            return
        }
        emit(["type":"status", "message":"Opening microphone…"])
        evaluate("{\"operation\":\"mute\",\"muted\":\(desiredMute)}")
        evaluate(command)
    }

    private func evaluate(_ json: String) {
        let identity = captureIdentity
        // callAsyncJavaScript passes data as an argument; SDP never becomes executable source.
        webView?.callAsyncJavaScript(
            "await window.decodexVoice.command(JSON.parse(command))",
            arguments: ["command": json], in: nil, in: .page
        ) { [weak self] result in
            guard let self, !self.isClosed, self.captureIdentity == identity, case .failure = result else { return }
            self.emit(["type": "error", "message": "The audio session could not continue."])
        }
    }

    #if DEBUG
    var mediaViewForTesting: WKWebView? { syntheticAudio ? webView : nil }
    var hasHostWindow: Bool { webView?.window != nil }
    var hasRequestedCapture: Bool { captureRequestedAt != nil }

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
        if !isReady && nativeDictation == nil && !initializationFailed && initializedAt.timeIntervalSinceNow < -10 {
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
        if value["type"] as? String == "level" {
            events.removeAll { $0.contains("\"type\":\"level\"") }
        }
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
              ["ready", "offer", "connected", "caption", "pcm", "level", "dictation_ready", "ended", "error"].contains(type)
        else { return }
        if type == "ready" {
            guard value["canCapture"] as? Bool == true else {
                emit(["type":"error", "message":"The native audio environment could not initialize."])
                return
            }
            isReady = true
            emit(["type":"media_ready", "startup_ms":Date().timeIntervalSince(initializedAt) * 1000])
            if let command = pendingCommand { pendingCommand = nil; startCapture(command) }
        } else {
            var timed = value
            if type == "dictation_ready" || type == "offer", let captureRequestedAt {
                timed["capture_ms"] = Date().timeIntervalSince(captureRequestedAt) * 1000
            }
            emit(timed)
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
        captureIdentity = nil
        nativeDictation?.stop()
        nativeDictation = nil
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
      let audioContext = null, processor = null, source = null, finishRequested = false, levelTimer = null;
      const emit = value => window.webkit.messageHandlers.voice.postMessage(value);
      const reply = document.getElementById('reply');
      function stop() {
        generation++; clearTimeout(connectionTimeout); connectionTimeout = null;
        processor?.disconnect(); source?.disconnect();
        if (processor) processor.onaudioprocess = null;
        clearInterval(levelTimer); levelTimer = null; audioContext?.close(); audioContext = null; processor = null; source = null; finishRequested = false;
        microphone?.getTracks().forEach(track => track.stop());
        channel?.close(); peer?.close();
        reply.pause(); reply.srcObject = null;
        microphone = null; channel = null; peer = null;
      }
      async function start(dictation = false, input = "") {
        stop(); const active = generation;
        let capture;
        let failureMessage = "The microphone could not open. Check its connection and Decodex microphone permission in System Settings.";
        try {
          connectionTimeout = setTimeout(() => { if (active === generation) { stop(); emit({type:'error',message:'The microphone did not open. Check its connection and try again.'}); } }, 15000);
          const knownInputs = input ? await navigator.mediaDevices.enumerateDevices() : [];
          if (active !== generation) return;
          const preferred = knownInputs.find(d => d.kind === 'audioinput' && (d.label === input || d.label.startsWith(input + ' (')));
          if (preferred) {
            capture = await navigator.mediaDevices.getUserMedia({video:false,audio:{deviceId:{exact:preferred.deviceId},echoCancellation:true,noiseSuppression:true,autoGainControl:true}});
          } else {
            capture = await navigator.mediaDevices.getUserMedia({video:false,audio:{echoCancellation:true,noiseSuppression:true,autoGainControl:true}});
          }
          if (input && !preferred) {
            const devices = await navigator.mediaDevices.enumerateDevices();
            if (active !== generation) { capture.getTracks().forEach(track=>track.stop()); return; }
            const selected = devices.find(d => d.kind === 'audioinput' && (d.label === input || d.label.startsWith(input + ' (')));
            if (!selected) { capture.getTracks().forEach(track=>track.stop()); stop(); emit({type:'error',message:'The selected microphone is unavailable. Choose another input.'}); return; }
            if (capture.getAudioTracks()[0]?.getSettings().deviceId !== selected.deviceId) {
              capture.getTracks().forEach(track=>track.stop());
              capture = await navigator.mediaDevices.getUserMedia({video:false,audio:{deviceId:{exact:selected.deviceId},echoCancellation:true,noiseSuppression:true,autoGainControl:true}});
            }
          }
          if (active !== generation) { capture.getTracks().forEach(track => track.stop()); return; }
          microphone = capture;
          capture.getAudioTracks().forEach(track => track.enabled = !muted);
          clearTimeout(connectionTimeout);
          failureMessage = "The audio runtime could not initialize. Check the input device and start a new call.";
          if (dictation) {
            audioContext = new AudioContext({sampleRate:24000});
            source = audioContext.createMediaStreamSource(capture);
            processor = audioContext.createScriptProcessor(2048,1,1);
            processor.onaudioprocess = event => {
              if (active !== generation) return;
              const samples = event.inputBuffer.getChannelData(0), pcm = new Int16Array(samples.length);
              let energy = 0;
              for (let i=0;i<samples.length;i++) { const sample=Math.max(-1,Math.min(1,samples[i])); pcm[i]=sample<0?sample*32768:sample*32767; energy+=sample*sample; }
              event.outputBuffer.getChannelData(0).fill(0);
              emit({type:'pcm',audio:btoa(String.fromCharCode(...new Uint8Array(pcm.buffer))),level:Math.min(1,Math.sqrt(energy/samples.length)*5)});
              if (finishRequested) { stop(); emit({type:'ended'}); }
            };
            source.connect(processor); processor.connect(audioContext.destination);
            await audioContext.resume();
            if (active !== generation) return;
            emit({type:'dictation_ready'}); return;
          }
          audioContext = new AudioContext();
          source = audioContext.createMediaStreamSource(capture);
          const analyser = audioContext.createAnalyser(); analyser.fftSize = 256;
          const silent = audioContext.createGain(); silent.gain.value = 0;
          source.connect(analyser); analyser.connect(silent); silent.connect(audioContext.destination);
          await audioContext.resume();
          if (active !== generation) return;
          levelTimer = setInterval(() => {
            const values = new Float32Array(analyser.fftSize); analyser.getFloatTimeDomainData(values);
            emit({type:'level',level:Math.min(1,Math.sqrt(values.reduce((sum,v)=>sum+v*v,0)/values.length)*5)});
          }, 50);
          connectionTimeout = setTimeout(() => { if (active === generation) { stop(); emit({type:'error',message:'The audio connection timed out.'}); } }, 30000);
          failureMessage = "The audio connection could not be established. Start a new call.";
          const connection = new RTCPeerConnection(); peer = connection;
          capture.getAudioTracks().forEach(track => connection.addTrack(track,capture));
          connection.ontrack = event => {
            if (active !== generation) return;
            reply.srcObject = event.streams[0] || new MediaStream([event.track]);
            reply.play().catch(() => { if (active === generation) emit({type:'error',message:'Audio playback was blocked.'}); });
          };
          const events = connection.createDataChannel('oai-events', {ordered:true}); channel = events;
          let announced = false;
          const ready = () => {
            if (active !== generation || announced || connection.connectionState !== 'connected' || events.readyState !== 'open') return;
            announced = true; clearTimeout(connectionTimeout); emit({type:'connected'});
          };
          const lost = () => {
            if (active !== generation) return;
            stop(); emit({type:'error',message:'The audio connection was lost.'});
          };
          connection.onconnectionstatechange = () => {
            if (active !== generation) return;
            if (['failed','disconnected','closed'].includes(connection.connectionState)) lost();
            else ready();
          };
          // Only the locally created ordered channel belongs to this call.
          connection.ondatachannel = event => event.channel.close();
          events.onopen = ready;
          events.onclose = lost;
          events.onerror = lost;
          channel.onmessage = event => {
            if (active !== generation || event.data.length > 65536) return;
            try {
              const value = JSON.parse(event.data);
              if (value.type === 'session.started') { /* TEST_AUDIO */ }
              // Captions are presentation data, never agent instructions.
              if (['input_transcript.added','output_transcript.added','turn.created','turn.done','turn.delta'].includes(value.type)) emit({type:'caption',event:value});
            } catch {}
          };
          const offer = await connection.createOffer();
          if (active !== generation) return;
          await connection.setLocalDescription(offer);
          if (active !== generation) return;
          if (connection.iceGatheringState !== 'complete') await new Promise(resolve => {
            const finish = () => { clearTimeout(timer); connection.removeEventListener('icegatheringstatechange', changed); resolve(); };
            const changed = () => { if (connection.iceGatheringState === 'complete') finish(); };
            const timer = setTimeout(finish, 3000);
            connection.addEventListener('icegatheringstatechange', changed);
          });
          if (active === generation) emit({type:'offer',sdp:connection.localDescription.sdp});
        } catch { if (active === generation) { stop(); emit({type:'error',message:failureMessage}); } }
      }
      window.decodexVoice = {async diagnostics() { const stats=peer ? [...(await peer.getStats()).values()].filter(s=>['outbound-rtp','inbound-rtp'].includes(s.type)).map(s=>({type:s.type,bytesSent:s.bytesSent,bytesReceived:s.bytesReceived,packetsSent:s.packetsSent})) : []; return {muted:microphone?.getAudioTracks().every(track=>!track.enabled),audio:window.testAudioContext?.state,stats,connection:peer?.connectionState,ice:peer?.iceConnectionState,gathering:peer?.iceGatheringState,signaling:peer?.signalingState,channel:channel?.readyState,localCandidates:(peer?.localDescription?.sdp.match(/a=candidate:/g)||[]).length,remoteCandidates:(peer?.remoteDescription?.sdp.match(/a=candidate:/g)||[]).length}; },async command(value) {
        if (value.operation === 'start') return start(false,value.input || '');
        if (value.operation === 'dictate') return start(true,value.input || '');
        if (value.operation === 'finish') { if (processor) { finishRequested = true; const active=generation; setTimeout(()=>{if(active===generation){stop();emit({type:'ended'});}},1000); } else { stop();emit({type:'ended'}); } return; }
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
public func decodexVoiceMediaABIVersion() -> UInt32 { 2 }

@_cdecl("decodex_voice_media_create")
public func decodexVoiceMediaCreate(_ nativeView: UnsafeMutableRawPointer?) -> UnsafeMutableRawPointer? {
    guard Thread.isMainThread, let nativeView else { return nil }
    let viewAddress = UInt(bitPattern: nativeView)
    let address = MainActor.assumeIsolated {
        let view = Unmanaged<NSView>.fromOpaque(UnsafeMutableRawPointer(bitPattern: viewAddress)!).takeUnretainedValue()
        guard let window = view.window else { return UInt(0) }
        return UInt(bitPattern: Unmanaged.passRetained(VoiceMediaHost(hostWindow: window)).toOpaque())
    }
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
