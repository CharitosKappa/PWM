import Foundation

/// A vault item: its D18 JSON object, with typed accessors for the fields
/// this app version knows. Everything else stays in `json` and is written
/// back unchanged, so an older app never drops a newer one's data (D18 §2).
/// The core checks and versions the item when it is saved.
public struct Item: Sendable, Equatable {
    /// The item types of schema v1 (D18 §3).
    public enum Kind: String, Sendable, CaseIterable {
        case login, note, card, identity, folder
    }

    /// The newest schema this app version edits.
    public static let schemaVersion: Int64 = 1

    public private(set) var json: [String: JSONValue]

    /// An item read from the core (`PwmClient.items` / `item`).
    public init(json text: String) throws {
        guard let object = try JSONValue.parse(text).object else {
            throw PWMError(code: "invalid_item")
        }
        json = object
    }

    /// A new item with a fresh id, created now.
    public init(kind: Kind, title: String, id: String, nowMs: Int64) {
        json = [
            "id": .string(id),
            "v": .int(Item.schemaVersion),
            "type": .string(kind.rawValue),
            "title": .string(title),
            "created_at": .int(nowMs),
            "updated_at": .int(nowMs),
        ]
        switch kind {
        case .login: json["login"] = .object([:])
        case .card: json["card"] = .object([:])
        case .identity: json["identity"] = .object([:])
        case .folder: json["folder"] = .object([:])
        case .note: break
        }
    }

    /// JSON text for `PwmClient.saveItem`.
    public func text() throws -> String {
        try JSONValue.object(json).text()
    }

    public var id: String? { json["id"]?.string }
    public var schemaVersion: Int64 { json["v"]?.int ?? 0 }
    /// The raw `type`; `nil` kind means a type from a newer app version.
    public var type: String { json["type"]?.string ?? "" }
    public var kind: Kind? { Kind(rawValue: type) }
    /// Items from a newer schema, or of an unknown type, are shown read-only.
    public var isEditable: Bool { schemaVersion <= Item.schemaVersion && kind != nil }

    public var title: String {
        get { json["title"]?.string ?? "" }
        set { json["title"] = .string(newValue) }
    }

    public var notes: String {
        get { json["notes"]?.string ?? "" }
        set { json["notes"] = .string(newValue) }
    }

    public var favorite: Bool {
        get { json["favorite"]?.bool ?? false }
        set { json["favorite"] = .bool(newValue) }
    }

    public var folderId: String? {
        get { json["folder_id"]?.string }
        set { json["folder_id"] = newValue.map(JSONValue.string) ?? .null }
    }

    public var tags: [String] {
        get { json["tags"]?.array?.compactMap(\.string) ?? [] }
        set { json["tags"] = .array(newValue.map(JSONValue.string)) }
    }

    public var createdAtMs: Int64? { json["created_at"]?.int }
    public var updatedAtMs: Int64? { json["updated_at"]?.int }
    /// Set while the item is in the trash (D19); use `PwmClient.setTrashed`.
    public var trashedAtMs: Int64? { json["trashed_at"]?.int }

    /// The login fields, for `login` items.
    public var login: Login? {
        get { json["login"]?.object.map(Login.init) }
        set { json["login"] = newValue.map { .object($0.json) } ?? .null }
    }
}

/// The `login` object of a login item. Unknown fields are kept.
public struct Login: Sendable, Equatable {
    public private(set) var json: [String: JSONValue]

    public init(json: [String: JSONValue] = [:]) {
        self.json = json
    }

    public var username: String {
        get { json["username"]?.string ?? "" }
        set { json["username"] = .string(newValue) }
    }

    /// A secret: keep it short-lived and never log it (D41 §4).
    public var password: String {
        get { json["password"]?.string ?? "" }
        set { json["password"] = .string(newValue) }
    }

    /// An `otpauth://` URI, if the login has a one-time code.
    public var totp: String? {
        get { json["totp"]?.string }
        set { json["totp"] = newValue.map(JSONValue.string) ?? .null }
    }

    public var urls: [LoginURL] {
        get { json["urls"]?.array?.compactMap { $0.object.map(LoginURL.init) } ?? [] }
        set { json["urls"] = .array(newValue.map { .object($0.json) }) }
    }

    /// How many passkeys the login holds; their keys never leave the core.
    public var passkeyCount: Int { json["passkeys"]?.array?.count ?? 0 }
}

/// A URL of a login with its match mode (`AUTOFILL.md`).
public struct LoginURL: Sendable, Equatable {
    public private(set) var json: [String: JSONValue]

    public init(json: [String: JSONValue]) {
        self.json = json
    }

    public init(url: String, match: String = "domain") {
        json = ["url": .string(url), "match": .string(match)]
    }

    public var url: String {
        get { json["url"]?.string ?? "" }
        set { json["url"] = .string(newValue) }
    }

    /// `domain`, `host`, `starts_with`, `regex`, `exact` or `never`.
    public var match: String {
        get { json["match"]?.string ?? "domain" }
        set { json["match"] = .string(newValue) }
    }
}

extension Item: CustomStringConvertible, CustomDebugStringConvertible, CustomReflectable {
    public var description: String { "Item(\(type), <redacted>)" }
    public var debugDescription: String { description }
    public var customMirror: Mirror { Mirror(self, children: []) }
}

extension Login: CustomStringConvertible, CustomDebugStringConvertible, CustomReflectable {
    public var description: String { "Login(<redacted>)" }
    public var debugDescription: String { description }
    public var customMirror: Mirror { Mirror(self, children: []) }
}
