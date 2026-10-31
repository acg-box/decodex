// Run the Chrome worker's protocol and ownership checks with system JavaScriptCore.
import CryptoKit
import Foundation
import JavaScriptCore

let directory = URL(fileURLWithPath: CommandLine.arguments[1])
let manifest = try JSONSerialization.jsonObject(with: Data(contentsOf: directory.appendingPathComponent("extension/manifest.json"))) as! [String: Any]
let publicKey = Data(base64Encoded: manifest["key"] as! String)!
let identifier = SHA256.hash(data: publicKey).prefix(16).map { byte in
    String(UnicodeScalar(97 + Int(byte >> 4))!) + String(UnicodeScalar(97 + Int(byte & 15))!)
}.joined()
precondition(identifier == CommandLine.arguments[2], "Native registration must match the actual extension key")

let context = JSContext()!
context.exceptionHandler = { _, error in fputs("\(error?.toString() ?? "JavaScript failure")\n", stderr) }
let parse: @convention(block) (String) -> NSDictionary? = { text in
    guard let parts = URLComponents(string: text), let scheme = parts.scheme, let host = parts.host else { return nil }
    let port = parts.port.map { $0 == 443 && scheme == "https" ? "" : ":\($0)" } ?? ""
    return ["origin": "\(scheme.lowercased())://\(host.lowercased())\(port)", "username": parts.user ?? "", "password": parts.password ?? ""]
}
context.setObject(parse, forKeyedSubscript: "parseURL" as NSString)
context.evaluateScript(try String(contentsOf: directory.appendingPathComponent("tests/extension_fixture.js"), encoding: .utf8))
context.evaluateScript(try String(contentsOf: directory.appendingPathComponent("extension/background.js"), encoding: .utf8))
context.evaluateScript("runChecks().then(() => { testDone = true; }, e => { testError = String(e.message || e) + '\\n' + String(e.stack || ''); testDone = true; });")
let deadline = Date().addingTimeInterval(5)
while context.objectForKeyedSubscript("testDone")?.toBool() != true && Date() < deadline {
    context.evaluateScript("void 0")
    RunLoop.current.run(until: Date().addingTimeInterval(0.001))
}
guard context.objectForKeyedSubscript("testDone")?.toBool() == true else { fatalError("Extension check timed out") }
let failure = context.objectForKeyedSubscript("testError")?.toString() ?? ""
guard failure.isEmpty else { fputs(failure + "\n", stderr); exit(1) }
print("Extension scope, reconnect, framing and manifest identity passed")
