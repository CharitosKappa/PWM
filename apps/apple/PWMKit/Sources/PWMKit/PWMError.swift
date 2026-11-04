@preconcurrency import PWMCore

/// An error from the core or this package: a stable code (D31 §7) and a
/// plain message for the UI. Neither ever contains a value.
public struct PWMError: Error, Sendable, Equatable, CustomStringConvertible {
    public let code: String

    public init(code: String) {
        self.code = code
    }

    init(_ error: PwmError) {
        switch error {
        case let .Failed(code): self.init(code: code)
        }
    }

    public var description: String { "PWMError(\(code))" }

    /// What to show the user: from `design/messages.json`, generated into
    /// Messages.swift (D52 §6).
    public var message: String {
        PWMError.messages[code] ?? PWMError.fallback
    }
}

/// Runs a core call, turning its error into `PWMError`.
func core<T>(_ call: () throws -> T) throws -> T {
    do {
        return try call()
    } catch let error as PwmError {
        throw PWMError(error)
    }
}
