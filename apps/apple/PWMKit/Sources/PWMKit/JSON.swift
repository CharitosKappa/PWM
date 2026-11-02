import Foundation

/// A JSON value, kept whole so fields this app version does not know are
/// written back unchanged (D18 §2). It may hold secrets, so its
/// descriptions are redacted.
public enum JSONValue: Sendable, Equatable {
    case null
    case bool(Bool)
    case int(Int64)
    case double(Double)
    case string(String)
    case array([JSONValue])
    case object([String: JSONValue])

    public var string: String? {
        if case let .string(value) = self { value } else { nil }
    }

    public var bool: Bool? {
        if case let .bool(value) = self { value } else { nil }
    }

    public var int: Int64? {
        switch self {
        case let .int(value): value
        case let .double(value) where value.rounded() == value: Int64(exactly: value)
        default: nil
        }
    }

    public var array: [JSONValue]? {
        if case let .array(value) = self { value } else { nil }
    }

    public var object: [String: JSONValue]? {
        if case let .object(value) = self { value } else { nil }
    }

    /// Parses UTF-8 JSON text.
    public static func parse(_ text: String) throws -> JSONValue {
        do {
            return try JSONDecoder().decode(JSONValue.self, from: Data(text.utf8))
        } catch {
            throw PWMError(code: "invalid_item")
        }
    }

    /// Compact JSON text.
    public func text() throws -> String {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys, .withoutEscapingSlashes]
        guard let text = String(data: try encoder.encode(self), encoding: .utf8) else {
            throw PWMError(code: "invalid_item")
        }
        return text
    }
}

extension JSONValue: Codable {
    public init(from decoder: Decoder) throws {
        let container = try decoder.singleValueContainer()
        if container.decodeNil() {
            self = .null
        } else if let value = try? container.decode(Bool.self) {
            self = .bool(value)
        } else if let value = try? container.decode(Int64.self) {
            self = .int(value)
        } else if let value = try? container.decode(Double.self) {
            self = .double(value)
        } else if let value = try? container.decode(String.self) {
            self = .string(value)
        } else if let value = try? container.decode([JSONValue].self) {
            self = .array(value)
        } else {
            self = .object(try container.decode([String: JSONValue].self))
        }
    }

    public func encode(to encoder: Encoder) throws {
        var container = encoder.singleValueContainer()
        switch self {
        case .null: try container.encodeNil()
        case let .bool(value): try container.encode(value)
        case let .int(value): try container.encode(value)
        case let .double(value): try container.encode(value)
        case let .string(value): try container.encode(value)
        case let .array(value): try container.encode(value)
        case let .object(value): try container.encode(value)
        }
    }
}

extension JSONValue: CustomStringConvertible, CustomDebugStringConvertible, CustomReflectable {
    public var description: String { "JSONValue(<redacted>)" }
    public var debugDescription: String { description }
    public var customMirror: Mirror { Mirror(self, children: []) }
}
