/// Holds a secret (master password, Secret Key, revealed field) that must be
/// kept for a while. Swift strings cannot be wiped (D31 §4), so keep the
/// value short-lived; this wrapper only makes sure printing, logging or
/// `dump` never shows it.
public struct Redacted<Value: Sendable>: Sendable {
    public let value: Value

    public init(_ value: Value) {
        self.value = value
    }
}

extension Redacted: CustomStringConvertible, CustomDebugStringConvertible, CustomReflectable {
    public var description: String { "<redacted>" }
    public var debugDescription: String { "<redacted>" }
    public var customMirror: Mirror { Mirror(self, children: []) }
}

extension Redacted: Equatable where Value: Equatable {}
