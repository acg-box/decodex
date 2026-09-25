import JavaScriptCore
import XCTest

@testable import DecodexApp

/// Execute the production media document with controlled peer callbacks.
/// These tests do not claim device or network acceptance.
@MainActor
final class VoiceReadinessTests: XCTestCase {
    func testReadinessRequiresPeerAndOrderedChannelInEitherOrder() throws {
        for peerFirst in [true, false] {
            let context = try fixture()
            XCTAssertTrue(value("fixture.peer.channel.ordered", context).toBool())
            run(peerFirst ? "fixture.connect()" : "fixture.open()", context)
            XCTAssertEqual(value("fixture.count('connected')", context).toInt32(), 0)
            XCTAssertEqual(value("fixture.timeouts.size", context).toInt32(), 1, "Channel negotiation must retain its deadline")
            run(peerFirst ? "fixture.open()" : "fixture.connect()", context)
            XCTAssertEqual(value("fixture.count('connected')", context).toInt32(), 1)
            XCTAssertEqual(value("fixture.timeouts.size", context).toInt32(), 0)
            run("fixture.open(); fixture.connect()", context)
            XCTAssertEqual(value("fixture.count('connected')", context).toInt32(), 1)
            run("fixture.remote = {closed:false, close(){this.closed=true}}; fixture.peer.ondatachannel({channel:fixture.remote})", context)
            XCTAssertTrue(value("fixture.remote.closed", context).toBool())
            XCTAssertEqual(value("fixture.peer.channel.readyState", context).toString(), "open")
        }
    }

    func testLostChannelStopsCaptureAndOldCallbacksCannotAffectNextCall() throws {
        for failure in ["onclose", "onerror"] {
            let context = try fixture()
            run("fixture.connect(); fixture.open(); fixture.old = fixture.peer; fixture.old.channel." + failure + "()", context)
            XCTAssertEqual(value("fixture.count('error')", context).toInt32(), 1)
            XCTAssertTrue(value("fixture.old.closed && fixture.track.stopped", context).toBool())
            run("window.decodexVoice.command({operation:'start'})", context)
            XCTAssertEqual(value("fixture.count('offer')", context).toInt32(), 2)
            run("fixture.old.channel.onopen(); fixture.old.channel.onclose(); fixture.old.onconnectionstatechange()", context)
            XCTAssertEqual(value("fixture.count('connected')", context).toInt32(), 1)
            XCTAssertEqual(value("fixture.count('error')", context).toInt32(), 1)
            XCTAssertFalse(value("fixture.peer.closed", context).toBool())
            run("fixture.connect(); fixture.open(); window.decodexVoice.command({operation:'stop'})", context)
            XCTAssertEqual(value("fixture.count('connected')", context).toInt32(), 2)
            XCTAssertEqual(value("fixture.count('error')", context).toInt32(), 1)
            XCTAssertEqual(value("fixture.count('ended')", context).toInt32(), 1)
        }
    }

    func testStoppedAudioInitializationCannotReplaceTheNextCall() throws {
        for operation in ["start", "dictate"] {
            let context = try fixture()
            run("fixture.holdResume=true; window.decodexVoice.command({operation:'" + operation + "'})", context)
            XCTAssertTrue(value("typeof fixture.resumePending === 'function'", context).toBool())
            run("window.decodexVoice.command({operation:'stop'}); fixture.holdResume=false; window.decodexVoice.command({operation:'start'})", context)
            run("fixture.current=fixture.peer", context)
            XCTAssertEqual(value("fixture.count('offer')", context).toInt32(), 2)
            run("fixture.resumePending()", context)
            XCTAssertTrue(value("fixture.peer === fixture.current && !fixture.current.closed", context).toBool())
            XCTAssertEqual(value("fixture.count('offer')", context).toInt32(), 2)
            XCTAssertEqual(value("fixture.count('dictation_ready')", context).toInt32(), 0)
            XCTAssertEqual(value("fixture.timeouts.size", context).toInt32(), 1)
            run("fixture.connect(); fixture.open()", context)
            XCTAssertEqual(value("fixture.count('connected')", context).toInt32(), 1)
        }
    }

    func testStoppedOfferDoesNotSetDescriptionOrEmitForTheNextCall() throws {
        let context = try fixture()
        run("fixture.holdOffer=true; window.decodexVoice.command({operation:'start'})", context)
        run("fixture.old=fixture.peer", context)
        run("window.decodexVoice.command({operation:'stop'}); fixture.holdOffer=false; window.decodexVoice.command({operation:'start'})", context)
        run("fixture.current=fixture.peer", context)
        run("fixture.offerPending({type:'offer',sdp:'stale-offer'})", context)
        XCTAssertTrue(value("fixture.old.closed && !fixture.old.localDescription", context).toBool())
        XCTAssertTrue(value("fixture.peer === fixture.current && !fixture.current.closed", context).toBool())
        XCTAssertEqual(value("fixture.count('offer')", context).toInt32(), 2)
        XCTAssertEqual(value("fixture.count('error')", context).toInt32(), 0)
    }

    func testStoppedDeviceLookupCannotOpenAnotherMicrophone() throws {
        let context = try fixture()
        run("fixture.holdEnumeration=true; window.decodexVoice.command({operation:'start',input:'Selected'})", context)
        run("window.decodexVoice.command({operation:'stop'}); fixture.holdEnumeration=false; window.decodexVoice.command({operation:'start'})", context)
        run("fixture.enumerationPending([])", context)
        XCTAssertEqual(value("fixture.microphoneCalls", context).toInt32(), 2)
        XCTAssertEqual(value("fixture.count('offer')", context).toInt32(), 2)
        XCTAssertEqual(value("fixture.count('error')", context).toInt32(), 0)
        run("window.decodexVoice.command({operation:'start',input:'Unavailable'})", context)
        XCTAssertTrue(value("fixture.track.stopped", context).toBool())
        XCTAssertEqual(value("fixture.count('error')", context).toInt32(), 1)
        XCTAssertEqual(value("fixture.timeouts.size", context).toInt32(), 0)
    }

    private func fixture() throws -> JSContext {
        let context = try XCTUnwrap(JSContext())
        run(Self.environment, context)
        let script = try XCTUnwrap(VoiceMediaHost.document.components(separatedBy: "<script>").last?
            .components(separatedBy: "</script>").first)
        run(script, context)
        run("window.decodexVoice.command({operation:'start'})", context)
        XCTAssertEqual(value("fixture.count('offer')", context).toInt32(), 1)
        XCTAssertEqual(value("fixture.count('error')", context).toInt32(), 0)
        return context
    }

    private func run(_ script: String, _ context: JSContext) {
        _ = value(script, context)
    }

    private func value(_ script: String, _ context: JSContext) -> JSValue {
        context.exception = nil
        let result = context.evaluateScript(script)
        XCTAssertNil(context.exception, "JavaScript fixture failed: \(context.exception?.toString() ?? "")")
        return result ?? JSValue(undefinedIn: context)
    }

    private static let environment = #"""
    const fixture = {
      events: [], timeouts: new Set(), nextTimer: 0, microphoneCalls:0, track: {enabled:true, stopped:false, stop(){this.stopped=true}},
      count(type){return this.events.filter(event=>event.type===type).length},
      connect(){this.peer.connectionState='connected'; this.peer.onconnectionstatechange()},
      open(){this.peer.channel.readyState='open'; this.peer.channel.onopen()}
    };
    const window = {webkit:{messageHandlers:{voice:{postMessage(value){fixture.events.push(value)}}}}, addEventListener(){}};
    const document = {getElementById(){return {pause(){},play(){return Promise.resolve()},srcObject:null}}};
    const navigator = {mediaDevices:{async enumerateDevices(){if(fixture.holdEnumeration) return await new Promise(resolve=>{fixture.enumerationPending=resolve}); return []},async getUserMedia(){fixture.microphoneCalls++;fixture.track.stopped=false; return {
      getAudioTracks(){return [fixture.track]}, getTracks(){return [fixture.track]}
    }}}};
    const setTimeout = ()=>{const id=++fixture.nextTimer;fixture.timeouts.add(id);return id};
    const clearTimeout = id=>fixture.timeouts.delete(id), setInterval = ()=>1, clearInterval = ()=>{};
    class AudioContext {
      constructor(){this.destination={}}
      createMediaStreamSource(){return {connect(){},disconnect(){}}}
      createAnalyser(){return {connect(){},fftSize:0}}
      createGain(){return {gain:{value:0},connect(){}}}
      createScriptProcessor(){return {connect(){},disconnect(){}}}
      async resume(){if(fixture.holdResume) await new Promise(resolve=>{fixture.resumePending=resolve})} async close(){}
    }
    class RTCPeerConnection {
      constructor(){fixture.peer=this; this.closed=false; this.connectionState='new'; this.iceGatheringState='complete'}
      addTrack(){}
      createDataChannel(label, options){return this.channel={label,ordered:options?.ordered ?? true,readyState:'connecting',close(){this.readyState='closed';this.onclose?.()}}}
      async createOffer(){if(fixture.holdOffer) return await new Promise(resolve=>{fixture.offerPending=resolve}); return {type:'offer',sdp:'synthetic-offer'}}
      async setLocalDescription(offer){this.localDescription=offer}
      close(){this.closed=true;this.connectionState='closed';this.onconnectionstatechange?.()}
    }
    """#
}
