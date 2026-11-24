import Foundation

/// How a stream is delivered.
enum StreamKind: Equatable, Sendable {
    /// A web address Haven's player can open.
    case direct
    /// A page to open in a web browser.
    case external
    case youTube
    /// A peer-to-peer link Haven cannot play.
    case peerToPeer
}

/// One way to watch a title.
struct MediaStream: Equatable, Sendable {
    /// Short label, often the quality ("1080p").
    var name: String?
    /// Longer text; `description` now, `title` before.
    var title: String?
    var description: String?
    /// A direct address a player can open.
    var url: String?
    var externalURL: String?
    var ytID: String?
    var infoHash: String?
    /// The file's size in bytes, from the `videoSize` hint.
    var videoSize: Int64?
    /// Who provided the stream, when whoever hands it on says so (a media
    /// server that gathers streams names where each came from). Never read
    /// from the stream's own answer.
    var provider: String?

    init(name: String? = nil, title: String? = nil, description: String? = nil, url: String? = nil,
         externalURL: String? = nil, ytID: String? = nil, infoHash: String? = nil,
         videoSize: Int64? = nil, provider: String? = nil) {
        self.name = name
        self.title = title
        self.description = description
        self.url = url
        self.externalURL = externalURL
        self.ytID = ytID
        self.infoHash = infoHash
        self.videoSize = videoSize
        self.provider = provider
    }

    init(json o: JSONObject) throws {
        name = o.textOrNumber("name")
        title = o.textOrNumber("title")
        description = o.textOrNumber("description")
        url = try o.optionalString("url")
        externalURL = try o.optionalString("externalUrl")
        ytID = try o.optionalString("ytId")
        infoHash = try o.optionalString("infoHash")
        videoSize = MediaStream.size(JSONObject(o["behaviorHints"])?["videoSize"])
    }

    /// A size hint sent as a number or text; nothing for zero.
    private static func size(_ value: Any?) -> Int64? {
        let bytes: Int64?
        switch value {
        case let number as NSNumber where !JSONObject.isBool(number) && !JSONObject.isFloat(number):
            bytes = number.int64Value
        case let text as String:
            bytes = Int64(text.trimmingCharacters(in: .whitespacesAndNewlines))
        default:
            bytes = nil
        }
        return bytes.flatMap { $0 > 0 ? $0 : nil }
    }

    /// How the stream is delivered; nil if it names no source at all.
    var kind: StreamKind? {
        func has(_ field: String?) -> Bool {
            !(field?.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ?? true)
        }
        if has(url) { return .direct }
        if has(externalURL) { return .external }
        if has(ytID) { return .youTube }
        if has(infoHash) { return .peerToPeer }
        return nil
    }

    /// The longer text, whichever field was used.
    var details: String? { description ?? title }

    /// Reads a `stream` answer. Entries that are not objects, or that name
    /// no source, are left out.
    static func parseList(_ json: String) throws -> [MediaStream] {
        let answer = try Titles.top(json)
        guard let streams = answer["streams"] else { return [] }
        guard let list = streams as? [Any] else { throw TitlesError.badResponse("streams is not a list") }
        return list
            .compactMap { item in JSONObject(item).flatMap { try? MediaStream(json: $0) } }
            .filter { $0.kind != nil }
    }
}
