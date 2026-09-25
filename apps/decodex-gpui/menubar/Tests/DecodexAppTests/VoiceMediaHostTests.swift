import AppKit
import Darwin
import XCTest

@testable import DecodexApp

@MainActor
final class VoiceMediaHostTests: XCTestCase {
    func testNativeDictationFinishFlushesButSupersededCallbacksAreIgnored() async throws {
        _ = NSApplication.shared
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 100, height: 100), styleMask: [.titled], backing: .buffered, defer: false)
        window.makeKeyAndOrderFront(nil)
        defer { window.orderOut(nil) }
        var captures: [DictationProbe] = []
        var authorizations: [@MainActor (Bool) -> Void] = []
        let host = VoiceMediaHost(syntheticAudioForTesting: true, hostWindow: window,
                                  authorizationRequestForTesting: { authorizations.append($0) },
                                  dictationFactoryForTesting: { emit in
            let capture = DictationProbe(emit: emit)
            captures.append(capture)
            return capture
        })
        defer { host.close() }
        _ = try await event(from: host, type: "media_ready")
        XCTAssertTrue(host.command(#"{"operation":"dictate"}"#))
        authorizations[0](true)
        XCTAssertEqual(captures.count, 1)
        XCTAssertTrue(host.command(#"{"operation":"finish"}"#))
        XCTAssertEqual(captures[0].finishes, 1)
        captures[0].emit(["type":"pcm", "audio":"final"])
        captures[0].emit(["type":"ended"])
        let finalPCM = try await event(from: host, type: "pcm")
        XCTAssertEqual(finalPCM["audio"] as? String, "final")
        _ = try await event(from: host, type: "ended")
        captures[0].emit(["type":"error", "message":"late"])
        XCTAssertNil(host.poll())

        for operation in ["dictate", "start"] {
            XCTAssertTrue(host.command(#"{"operation":"dictate"}"#))
            authorizations.last?(true)
            let old = try XCTUnwrap(captures.last)
            XCTAssertTrue(host.command(#"{"operation":"finish"}"#))
            XCTAssertTrue(host.command("{\"operation\":\"\(operation)\"}"))
            XCTAssertGreaterThan(old.stops, 0, "Superseding must stop native audio before permission resolves")
            while host.poll() != nil {}
            for type in ["pcm", "level", "ended", "error"] {
                old.emit(["type":type, "audio":"stale", "message":"stale"])
            }
            XCTAssertNil(host.poll(), "Old native events must not enter the new capture")
            authorizations.last?(true)
            if operation == "start" { _ = try await event(from: host, type: "offer") }
            XCTAssertTrue(host.command(#"{"operation":"stop"}"#))
            _ = try await event(from: host, type: "ended")
        }
        host.close()
        captures.last?.emit(["type":"pcm", "audio":"closed"])
        XCTAssertNil(host.poll())
    }

    func testDelayedAuthorizationCannotRestartAnOldCapture() async throws {
        _ = NSApplication.shared
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 100, height: 100), styleMask: [.titled], backing: .buffered, defer: false)
        window.makeKeyAndOrderFront(nil)
        defer { window.orderOut(nil) }
        var authorizations: [@MainActor (Bool) -> Void] = []
        let host = VoiceMediaHost(syntheticAudioForTesting: true, hostWindow: window,
                                  authorizationRequestForTesting: { authorizations.append($0) })
        defer { host.close() }
        _ = try await event(from: host, type: "media_ready")
        XCTAssertTrue(host.command(#"{"operation":"start"}"#))
        XCTAssertEqual(authorizations.count, 1)
        XCTAssertTrue(host.command(#"{"operation":"stop"}"#))
        _ = try await event(from: host, type: "ended")
        XCTAssertTrue(host.command(#"{"operation":"start"}"#))
        XCTAssertEqual(authorizations.count, 2)
        while host.poll() != nil {}
        authorizations[0](false)
        authorizations[0](true)
        XCTAssertNil(host.poll(), "Old permission results must not emit an error or open capture")
        authorizations[1](true)
        _ = try await event(from: host, type: "offer")
        host.close()
        authorizations[1](true)
        XCTAssertNil(host.poll())
    }

    func testNativeWebRTCProducesOfferAndStopsWithoutMicrophoneAccess() async throws {
        _ = NSApplication.shared
        NSApp.setActivationPolicy(.regular)
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 100, height: 100), styleMask: [.titled], backing: .buffered, defer: false)
        window.title = "Decodex Voice Test"
        window.makeKeyAndOrderFront(nil)
        NSApp.activate()
        defer { window.orderOut(nil) }
        let host = VoiceMediaHost(syntheticAudioForTesting: true, hostWindow: window)
        defer { host.close() }
        XCTAssertTrue(host.command(#"{"operation":"mute","muted":true}"#))
        XCTAssertTrue(host.command(#"{"operation":"start"}"#))
        let offer = try await event(from: host, type: "offer")
        let sdp = try XCTUnwrap(offer["sdp"] as? String)
        XCTAssertTrue(sdp.contains("m=audio"))
        XCTAssertTrue(sdp.contains("m=application"))
        let diagnostics = await host.connectionDiagnostics()
        let state = try JSONSerialization.jsonObject(with: Data(diagnostics.utf8)) as? [String: Any]
        XCTAssertEqual(state?["muted"] as? Bool, true, "Mute requested before capture must apply to the first track")
        XCTAssertTrue(host.command(#"{"operation":"mute","muted":true}"#))
        XCTAssertTrue(host.command(#"{"operation":"stop"}"#))
        _ = try await event(from: host, type: "ended")
        host.close()
        XCTAssertFalse(host.command(#"{"operation":"start"}"#))
    }

    func testIncomingAudioStartsBeforeCaptionsAndRejectsOldCallTracks() async throws {
        _ = NSApplication.shared
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 100, height: 100), styleMask: [.titled], backing: .buffered, defer: false)
        window.makeKeyAndOrderFront(nil)
        defer { window.orderOut(nil) }
        let host = VoiceMediaHost(syntheticAudioForTesting: true, hostWindow: window)
        defer { host.close() }
        _ = try await event(from: host, type: "media_ready")
        let view = try XCTUnwrap(host.mediaViewForTesting)
        func evaluate(_ script: String) async throws -> String {
            try await withCheckedThrowingContinuation { continuation in
                view.callAsyncJavaScript(script, arguments: [:], in: nil, in: .page) { result in
                    do { continuation.resume(returning: try XCTUnwrap(result.get() as? String)) }
                    catch { continuation.resume(throwing: error) }
                }
            }
        }
        _ = try await evaluate(#"""
            window.fixturePeers = []; window.fixturePlayCount = 0;
            const NativePeer = window.RTCPeerConnection;
            window.RTCPeerConnection = function(...args) {
                const peer = new NativePeer(...args); fixturePeers.push(peer); return peer;
            };
            // Observe the production play request without claiming audible output or ICE.
            document.getElementById('reply').play = async () => { fixturePlayCount++; };
            return 'installed';
            """#)
        XCTAssertTrue(host.command(#"{"operation":"start"}"#))
        _ = try await event(from: host, type: "offer")
        let first = try await evaluate(#"""
            const stream = window.testAudioDestination.stream;
            fixturePeers[0].ontrack({streams:[stream],track:stream.getAudioTracks()[0]});
            if (fixturePlayCount !== 1 || document.getElementById('reply').srcObject !== stream) throw new Error('Audio waited for captions');
            return 'playing-before-any-caption';
            """#)
        XCTAssertEqual(first, "playing-before-any-caption")
        XCTAssertTrue(host.command(#"{"operation":"start"}"#))
        _ = try await event(from: host, type: "offer")
        let replacement = try await evaluate(#"""
            const stream = window.testAudioDestination.stream;
            const event = {streams:[stream],track:stream.getAudioTracks()[0]};
            fixturePeers[0].ontrack(event);
            if (fixturePlayCount !== 1 || document.getElementById('reply').srcObject !== null) throw new Error('Old call revived playback');
            fixturePeers[1].ontrack(event);
            if (fixturePlayCount !== 2 || document.getElementById('reply').srcObject !== stream) throw new Error('New call waited for captions');
            return 'only-current-call-plays';
            """#)
        XCTAssertEqual(replacement, "only-current-call-plays")
        XCTAssertTrue(host.command(#"{"operation":"stop"}"#))
        _ = try await event(from: host, type: "ended")
    }

    func testQueuedFramelessCaptionFinalPrecedesStopAndOldChannelIsIgnored() async throws {
        _ = NSApplication.shared
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 100, height: 100), styleMask: [.titled], backing: .buffered, defer: false)
        window.makeKeyAndOrderFront(nil)
        defer { window.orderOut(nil) }
        let host = VoiceMediaHost(syntheticAudioForTesting: true, hostWindow: window)
        defer { host.close() }
        _ = try await event(from: host, type: "media_ready")
        let view = try XCTUnwrap(host.mediaViewForTesting)
        func evaluate(_ script: String) async throws -> String {
            try await withCheckedThrowingContinuation { continuation in
                view.callAsyncJavaScript(script, arguments: [:], in: nil, in: .page) { result in
                    do { continuation.resume(returning: try XCTUnwrap(result.get() as? String)) }
                    catch { continuation.resume(throwing: error) }
                }
            }
        }
        _ = try await evaluate(#"""
            window.fixtureChannels = [];
            const create = RTCPeerConnection.prototype.createDataChannel;
            RTCPeerConnection.prototype.createDataChannel = function(...args) {
                const channel = create.apply(this, args); fixtureChannels.push(channel); return channel;
            };
            return 'installed';
            """#)
        XCTAssertTrue(host.command(#"{"operation":"start"}"#))
        _ = try await event(from: host, type: "offer")
        _ = try await evaluate(#"""
            // Exercise the production message callback and WebKit bridge, without claiming ICE.
            for (const frame of [
                {type:'input_transcript.added',item:{text:'uncorrected'}},
                {type:'output_transcript.added',item:{text:'reply'}},
                {type:'turn.done',turn:{role:'user',transcript:'Corrected final.'}}
            ]) fixtureChannels[0].onmessage({data:JSON.stringify(frame)});
            await window.decodexVoice.command({operation:'stop'});
            fixtureChannels[0].onmessage({data:JSON.stringify({type:'input_transcript.added',item:{text:'stale'}})});
            return 'stopped';
            """#)
        var captions: [[String: Any]] = []
        var ended = false
        let deadline = Date().addingTimeInterval(5)
        while !ended && Date() < deadline {
            if let pointer = host.poll() {
                let value = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(String(cString: pointer).utf8)) as? [String: Any])
                if value["type"] as? String == "caption" { captions.append(try XCTUnwrap(value["event"] as? [String: Any])) }
                ended = value["type"] as? String == "ended"
            } else { try await Task.sleep(for: .milliseconds(10)) }
        }
        XCTAssertTrue(ended)
        XCTAssertEqual(captions.compactMap { $0["type"] as? String }, ["input_transcript.added", "output_transcript.added", "turn.done"])
        XCTAssertEqual((captions.last?["turn"] as? [String: Any])?["transcript"] as? String, "Corrected final.")
        XCTAssertNil(host.poll(), "The retired channel must not publish a stale caption")
    }

    func testStartupFailuresIdentifyTheirStageWithoutNativeDetails() async throws {
        _ = NSApplication.shared
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 100, height: 100), styleMask: [.titled], backing: .buffered, defer: false)
        window.makeKeyAndOrderFront(nil)
        defer { window.orderOut(nil) }
        let cases = [
            ("AudioContext.prototype.createMediaStreamDestination = () => { throw new Error('private-device-path'); };", "The microphone could not open."),
            ("AudioContext.prototype.createMediaStreamSource = () => { throw new Error('private-audio-runtime'); };", "The audio runtime could not initialize."),
            ("RTCPeerConnection.prototype.createOffer = async () => { throw new Error('private-sdp'); };", "The audio connection could not be established.")
        ]
        for (script, expected) in cases {
            let host = VoiceMediaHost(syntheticAudioForTesting: true, hostWindow: window)
            defer { host.close() }
            _ = try await event(from: host, type: "media_ready")
            let view = try XCTUnwrap(host.mediaViewForTesting)
            let _: String = try await withCheckedThrowingContinuation { continuation in
                view.callAsyncJavaScript(script + " return 'installed';", arguments: [:], in: nil, in: .page) { result in
                    do { continuation.resume(returning: try XCTUnwrap(result.get() as? String)) }
                    catch { continuation.resume(throwing: error) }
                }
            }
            XCTAssertTrue(host.command(#"{"operation":"start"}"#))
            let failure = try await event(from: host, type: "error")
            let message = try XCTUnwrap(failure["message"] as? String)
            XCTAssertTrue(message.hasPrefix(expected), message)
            XCTAssertFalse(message.contains("private-"))
            let diagnostics = await host.connectionDiagnostics()
            let state = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(diagnostics.utf8)) as? [String: Any])
            XCTAssertNil(state["connection"], "Failed startup must release its peer")
            XCTAssertNil(state["muted"], "Failed startup must release microphone tracks")
            host.close()
        }
    }

    func testWebRTCMuteKeepsPacketsFlowingAndUnmuteRestoresAudio() async throws {
        guard ProcessInfo.processInfo.environment["DECODEX_NATIVE_WEBRTC_LOOPBACK"] == "1" else {
            throw XCTSkip("Requires an interactive WebKit host with usable local ICE candidates")
        }
        _ = NSApplication.shared
        NSApp.setActivationPolicy(.regular)
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 100, height: 100), styleMask: [.titled], backing: .buffered, defer: false)
        window.makeKeyAndOrderFront(nil)
        NSApp.activate()
        defer { window.orderOut(nil) }
        let host = VoiceMediaHost(syntheticAudioForTesting: true, hostWindow: window)
        defer { host.close() }
        XCTAssertTrue(host.command(#"{"operation":"start"}"#))
        let offer = try await event(from: host, type: "offer")
        let view = try XCTUnwrap(host.mediaViewForTesting)
        func evaluate(_ script: String, arguments: [String: Any] = [:]) async throws -> String {
            try await withCheckedThrowingContinuation { continuation in
                view.callAsyncJavaScript(script, arguments: arguments, in: nil, in: .page) { result in
                    do { continuation.resume(returning: try XCTUnwrap(result.get() as? String)) }
                    catch { continuation.resume(throwing: error) }
                }
            }
        }
        let answer = try await evaluate(#"""
            const peer = new RTCPeerConnection(); window.fixturePeer = peer;
            const context = window.testAudioContext;
            const tone = context.createOscillator(); tone.frequency.value = 440;
            tone.connect(window.testAudioDestination); tone.start();
            window.fixtureTone = tone;
            const analyser = context.createAnalyser(); analyser.fftSize = 2048;
            window.fixtureAnalyser = analyser;
            const silent = context.createGain(); silent.gain.value = 0;
            analyser.connect(silent); silent.connect(context.destination);
            peer.ontrack = event => {
                const source = context.createMediaStreamSource(new MediaStream([event.track]));
                source.connect(analyser); window.fixtureSource = source;
            };
            peer.ondatachannel = event => { window.fixtureChannel = event.channel; };
            await context.resume();
            await peer.setRemoteDescription({type:'offer',sdp:offer});
            await peer.setLocalDescription(await peer.createAnswer());
            if (peer.iceGatheringState !== 'complete') await new Promise((resolve,reject) => {
                const timer = setTimeout(()=>reject(new Error('fixture ICE timeout')),5000);
                peer.onicegatheringstatechange = () => {
                    if(peer.iceGatheringState === 'complete'){clearTimeout(timer);resolve();}
                };
            });
            return peer.localDescription.sdp;
            """#, arguments: ["offer": try XCTUnwrap(offer["sdp"] as? String)])
        let command = try JSONSerialization.data(withJSONObject: ["operation":"answer", "sdp": answer])
        XCTAssertTrue(host.command(String(decoding: command, as: UTF8.self)))
        _ = try await event(from: host, type: "connected")
        func sample() async throws -> [String: Any] {
            let encoded = try await evaluate(#"""
                const values=new Float32Array(fixtureAnalyser.fftSize);
                fixtureAnalyser.getFloatTimeDomainData(values);
                const stats=[...(await fixturePeer.getStats()).values()];
                const incoming=stats.find(s=>s.type==='inbound-rtp' && s.kind==='audio');
                return JSON.stringify({energy:values.reduce((n,v)=>n+v*v,0)/values.length,
                    packets:incoming?.packetsReceived || 0,connection:fixturePeer.connectionState});
                """#)
            return try XCTUnwrap(JSONSerialization.jsonObject(with: Data(encoded.utf8)) as? [String: Any])
        }
        try await Task.sleep(for: .seconds(1))
        let audible = try await sample()
        XCTAssertGreaterThan(try XCTUnwrap(audible["energy"] as? Double), 0.01)
        XCTAssertTrue(host.command(#"{"operation":"mute","muted":true}"#))
        try await Task.sleep(for: .seconds(1))
        let muted = try await sample()
        try await Task.sleep(for: .seconds(2))
        let sustained = try await sample()
        XCTAssertEqual(sustained["connection"] as? String, "connected")
        XCTAssertLessThan(try XCTUnwrap(sustained["energy"] as? Double), 0.000001)
        XCTAssertGreaterThan(try XCTUnwrap(sustained["packets"] as? Int), try XCTUnwrap(muted["packets"] as? Int))
        XCTAssertTrue(host.command(#"{"operation":"mute","muted":false}"#))
        try await Task.sleep(for: .seconds(1))
        let restored = try await sample()
        XCTAssertGreaterThan(try XCTUnwrap(restored["energy"] as? Double), 0.01)
        _ = try await evaluate("fixtureTone.stop();fixturePeer.close();return 'closed';")
        XCTAssertTrue(host.command(#"{"operation":"stop"}"#))
        _ = try await event(from: host, type: "ended")
    }

    func testPreparedHostPreservesPointerEventDelivery() async throws {
        _ = NSApplication.shared
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 100, height: 100), styleMask: [.titled], backing: .buffered, defer: false)
        let sink = HoverProbeView(frame: NSRect(x: 0, y: 0, width: 100, height: 100))
        window.contentView?.addSubview(sink)
        window.acceptsMouseMovedEvents = true
        window.makeKeyAndOrderFront(nil)
        window.makeFirstResponder(sink)
        defer { window.orderOut(nil) }
        let host = VoiceMediaHost(syntheticAudioForTesting: true, hostWindow: window)
        defer { host.close() }
        _ = try await event(from: host, type: "media_ready")
        XCTAssertTrue(window.acceptsMouseMovedEvents, "Preparing audio must not disable hover events")
        XCTAssertTrue(window.firstResponder === sink, "Preparing audio must not steal the GPUI responder")
        let moved = try XCTUnwrap(NSEvent.mouseEvent(with: .mouseMoved, location: NSPoint(x: 50, y: 50), modifierFlags: [], timestamp: 0, windowNumber: window.windowNumber, context: nil, eventNumber: 0, clickCount: 0, pressure: 0))
        window.sendEvent(moved)
        XCTAssertEqual(sink.moves, 1)
    }

    func testPreparedHostDoesNotCaptureUntilRequested() async throws {
        _ = NSApplication.shared
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 100, height: 100), styleMask: [.titled], backing: .buffered, defer: false)
        window.makeKeyAndOrderFront(nil)
        defer { window.orderOut(nil) }
        let host = VoiceMediaHost(syntheticAudioForTesting: true, hostWindow: window)
        defer { host.close() }
        let initialized = try await event(from: host, type: "media_ready")
        XCTAssertFalse(host.hasRequestedCapture)
        XCTAssertTrue(host.command(#"{"operation":"dictate"}"#))
        let capture = try await event(from: host, type: "dictation_ready")
        XCTAssertTrue(host.hasRequestedCapture)
        XCTAssertNotNil(capture["capture_ms"])
        print("Audio preparation ms: \(initialized["startup_ms"] ?? "missing"); warm synthetic capture ms: \(capture["capture_ms"] ?? "missing")")
        XCTAssertTrue(host.command(#"{"operation":"finish"}"#))
        _ = try await event(from: host, type: "ended")
    }

    func testDictationCapturesPCMAndFlushesBeforeEnding() async throws {
        _ = NSApplication.shared
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 100, height: 100), styleMask: [.titled], backing: .buffered, defer: false)
        window.makeKeyAndOrderFront(nil)
        NSApp.activate()
        defer { window.orderOut(nil) }
        let host = VoiceMediaHost(syntheticAudioForTesting: true, hostWindow: window)
        defer { host.close() }
        XCTAssertTrue(host.command(#"{"operation":"dictate"}"#))
        _ = try await event(from: host, type: "dictation_ready")
        let frame = try await event(from: host, type: "pcm")
        let encoded = try XCTUnwrap(frame["audio"] as? String)
        let pcm = try XCTUnwrap(Data(base64Encoded: encoded))
        XCTAssertEqual(pcm.count, 4096)
        XCTAssertNotNil(frame["level"] as? Double)
        XCTAssertTrue(host.command(#"{"operation":"finish"}"#))
        _ = try await event(from: host, type: "ended")
    }

    func testMediaABIBindsTheSuppliedWindowAndRejectsDetachedViews() throws {
        _ = NSApplication.shared
        XCTAssertEqual(decodexVoiceMediaABIVersion(), 2)
        let detached = NSView()
        XCTAssertNil(decodexVoiceMediaCreate(Unmanaged.passUnretained(detached).toOpaque()))
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 100, height: 100), styleMask: [.borderless], backing: .buffered, defer: false)
        let view = try XCTUnwrap(window.contentView)
        let pointer = try XCTUnwrap(decodexVoiceMediaCreate(Unmanaged.passUnretained(view).toOpaque()))
        defer { decodexVoiceMediaDestroy(pointer) }
        let host = Unmanaged<VoiceMediaHost>.fromOpaque(pointer).takeUnretainedValue()
        XCTAssertTrue(host.hasHostWindow)
    }

    func testInputDiscoveryDoesNotStartCapture() throws {
        _ = NSApplication.shared
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 100, height: 100), styleMask: [.titled], backing: .buffered, defer: false)
        let host = VoiceMediaHost(hostWindow: window)
        defer { host.close() }
        XCTAssertTrue(host.command(#"{"operation":"devices"}"#))
        let pointer = try XCTUnwrap(host.poll())
        let value = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(String(cString:pointer).utf8)) as? [String:Any])
        XCTAssertEqual(value["type"] as? String, "devices")
        XCTAssertNotNil(value["inputs"] as? [String])
    }

    func testVisibleWindowFallbackBindsMediaWithoutKeyWindow() {
        _ = NSApplication.shared
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 100, height: 100), styleMask: [.borderless], backing: .buffered, defer: false)
        window.orderFront(nil)
        defer { window.orderOut(nil) }
        let host = VoiceMediaHost(syntheticAudioForTesting: true)
        defer { host.close() }
        XCTAssertTrue(host.hasHostWindow, "A visible native window must own the media view")
    }

    func testOptInNativeSubscriptionCall() async throws {
        let previousSignal = signal(SIGPIPE, SIG_IGN)
        defer { signal(SIGPIPE, previousSignal) }
        let environment = ProcessInfo.processInfo.environment
        guard let helper = environment["DECODEX_VOICE_SIGNAL_HELPER"],
              let root = environment["DECODEX_VOICE_SERVICE_ROOT"],
              let work = environment["DECODEX_VOICE_WORK_ID"],
              let sample = environment["DECODEX_VOICE_SAMPLE"] else {
            throw XCTSkip("Explicit voice qualification profile and synthetic audio required")
        }
        _ = NSApplication.shared
        NSApp.setActivationPolicy(.regular)
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 100, height: 100), styleMask: [.titled], backing: .buffered, defer: false)
        window.title = "Decodex Voice Test"
        window.makeKeyAndOrderFront(nil)
        NSApp.activate()
        defer { window.orderOut(nil) }
        let host = VoiceMediaHost(syntheticAudioForTesting: true, sampleForTesting: try Data(contentsOf: URL(fileURLWithPath: sample)), hostWindow: window)
        defer { host.close() }
        XCTAssertTrue(host.command(#"{"operation":"start"}"#))
        let offer = try await event(from: host, type: "offer")
        let description = try XCTUnwrap(offer["sdp"] as? String)
        let process = Process(), input = Pipe(), output = Pipe()
        process.executableURL = URL(fileURLWithPath: helper)
        process.arguments = [root, work]
        process.standardInput = input
        process.standardOutput = output
        process.standardError = FileHandle.standardError
        try process.run()
        defer {
            try? input.fileHandleForWriting.write(contentsOf: Data("stop\n".utf8))
            try? input.fileHandleForWriting.close()
            if process.isRunning { process.terminate() }
        }
        var encoded = try JSONSerialization.data(withJSONObject: description, options: .fragmentsAllowed)
        encoded.append(10)
        try input.fileHandleForWriting.write(contentsOf: encoded)
        let readHandle = output.fileHandleForReading
        let response = await Task.detached {
            var bytes = Data()
            while bytes.count <= 70_000 {
                let chunk = readHandle.availableData
                if chunk.isEmpty { break }
                bytes.append(chunk)
                if bytes.contains(10) { break }
            }
            return bytes
        }.value
        let answer = try XCTUnwrap(JSONSerialization.jsonObject(with: response, options: .fragmentsAllowed) as? String)
        let command = try JSONSerialization.data(withJSONObject: ["operation":"answer", "sdp":answer])
        XCTAssertTrue(host.command(String(decoding: command, as: UTF8.self)))
        _ = try await event(from: host, type: "connected")
        let deadline = Date().addingTimeInterval(55)
        var heardUser = false, heardAssistant = false, finalizedUser = false
        while Date() < deadline && !(heardUser && heardAssistant && finalizedUser) {
            if let pointer = host.poll() {
                let value = try JSONSerialization.jsonObject(with: Data(String(cString: pointer).utf8)) as? [String: Any]
                if value?["type"] as? String == "error" { XCTFail("Native voice failed after connection"); break }
                let caption = value?["event"] as? [String: Any]
                heardUser = heardUser || caption?["type"] as? String == "input_transcript.added"
                heardAssistant = heardAssistant || caption?["type"] as? String == "output_transcript.added"
                let turn = caption?["turn"] as? [String: Any]
                finalizedUser = finalizedUser || (caption?["type"] as? String == "turn.done" && turn?["role"] as? String == "user")
            }
            try await Task.sleep(for: .milliseconds(20))
        }
        let diagnostics = await host.connectionDiagnostics()
        XCTAssertTrue(heardUser, "No live input transcript from synthetic speech: \(diagnostics)")
        XCTAssertTrue(heardAssistant, "No live assistant reply")
        XCTAssertTrue(finalizedUser, "No final user transcript")
        XCTAssertTrue(host.command(#"{"operation":"stop"}"#))
        _ = try await event(from: host, type: "ended")
        try input.fileHandleForWriting.write(contentsOf: Data("stop\n".utf8))
        for _ in 0..<100 where process.isRunning { try await Task.sleep(for: .milliseconds(20)) }
        XCTAssertFalse(process.isRunning, "Signaling helper did not close")
    }

    private func event(from host: VoiceMediaHost, type: String) async throws -> [String: Any] {
        let deadline = Date().addingTimeInterval(32)
        while Date() < deadline {
            if let pointer = host.poll() {
                let data = Data(String(cString: pointer).utf8)
                let event = try XCTUnwrap(JSONSerialization.jsonObject(with: data) as? [String: Any])
                if event["type"] as? String == type { return event }
                if event["type"] as? String == "error" {
                    XCTFail(event["message"] as? String ?? "Native audio failed")
                    throw NSError(domain: "VoiceMediaHostTests", code: 1)
                }
            }
            try await Task.sleep(for: .milliseconds(20))
        }
        XCTFail("No native media event: \(type); \(await host.connectionDiagnostics())")
        throw NSError(domain: "VoiceMediaHostTests", code: 2)
    }
}

@MainActor
private final class DictationProbe: DictationCapturing {
    let emit: @MainActor @Sendable ([String: Any]) -> Void
    var finishes = 0
    var stops = 0
    init(emit: @escaping @MainActor @Sendable ([String: Any]) -> Void) { self.emit = emit }
    func start(input: String) throws {}
    func finish() { finishes += 1 }
    func stop() { stops += 1 }
}

@MainActor
private final class HoverProbeView: NSView {
    var moves = 0
    override var acceptsFirstResponder: Bool { true }
    override func mouseMoved(with event: NSEvent) { moves += 1 }
}
