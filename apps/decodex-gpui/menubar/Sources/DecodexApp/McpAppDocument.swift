import Foundation

/// A native, source-bound widget document. Parsing does not grant browser or tool authority.
struct McpAppDocument {
    enum Invalid: Error { case document, resource, policy }

    let item: [String: Any]
    let uri: String
    let html: String
    let contentSecurityPolicy: String
    let preferredDisplayMode: String

    init(data: Data) throws {
        guard data.count <= 6 * 1024 * 1024,
              let document = try JSONSerialization.jsonObject(with: data) as? [String: Any],
              let item = document["item"] as? [String: Any],
              item["type"] as? String == "mcpToolCall",
              let resources = document["resources"] as? [[String: Any]] else {
            throw Invalid.document
        }
        let presentation = item["mcpAppUi"] as? [String: Any]
        let context = item["appContext"] as? [String: Any]
        let selectedURI = Self.nonNull(presentation?["resourceUri"]) ?? Self.nonNull(item["mcpAppResourceUri"]) ?? Self.nonNull(context?["resourceUri"])
        guard let uri = selectedURI as? String, uri.hasPrefix("ui://"), uri.count > 5 else {
            throw Invalid.resource
        }
        let matches = resources.filter { $0["uri"] as? String == uri }
        guard matches.count == 1,
              let resource = matches.first,
              let mime = resource["mimeType"] as? String,
              mime.replacingOccurrences(of: " ", with: "").lowercased() == "text/html;profile=mcp-app" else {
            throw Invalid.resource
        }
        let html: String
        switch (Self.nonNull(resource["text"]), Self.nonNull(resource["blob"])) {
        case (let text as String, nil): html = text
        case (nil, let encoded as String):
            guard let bytes = Data(base64Encoded: encoded), let text = String(data: bytes, encoding: .utf8) else {
                throw Invalid.resource
            }
            html = text
        default: throw Invalid.resource
        }
        let metadata = resource["_meta"] as? [String: Any]
        let ui = metadata?["ui"] as? [String: Any]
        if let policy = ui?["csp"], !(policy is [String: Any]) { throw Invalid.policy }
        let policy = ui?["csp"] as? [String: Any] ?? [:]
        let connect = try Self.origins(policy["connectDomains"], websockets: true)
        let resourcesPolicy = try Self.origins(policy["resourceDomains"])
        let frames = try Self.origins(policy["frameDomains"])
        let bases = try Self.origins(policy["baseUriDomains"])
        self.item = item
        self.uri = uri
        self.html = html
        self.preferredDisplayMode = presentation?["preferredModelDisplayMode"] as? String == "fullscreen" ? "fullscreen" : "inline"
        self.contentSecurityPolicy = [
            "default-src 'none'",
            "script-src 'unsafe-inline' \(resourcesPolicy)",
            "style-src 'unsafe-inline' \(resourcesPolicy)",
            "img-src data: blob: \(resourcesPolicy)",
            "font-src data: \(resourcesPolicy)",
            "media-src data: blob: \(resourcesPolicy)",
            "connect-src \(connect.isEmpty ? "'none'" : connect)",
            "frame-src \(frames.isEmpty ? "'none'" : frames)",
            "base-uri \(bases.isEmpty ? "'none'" : bases)",
            "object-src 'none'", "form-action 'none'"
        ].joined(separator: "; ")
    }

    /// Place this document in an opaque-origin sandbox with allow-scripts only.
    /// The host must deny navigation, media capture, file panels and new windows.
    var sandboxedHTML: String {
        let escaped = contentSecurityPolicy.replacingOccurrences(of: "&", with: "&amp;")
            .replacingOccurrences(of: "\"", with: "&quot;")
        return "<!doctype html><meta http-equiv=\"Content-Security-Policy\" content=\"\(escaped)\">" + html
    }

    private static func nonNull(_ value: Any?) -> Any? { value is NSNull ? nil : value }

    private static func origins(_ value: Any?, websockets: Bool = false) throws -> String {
        guard let value else { return "" }
        guard let domains = value as? [String], domains.count <= 128 else { throw Invalid.policy }
        return try domains.map { origin in
            guard origin.count <= 4096,
                  !origin.contains(where: { $0.isWhitespace || $0 == "'" || $0 == "\"" || $0 == ";" || $0 == "\\" }),
                  let url = URLComponents(string: origin),
                  let scheme = url.scheme, scheme == "https" || (websockets && scheme == "wss"),
                  let host = url.host, !host.isEmpty,
                  url.user == nil, url.password == nil, url.query == nil, url.fragment == nil,
                  url.path.isEmpty || url.path == "/",
                  !host.contains("*") || (host.hasPrefix("*.") && !host.dropFirst(2).contains("*")) else {
                throw Invalid.policy
            }
            return origin.hasSuffix("/") ? String(origin.dropLast()) : origin
        }.joined(separator: " ")
    }
}
