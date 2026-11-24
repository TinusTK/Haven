import Foundation

/// Reading the JSON that catalogue services, media servers and add-ons
/// send. The same field can arrive as text, a number, a list or nothing,
/// so the lenient readers accept all of those, as the Rust build did.
/// The strict ones fail on a wrong type, as serde did (which drops that
/// entry, or fails the whole answer, depending on the caller).
struct JSONObject {
    let raw: [String: Any]

    /// Something went wrong reading a field.
    struct Wrong: Error {
        let field: String
    }

    init(_ raw: [String: Any]) {
        self.raw = raw
    }

    init?(_ value: Any?) {
        guard let raw = value as? [String: Any] else { return nil }
        self.raw = raw
    }

    /// Parses JSON text whose top level is an object.
    static func parse(_ text: String) throws -> JSONObject {
        let value = try JSONSerialization.jsonObject(with: Data(text.utf8), options: [.fragmentsAllowed])
        guard let raw = value as? [String: Any] else { throw Wrong(field: "(top)") }
        return JSONObject(raw)
    }

    subscript(key: String) -> Any? {
        let value = raw[key]
        return value is NSNull ? nil : value
    }

    func has(_ key: String) -> Bool { self[key] != nil }

    // MARK: Strict (as serde's String / Option<String> / Vec<String>)

    func string(_ key: String) throws -> String {
        guard let text = self[key] as? String else { throw Wrong(field: key) }
        return text
    }

    /// Missing or null is nil; anything but text is wrong.
    func optionalString(_ key: String) throws -> String? {
        guard let value = self[key] else { return nil }
        guard let text = value as? String else { throw Wrong(field: key) }
        return text
    }

    /// Missing is empty; anything but a list of text is wrong.
    func strings(_ key: String) throws -> [String] {
        guard let value = self[key] else { return [] }
        guard let list = value as? [Any], let texts = list as? [String] else { throw Wrong(field: key) }
        return texts
    }

    /// Missing or null is nil; anything but a list of text is wrong.
    func optionalStrings(_ key: String) throws -> [String]? {
        guard self[key] != nil else { return nil }
        return try strings(key)
    }

    func bool(_ key: String) throws -> Bool {
        guard let value = self[key] else { return false }
        guard let number = value as? NSNumber, JSONObject.isBool(number) else { throw Wrong(field: key) }
        return number.boolValue
    }

    // MARK: Lenient

    /// Text or a number, as text. Anything else, or blank text, is nil.
    func textOrNumber(_ key: String) -> String? {
        JSONObject.text(self[key])
    }

    /// A whole number sent as a number or as text.
    func number(_ key: String) -> Int? {
        switch self[key] {
        case let number as NSNumber where !JSONObject.isBool(number):
            guard !JSONObject.isFloat(number) else {
                // serde's as_u64: only whole numbers written without a point.
                return nil
            }
            let value = number.int64Value
            return value >= 0 && value <= Int64(UInt32.max) ? Int(value) : nil
        case let text as String:
            guard let value = UInt32(text.trimmingCharacters(in: .whitespacesAndNewlines)) else { return nil }
            return Int(value)
        default:
            return nil
        }
    }

    /// A list of text, a single text, or nothing.
    func texts(_ key: String) -> [String] {
        switch self[key] {
        case let list as [Any]: list.compactMap(JSONObject.text)
        case let other: JSONObject.text(other).map { [$0] } ?? []
        }
    }

    static func text(_ value: Any?) -> String? {
        switch value {
        case let text as String:
            return text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? nil : text
        case let number as NSNumber where !isBool(number):
            if isFloat(number) {
                let double = number.doubleValue
                return double.isFinite ? "\(double)" : nil
            }
            return number.stringValue
        default:
            return nil
        }
    }

    static func isBool(_ number: NSNumber) -> Bool {
        CFGetTypeID(number) == CFBooleanGetTypeID()
    }

    /// Written with a point or exponent ("8.4", "8.0"), not a whole number.
    static func isFloat(_ number: NSNumber) -> Bool {
        let type = String(cString: number.objCType)
        return type == "d" || type == "f"
    }
}
