import Foundation
@preconcurrency import PWMCore

/// One of the account's devices (D43 §6).
public struct DeviceEntry: Sendable, Equatable, Identifiable {
    public let id: String
    /// `ios`, `android`, `macos`, `windows`, `linux` or `extension`.
    public let platform: String
    /// `nil` without a name, or when it does not open on this account.
    public let name: String?
    public let added: Date
    public let lastSeen: Date
    public let revoked: Date?
    /// This device.
    public let isCurrent: Bool

    public init(id: String, platform: String, name: String?, added: Date, lastSeen: Date, revoked: Date?, isCurrent: Bool) {
        self.id = id
        self.platform = platform
        self.name = name
        self.added = added
        self.lastSeen = lastSeen
        self.revoked = revoked
        self.isCurrent = isCurrent
    }

    init(_ device: Device) {
        func date(_ ms: Int64) -> Date { Date(timeIntervalSince1970: TimeInterval(ms) / 1000) }
        self.init(
            id: device.id, platform: device.platform, name: device.name,
            added: date(device.createdAt), lastSeen: date(device.lastSeenAt),
            revoked: device.revokedAt.map(date), isCurrent: device.current
        )
    }
}

/// A request from a new device to sign in (D46).
public struct ApprovalEntry: Sendable, Equatable, Identifiable {
    public let id: String
    public let platform: String
    public let created: Date
    /// `pending`, `accepted` or `revealed`.
    public let state: String
    /// The code to compare, once this device accepted and the new device
    /// revealed its key.
    public let code: String?

    public init(id: String, platform: String, created: Date, state: String, code: String?) {
        self.id = id
        self.platform = platform
        self.created = created
        self.state = state
        self.code = code
    }
}

/// The local health checks (D22): weak passwords by item id with their
/// score (below 3), and groups of item ids sharing a password.
public struct PasswordHealth: Sendable, Equatable {
    public let weak: [String: Int]
    public let reused: [[String]]

    public init(weak: [String: Int], reused: [[String]]) {
        self.weak = weak
        self.reused = reused
    }
}

/// Export files (D24).
public enum ExportFile {
    public enum Format: Sendable {
        /// Encrypted with an export password, which opens it alone.
        case pwmx(password: Redacted<String>)
        /// Plaintext: anyone with the file can read every password.
        case json, csv

        public var fileExtension: String {
            switch self {
            case .pwmx: "pwmx"
            case .json: "json"
            case .csv: "csv"
            }
        }
    }
}
