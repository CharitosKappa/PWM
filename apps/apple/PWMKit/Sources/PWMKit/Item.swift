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

    /// The card fields, for `card` items.
    public var card: Card? {
        get { json["card"]?.object.map(Card.init) }
        set { json["card"] = newValue.map { .object($0.json) } ?? .null }
    }

    /// The identity fields, for `identity` items.
    public var identity: Identity? {
        get { json["identity"]?.object.map(Identity.init) }
        set { json["identity"] = newValue.map { .object($0.json) } ?? .null }
    }

    /// Every type has custom fields (D18 §3).
    public var customFields: [CustomField] {
        get { json["custom_fields"]?.array?.compactMap { $0.object.map(CustomField.init) } ?? [] }
        set { json["custom_fields"] = .array(newValue.map { .object($0.json) }) }
    }

    /// A login's earlier passwords from its history (D19), newest first,
    /// each with when it was replaced. Read-only: the core keeps history.
    public var passwordHistory: [PreviousPassword] {
        var newer = login?.password ?? ""
        var out: [PreviousPassword] = []
        for version in json["history"]?.array ?? [] {
            guard let object = version.object,
                  let password = object["login"]?.object?["password"]?.string
            else { continue }
            if !password.isEmpty, password != newer {
                out.append(PreviousPassword(password: password, replacedAtMs: object["saved_at"]?.int))
            }
            newer = password
        }
        return out
    }
}

/// A password a login had before; a secret like the current one.
public struct PreviousPassword: Sendable, Equatable {
    public let password: String
    public let replacedAtMs: Int64?
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

    /// The site each passkey is for (its relying party id).
    public var passkeySites: [String] {
        json["passkeys"]?.array?.compactMap { $0.object?["rp_id"]?.string } ?? []
    }
}

/// The `card` object of a card item. Unknown fields are kept.
public struct Card: Sendable, Equatable {
    public private(set) var json: [String: JSONValue]

    public init(json: [String: JSONValue] = [:]) {
        self.json = json
    }

    public var cardholder: String {
        get { json["cardholder"]?.string ?? "" }
        set { json["cardholder"] = .string(newValue) }
    }

    /// A secret, as typed (spaces kept).
    public var number: String {
        get { json["number"]?.string ?? "" }
        set { json["number"] = .string(newValue) }
    }

    /// The last 4 digits, for lists and search.
    public var lastFour: String { String(number.filter(\.isNumber).suffix(4)) }

    /// 1–12.
    public var expiryMonth: Int? {
        get { json["expiry_month"]?.int.map { Int($0) } }
        set { json["expiry_month"] = newValue.map { .int(Int64($0)) } ?? .null }
    }

    /// Four digits.
    public var expiryYear: Int? {
        get { json["expiry_year"]?.int.map { Int($0) } }
        set { json["expiry_year"] = newValue.map { .int(Int64($0)) } ?? .null }
    }

    /// A secret.
    public var securityCode: String {
        get { json["security_code"]?.string ?? "" }
        set { json["security_code"] = .string(newValue) }
    }

    public var brand: String {
        get { json["brand"]?.string ?? "" }
        set { json["brand"] = .string(newValue) }
    }
}

/// The `identity` object of an identity item. Unknown fields are kept.
public struct Identity: Sendable, Equatable {
    public private(set) var json: [String: JSONValue]

    public init(json: [String: JSONValue] = [:]) {
        self.json = json
    }

    private func text(_ key: String) -> String { json[key]?.string ?? "" }

    public var firstName: String {
        get { text("first_name") }
        set { json["first_name"] = .string(newValue) }
    }

    public var middleName: String {
        get { text("middle_name") }
        set { json["middle_name"] = .string(newValue) }
    }

    public var lastName: String {
        get { text("last_name") }
        set { json["last_name"] = .string(newValue) }
    }

    public var email: String {
        get { text("email") }
        set { json["email"] = .string(newValue) }
    }

    public var phone: String {
        get { text("phone") }
        set { json["phone"] = .string(newValue) }
    }

    public var company: String {
        get { text("company") }
        set { json["company"] = .string(newValue) }
    }

    public var addressLines: [String] {
        get { json["address_lines"]?.array?.compactMap(\.string) ?? [] }
        set { json["address_lines"] = .array(newValue.map(JSONValue.string)) }
    }

    public var city: String {
        get { text("city") }
        set { json["city"] = .string(newValue) }
    }

    public var region: String {
        get { text("region") }
        set { json["region"] = .string(newValue) }
    }

    public var postalCode: String {
        get { text("postal_code") }
        set { json["postal_code"] = .string(newValue) }
    }

    public var country: String {
        get { text("country") }
        set { json["country"] = .string(newValue) }
    }
}

/// A custom field of any item. Unknown fields are kept.
public struct CustomField: Sendable, Equatable {
    /// How the field is shown.
    public enum Kind: String, Sendable, CaseIterable {
        case text, hidden, url, date
    }

    public private(set) var json: [String: JSONValue]

    public init(json: [String: JSONValue]) {
        self.json = json
    }

    public init(name: String, value: String, kind: Kind) {
        json = ["name": .string(name), "value": .string(value), "kind": .string(kind.rawValue)]
    }

    public var name: String {
        get { json["name"]?.string ?? "" }
        set { json["name"] = .string(newValue) }
    }

    /// A secret when the kind is `hidden` or unknown.
    public var value: String {
        get { json["value"]?.string ?? "" }
        set { json["value"] = .string(newValue) }
    }

    /// A missing kind is `text`, as in the core; `nil` is a kind from a
    /// newer app version, shown masked and kept unchanged.
    public var kind: Kind? {
        get { json["kind"]?.string.map { Kind(rawValue: $0) } ?? .text }
        set { json["kind"] = .string((newValue ?? .text).rawValue) }
    }
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

extension Card: CustomStringConvertible, CustomDebugStringConvertible, CustomReflectable {
    public var description: String { "Card(<redacted>)" }
    public var debugDescription: String { description }
    public var customMirror: Mirror { Mirror(self, children: []) }
}

extension Identity: CustomStringConvertible, CustomDebugStringConvertible, CustomReflectable {
    public var description: String { "Identity(<redacted>)" }
    public var debugDescription: String { description }
    public var customMirror: Mirror { Mirror(self, children: []) }
}

extension CustomField: CustomStringConvertible, CustomDebugStringConvertible, CustomReflectable {
    public var description: String { "CustomField(<redacted>)" }
    public var debugDescription: String { description }
    public var customMirror: Mirror { Mirror(self, children: []) }
}

extension PreviousPassword: CustomStringConvertible, CustomDebugStringConvertible, CustomReflectable {
    public var description: String { "PreviousPassword(<redacted>)" }
    public var debugDescription: String { description }
    public var customMirror: Mirror { Mirror(self, children: []) }
}
